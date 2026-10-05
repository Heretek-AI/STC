//! Axum router: SPA static assets + `/api` projection reads + the ONE write
//! endpoint (`POST /api/ask/:id/decide`, A1 one-tap decision through the P02
//! approval service CAS).
//!
//! Browser discipline: every `/api` route registers a GET handler only,
//! EXCEPT the decide path which registers POST only — all other methods fall
//! through to `405 Method Not Allowed` (pinned by
//! `write_surface_is_decide_only`, extended to HEAD/OPTIONS/TRACE/CONNECT).
//! `GET` routes also answer `HEAD` via axum's `get()` (200 with headers, no
//! body — safe by construction, pinned as 405-or-safe). There are NO other
//! write endpoints. The server owns the projection (hub over `studio.db`);
//! the browser reads.

use axum::{
    extract::{
        rejection::{JsonRejection, QueryRejection},
        Path, Query, State,
    },
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{sse::Event, Html, IntoResponse, Sse},
    routing::{get, post},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::convert::Infallible;
use std::future::Future as _;
use std::sync::{Arc, Mutex};
use studio_core::approvals::{self, decide_stored, ApprovalError};
use studio_core::state::StateStore;
use tower_http::services::ServeDir;

use crate::api::{
    AgentEventDto, AgentStreamDto, AskFeedDto, AskItemDto, AuditDto, AuthProbeDto, BucketDto,
    BudgetDto, DaemonDto, DecideBody, DecideOutcomeDto, FeedbackDto, GapKind, HealthDto,
    HistoryDto, HookProviderDto, KanbanCardDto, KanbanColumnDto, ManifestSource, McpAgentDto,
    McpRegistryDto, NotificationDto, ProbeDto, ProvidersDto, RunSlotDto, RunsDto, TaskDto,
    WarRoomDto,
};
use crate::daemon;
use crate::hub::Hub;
use crate::staleness::{now_ms, staleness_of};

/// Cached probe verdict: (epoch ms of the probe, pass flag, legible detail).
type CachedProbe = (u64, bool, String);

#[derive(Clone, Default)]
pub struct ProbeCache {
    inner: Arc<Mutex<HashMap<String, CachedProbe>>>,
}

/// Health-probe cache with TTL (f05-openfang-providers clean-room).
/// `fresh` is honest: true ONLY when the verdict was served from cache
/// inside the TTL (no new probe ran); a just-executed probe reports
/// `fresh: false` ("re-probed") with its new `last_probe_ms`. Pass/fail
/// rides inside `detail` in both cases.
pub const PROBE_TTL_MS: u64 = 30_000;

fn probe_cached(
    cache: &ProbeCache,
    name: &str,
    ttl_ms: u64,
    run: impl FnOnce() -> String,
) -> ProbeDto {
    let now = now_ms();
    if let Some((at, _ok, detail)) = cache.inner.lock().expect("probe mutex").get(name).cloned() {
        if now.saturating_sub(at) < ttl_ms {
            return ProbeDto {
                name: name.into(),
                fresh: true,
                last_probe_ms: at,
                ttl_ms,
                detail,
            };
        }
    }
    let detail = run();
    cache
        .inner
        .lock()
        .expect("probe mutex")
        .insert(name.into(), (now, true, detail.clone()));
    ProbeDto {
        name: name.into(),
        fresh: false,
        last_probe_ms: now,
        ttl_ms,
        detail,
    }
}

#[derive(Clone)]
pub struct AppState {
    pub hub: Hub,
    pub db_path: String,
    pub static_dir: std::path::PathBuf,
    pub version: &'static str,
    pub probe_cache: ProbeCache,
}

/// Read latency of one handler call, used as the honest `db_age_ms` for
/// direct-read views (the data was read Nms ago — just now).
fn timed<T>(f: impl FnOnce() -> T) -> (T, u64) {
    let t0 = std::time::Instant::now();
    let v = f();
    (v, t0.elapsed().as_millis() as u64)
}

/// Short badge source: the DB basename (`ms1.db`). By design the badge
/// keeps the short name (ms1_gate asserts the exact badge text); the
/// distinguishing absolute path is always in `/api/health.db_path`
/// (see `health_handler`) so two DBs named `studio.db` in different dirs
/// never collide observably — pinned by `db_source_keeps_basename`.
fn db_source(db_path: &str) -> String {
    std::path::Path::new(db_path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "studio.db".into())
}

/// Typed JSON for query-string rejections (fix 5): axum's default is
/// `text/plain`; the F1 typed-JSON promise requires `application/json`
/// with an `error` field for every 400. Invalid `since`/`limit` (e.g.
/// `?since=abc`, `?limit=1.5`) lands here.
fn bad_query_response(err: QueryRejection) -> axum::response::Response {
    let status = err.status();
    (
        status,
        Json(json!({"error": "bad_query", "detail": err.to_string()})),
    )
        .into_response()
}

/// Typed JSON for decide-body rejections (fix 5): 415 wrong content-type,
/// 400 malformed JSON, 422 missing/wrong-typed fields — all JSON typed,
/// never `text/plain`. The `error` code mirrors the status.
fn bad_body_response(err: JsonRejection) -> axum::response::Response {
    let status = err.status();
    let code = match status {
        StatusCode::UNSUPPORTED_MEDIA_TYPE => "unsupported_media_type",
        StatusCode::UNPROCESSABLE_ENTITY => "unprocessable_entity",
        _ => "bad_request",
    };
    (
        status,
        Json(json!({"error": code, "detail": err.to_string()})),
    )
        .into_response()
}

/// Retry guidance headers on `Lost` (fix 8): `X-Resync-From` names the
/// `floor_seq` to re-request from, `Retry-After: 1` tells slow consumers
/// to back off one second before resyncing (never retry the stale cursor).
fn lost_headers(floor_seq: i64) -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
    if let Ok(v) = HeaderValue::from_str(&floor_seq.to_string()) {
        h.insert("x-resync-from", v);
    }
    h
}

fn daemon_status() -> DaemonDto {
    daemon::probe(env!("CARGO_PKG_VERSION"))
}

/// GET /api/status — the V7 catalog entry. 503 (legible JSON) when no poll
/// ever succeeded (DB missing): fail closed, never an empty-200 masquerading
/// as data. F4: `?since=` threads the caller's event cursor into the
/// taxonomy (cursor below the ring floor OR ahead of the head → `Lost`,
/// including future cursors and old-epoch stale cursors); omitted → age
/// verdict. On `Lost` the response carries `X-Resync-From` + `Retry-After`.
async fn status_handler(
    State(st): State<Arc<AppState>>,
    query: Result<Query<HistoryParams>, QueryRejection>,
) -> impl IntoResponse {
    let p = match query {
        Ok(q) => q.0,
        Err(e) => return bad_query_response(e),
    };
    let daemon_reachable = daemon_status().reachable;
    match st.hub.status_view(daemon_reachable, p.since) {
        Some(mut dto) => {
            dto.daemon_reachable = daemon_reachable;
            if dto.staleness.state == crate::api::StalenessState::Lost {
                let floor = st.hub.hub_report().floor_seq;
                (StatusCode::OK, lost_headers(floor), Json(dto)).into_response()
            } else {
                (StatusCode::OK, Json(dto)).into_response()
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "projection_unavailable",
                "detail": format!("no successful DB read yet for {} — is studio.db initialized (`studio init`)?", st.db_path),
            })),
        )
            .into_response(),
    }
}

/// GET /api/ask — A1 Ask queue over the P02 approval store.
/// P05: the one-tap decision path is LIVE (`POST /api/ask/:id/decide` via
/// `decide_stored` CAS). `decision_enabled` is true for `pending` rows;
/// decided rows stay visibly disabled (single-shot, never double-decides).
async fn ask_handler(State(st): State<Arc<AppState>>) -> impl IntoResponse {
    let items: Vec<AskItemDto> = StateStore::open_readonly(&st.db_path)
        .ok()
        .and_then(|store| approvals::list_requests(&store, 100).ok())
        .map(|rows| {
            rows.into_iter()
                .map(|r| {
                    let pending = r.status == approvals::ApprovalStatus::Pending;
                    AskItemDto {
                        id: r.id,
                        task_id: r.task_id,
                        tool: r.tool,
                        status: r.status.as_str().to_owned(),
                        reason: r.reason,
                        created_ms: r.created_ms,
                        decided_ms: r.decided_ms,
                        decision_enabled: pending,
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let feed = AskFeedDto {
        source: db_source(&st.db_path),
        decision_contract: "one-tap decision is live: POST /api/ask/:id/decide {approved, reason} — CAS on status='pending' via the P02 approval service; every decision lands in contract_approvals (durable ledger); decided rows stay disabled (single-shot).".into(),
        items,
    };
    (StatusCode::OK, Json(feed)).into_response()
}

/// POST /api/ask/:id/decide — the A1 one-tap decision (THIS phase wires it).
/// Body: `{approved: bool, reason: String}` (reason 1..=1024 chars, counted
/// in chars not bytes). Typed outcomes: 200 (durable ledger row) · 400 bad
/// reason/bad JSON · 404 unknown request (consistent JSON shape for encoded
/// slashes; overlong/control ids are 404 without echo) · 409 already decided
/// · 415 wrong content-type · 422 missing/wrong-typed fields · 503 DB
/// unwritable. The single-shot CAS in `decide_stored` is the only write
/// path; everything else stays GET-only.
/// Route parsing (fix 11): `id` is bound to 1..=256 chars; `/`, NUL and
/// control chars are invalid (never echoed — no `%00` leak); overlong ids
/// are typed 404 without echo.
pub const DECIDE_ID_MAX_CHARS: usize = 256;

fn decide_id_invalid(id: &str) -> bool {
    let n = id.chars().count();
    n == 0
        || n > DECIDE_ID_MAX_CHARS
        || id.contains('\0')
        || id.contains('/')
        || id.chars().any(|c| c.is_control())
}

async fn decide_handler(
    State(st): State<Arc<AppState>>,
    Path(id): Path<String>,
    body: Result<Json<DecideBody>, JsonRejection>,
) -> impl IntoResponse {
    if decide_id_invalid(&id) {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "unknown_request", "detail": "no approval (invalid id)"})),
        )
            .into_response();
    }
    let body = match body {
        Ok(b) => b.0,
        Err(e) => return bad_body_response(e),
    };
    let reason = body.reason.trim();
    // Fix 10: chars not bytes — 1024 `é` (2048 bytes) is 1024 chars, valid.
    let reason_chars = reason.chars().count();
    if reason_chars == 0 || reason_chars > 1024 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "bad_reason", "detail": "reason must be 1..=1024 chars"})),
        )
            .into_response();
    }
    let store = match StateStore::open(&st.db_path) {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error": "projection_unavailable", "detail": format!("db not writable: {e}")})),
            )
                .into_response();
        }
    };
    match decide_stored(&store, &id, body.approved, reason) {
        Ok(status) => {
            let decided = approvals::load_request(&store, &id)
                .ok()
                .flatten()
                .and_then(|r| r.decided_ms);
            (
                StatusCode::OK,
                Json(DecideOutcomeDto {
                    id: id.clone(),
                    status: status.as_str().into(),
                    decided_ms: decided,
                }),
            )
                .into_response()
        }
        Err(ApprovalError::UnknownRequest { .. }) => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "unknown_request", "detail": format!("no approval {id}")})),
        )
            .into_response(),
        Err(ApprovalError::AlreadyDecided { status, .. }) => (
            StatusCode::CONFLICT,
            Json(json!({"error": "already_decided", "detail": format!("{id} is {status}: decisions are single-shot")})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "projection_unavailable", "detail": e.to_string()})),
        )
            .into_response(),
    }
}

/// Attention bucket for one task status (f05-paseo-buckets ordering).
/// `needs_input` (a pending approval names this task) outranks everything.
fn bucket_for(status: &str, needs_input: bool) -> (u32, &'static str) {
    if needs_input {
        return (0, "needs_input");
    }
    match status {
        "failed" | "error" => (1, "failed"),
        "building" | "running" | "pending" | "ready" | "dispatched" => (2, "running"),
        "validating" | "in-review" | "review" => (3, "attention"),
        "done" | "merged" | "ready-to-merge" => (4, "done"),
        _ => (2, "running"),
    }
}

/// GET /api/war-room — attention-first buckets + permission notifications.
/// Pure read: hub snapshot tasks × pending approvals from the durable store.
/// Badge parity (fix 8): `?since=` threads the cursor into the staleness
/// taxonomy via `seq_gap_for` (floor/future/epoch); omitted stays honest
/// age-only by design (documented). On `Lost` the response carries
/// `X-Resync-From` + `Retry-After`.
async fn war_room_handler(
    State(st): State<Arc<AppState>>,
    query: Result<Query<HistoryParams>, QueryRejection>,
) -> impl IntoResponse {
    let q = match query {
        Ok(v) => v.0,
        Err(e) => return bad_query_response(e),
    };
    let source = db_source(&st.db_path);
    let ((snap_opt, approvals_opt), age) = timed(|| {
        let store = StateStore::open_readonly(&st.db_path).ok();
        let snap = store.as_ref().and_then(|s| s.snapshot().ok());
        let appr = store
            .as_ref()
            .and_then(|s| approvals::list_requests(s, 512).ok());
        (snap, appr)
    });
    let Some(snap) = snap_opt else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "projection_unavailable", "detail": format!("no successful DB read yet for {} — is studio.db initialized (`studio init`)?", st.db_path)})),
        )
            .into_response();
    };
    let stale_after = crate::staleness::stale_after_ms();
    let mut pending_by_task: HashMap<String, Vec<(String, String)>> = HashMap::new();
    let mut notifications = Vec::new();
    for r in approvals_opt.unwrap_or_default() {
        if r.status == approvals::ApprovalStatus::Pending {
            pending_by_task
                .entry(r.task_id.clone())
                .or_default()
                .push((r.id.clone(), r.tool.clone()));
            notifications.push(NotificationDto {
                id: r.id.clone(),
                task_id: r.task_id.clone(),
                tool: r.tool.clone(),
                kind: "permission".into(),
                label: format!("approve/deny {} on {}", r.tool, r.task_id),
            });
        }
    }
    notifications.sort_by(|a, b| a.id.cmp(&b.id));
    // All five buckets are always emitted (empty ones render "bucket clear":
    // an attention board must show the absence, not hide the bucket).
    let mut slots: Vec<(&'static str, Vec<TaskDto>)> = vec![
        ("needs_input", Vec::new()),
        ("failed", Vec::new()),
        ("running", Vec::new()),
        ("attention", Vec::new()),
        ("done", Vec::new()),
    ];
    for t in &snap.fleet {
        let (_order, name) = bucket_for(&t.status, pending_by_task.contains_key(&t.id));
        if let Some(slot) = slots.iter_mut().find(|(n, _)| *n == name) {
            slot.1.push(TaskDto {
                id: t.id.clone(),
                kind: t.kind.clone(),
                status: t.status.clone(),
                lane: t.lane.clone(),
            });
        }
    }
    let out: Vec<BucketDto> = slots
        .into_iter()
        .enumerate()
        .map(|(order, (name, mut tasks))| {
            tasks.sort_by(|a, b| a.id.cmp(&b.id));
            BucketDto {
                name: name.into(),
                order: order as u32,
                tasks,
            }
        })
        .collect();
    let dto = WarRoomDto {
        source: source.clone(),
        db_age_ms: age,
        staleness: staleness_of(
            &source,
            age,
            snap.change_seq,
            stale_after,
            st.hub.seq_gap_for(q.since),
        ),
        change_seq: snap.change_seq,
        buckets: out,
        notifications,
    };
    if dto.staleness.state == crate::api::StalenessState::Lost {
        let floor = st.hub.hub_report().floor_seq;
        (StatusCode::OK, lost_headers(floor), Json(dto)).into_response()
    } else {
        (StatusCode::OK, Json(dto)).into_response()
    }
}

/// The HookProvider protocol version this server speaks (f05-px-events).
pub const HOOK_PROTOCOL_VERSION: u32 = 1;

/// Split a CDC `new` payload into (provider, protocol_version): declared
/// inside the JSON when present, otherwise the studio-cdc default.
fn provider_of(new: &Option<String>) -> (String, u32) {
    let v: Option<serde_json::Value> = new.as_deref().and_then(|s| serde_json::from_str(s).ok());
    let provider = v
        .as_ref()
        .and_then(|j| j.get("provider"))
        .and_then(|p| p.as_str())
        .unwrap_or("studio-cdc")
        .to_owned();
    let version = v
        .as_ref()
        .and_then(|j| j.get("protocol_version"))
        .and_then(|n| n.as_u64())
        .map(|n| n as u32)
        .unwrap_or(HOOK_PROTOCOL_VERSION);
    (provider, version)
}

/// Normalize one CDC row into the AgentEvent union. Unknown provider
/// versions are REFUSED (kind `unknown`, `refused: true`) — surfaced, never
/// executed, never dropped silently. Unknown tables with a supported version
/// surface as `unknown` with `refused: false`.
fn normalize_cdc(seq: i64, tbl: &str, row: &str, op: &str, new: &Option<String>) -> AgentEventDto {
    let (provider, version) = provider_of(new);
    let compatible = version == HOOK_PROTOCOL_VERSION;
    let (kind, refused) = if !compatible {
        ("unknown", true)
    } else {
        match tbl {
            "tasks" => ("status", false),
            "contract_approvals" => ("message", false),
            "events" => ("message", false),
            _ => ("unknown", false),
        }
    };
    AgentEventDto {
        seq,
        kind: kind.into(),
        provider,
        protocol_version: version,
        payload: format!("{tbl}:{row}:{op}"),
        ts_ms: None,
        refused,
    }
}

/// GET /api/agent-stream — normalized AgentEvent union over the hub ring
/// (`?since=`/`limit=` cursor with Lost gap semantics) + latest agent-origin
/// `events` rows + HookProvider protocol record. Unknown versions refused.
/// `limit` threads into both the history page and the SSE-equivalent event
/// slice with the shared [1,1024] clamp (fix 3); negative clamps to 1 like
/// HTTP. On `Lost` the response carries `X-Resync-From` + `Retry-After`.
async fn agent_stream_handler(
    State(st): State<Arc<AppState>>,
    query: Result<Query<HistoryParams>, QueryRejection>,
) -> impl IntoResponse {
    let p = match query {
        Ok(v) => v.0,
        Err(e) => return bad_query_response(e),
    };
    let source = db_source(&st.db_path);
    let limit = p.limit.unwrap_or(128).clamp(1, 1024) as usize;
    let page = st.hub.history(p.since.unwrap_or(0), limit);
    let events: Vec<AgentEventDto> = page
        .events
        .iter()
        .map(|e| normalize_cdc(e.seq, &e.tbl, &e.row, &e.op, &e.new))
        .collect();
    let mut providers: Vec<HookProviderDto> = vec![HookProviderDto {
        name: "studio-cdc".into(),
        protocol_version: HOOK_PROTOCOL_VERSION,
        compatible: true,
    }];
    for e in &events {
        if e.provider != "studio-cdc" && !providers.iter().any(|x| x.name == e.provider) {
            providers.push(HookProviderDto {
                name: e.provider.clone(),
                protocol_version: e.protocol_version,
                compatible: e.protocol_version == HOOK_PROTOCOL_VERSION,
            });
        }
    }
    providers.sort_by(|a, b| a.name.cmp(&b.name));
    let stale_after = crate::staleness::stale_after_ms();
    // Age of the hub's last good read: reuse the status view's verdict when
    // available so the badge matches Status; else fail closed 503.
    let seq_gap = page.gap == GapKind::Lost;
    let Some(status) = st.hub.status_view(daemon_status().reachable, None) else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "projection_unavailable", "detail": format!("no successful DB read yet for {} — is studio.db initialized (`studio init`)?", st.db_path)})),
        )
            .into_response();
    };
    let dto = AgentStreamDto {
        source: source.clone(),
        db_age_ms: status.db_age_ms,
        staleness: staleness_of(
            &source,
            status.db_age_ms,
            status.change_seq,
            stale_after,
            seq_gap,
        ),
        change_seq: status.change_seq,
        gap: page.gap,
        floor_seq: page.floor_seq,
        provider_protocol_version: HOOK_PROTOCOL_VERSION,
        providers,
        events,
    };
    if dto.staleness.state == crate::api::StalenessState::Lost {
        (StatusCode::OK, lost_headers(page.floor_seq), Json(dto)).into_response()
    } else {
        (StatusCode::OK, Json(dto)).into_response()
    }
}

/// GET /api/audit — kanban board (tasks by lane + receipt evidence) +
/// head-SHA-fenced feedback (frozen candidates; a row applies only at its
/// recorded head — `current_head` marks the live fence per target).
/// Badge parity (fix 8): `?since=` threads like war-room; omitted is
/// honest age-only. On `Lost` carries retry headers.
async fn audit_handler(
    State(st): State<Arc<AppState>>,
    query: Result<Query<HistoryParams>, QueryRejection>,
) -> impl IntoResponse {
    let q = match query {
        Ok(v) => v.0,
        Err(e) => return bad_query_response(e),
    };
    let source = db_source(&st.db_path);
    let ((snap_opt, frozen), age) = timed(|| {
        let store = StateStore::open_readonly(&st.db_path).ok();
        let snap = store.as_ref().and_then(|s| s.snapshot().ok());
        let frozen = store
            .as_ref()
            .and_then(|s| s.frozen_candidate_rows().ok())
            .unwrap_or_default();
        (snap, frozen)
    });
    let Some(snap) = snap_opt else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "projection_unavailable", "detail": format!("no successful DB read yet for {} — is studio.db initialized (`studio init`)?", st.db_path)})),
        )
            .into_response();
    };
    let stale_after = crate::staleness::stale_after_ms();
    let mut evidence_by_task: HashMap<String, Vec<String>> = HashMap::new();
    for r in &snap.receipts {
        evidence_by_task
            .entry(r.task_id.clone())
            .or_default()
            .push(r.id.clone());
    }
    // Receipt evidence links a freeze fence to its task (evidence naming the
    // freeze hash); unlinked fences render with an empty task id, honestly.
    let mut task_by_freeze: HashMap<String, String> = HashMap::new();
    for r in &snap.receipts {
        for f in &frozen {
            if r.evidence.contains(&f.freeze_hash) {
                task_by_freeze
                    .entry(f.freeze_hash.clone())
                    .or_insert_with(|| r.task_id.clone());
            }
        }
    }
    let mut head_by_target: HashMap<String, String> = HashMap::new();
    for f in &frozen {
        head_by_target.insert(f.target_ref.clone(), f.freeze_hash.clone());
    }
    let mut cols: HashMap<String, Vec<KanbanCardDto>> = HashMap::new();
    for t in &snap.fleet {
        let mut ev = evidence_by_task.get(&t.id).cloned().unwrap_or_default();
        ev.sort();
        cols.entry(t.lane.clone()).or_default().push(KanbanCardDto {
            id: t.id.clone(),
            kind: t.kind.clone(),
            status: t.status.clone(),
            evidence: ev,
        });
    }
    let mut columns: Vec<KanbanColumnDto> = cols
        .into_iter()
        .map(|(name, mut cards)| {
            cards.sort_by(|a, b| a.id.cmp(&b.id));
            KanbanColumnDto { name, cards }
        })
        .collect();
    columns.sort_by(|a, b| a.name.cmp(&b.name));
    let mut feedback: Vec<FeedbackDto> = frozen
        .iter()
        .map(|f| FeedbackDto {
            id: format!("fb-{}", &f.freeze_hash[..f.freeze_hash.len().min(12)]),
            task_id: task_by_freeze
                .get(&f.freeze_hash)
                .cloned()
                .unwrap_or_default(),
            head_sha: f.freeze_hash.clone(),
            target_ref: f.target_ref.clone(),
            tier: f.tier.clone(),
            body: format!("frozen {} r{} → {}", f.tier, f.revision, f.target_ref),
            current_head: head_by_target
                .get(&f.target_ref)
                .is_some_and(|h| h == &f.freeze_hash),
        })
        .collect();
    feedback.sort_by(|a, b| a.id.cmp(&b.id));
    let dto = AuditDto {
        source: source.clone(),
        db_age_ms: age,
        staleness: staleness_of(
            &source,
            age,
            snap.change_seq,
            stale_after,
            st.hub.seq_gap_for(q.since),
        ),
        change_seq: snap.change_seq,
        columns,
        feedback,
    };
    if dto.staleness.state == crate::api::StalenessState::Lost {
        let floor = st.hub.hub_report().floor_seq;
        (StatusCode::OK, lost_headers(floor), Json(dto)).into_response()
    } else {
        (StatusCode::OK, Json(dto)).into_response()
    }
}

/// Bundled per-agent detection manifest (f05-herdr-detect PATTERN ONLY).
/// (binary, display name, manifest version). ghostty-vt is explicitly NOT a
/// dependency (Zig toolchain) — detection is a PATH probe, nothing linked.
const BUNDLED_AGENTS: &[(&str, &str, &str)] = &[
    ("opencode", "opencode", "1"),
    ("claude", "claude", "1"),
    ("codex", "codex", "1"),
    ("gemini", "gemini", "1"),
    ("auggie", "auggie", "1"),
    ("cursor-agent", "cursor-agent", "1"),
];

fn path_probe(binary: &str) -> Option<String> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let full = dir.join(binary);
            full.is_file().then(|| full.to_string_lossy().into_owned())
        })
    })
}

fn agent_state(detected: bool, daemon_reachable: bool) -> &'static str {
    match (detected, daemon_reachable) {
        (true, true) => "working",
        (true, false) => "blocked",
        (false, _) => "idle",
    }
}

/// GET /api/mcp — per-agent detection manifests (bundled PATH probes +
/// optional JSON override file via `STUDIO_MCP_MANIFEST` + remote status via
/// `STUDIO_MCP_MANIFEST_URL`, listed never fetched) with explain strings and
/// working/blocked/idle flags from real signals.
/// Badge parity (fix 8): `?since=` threads like war-room; omitted is honest
/// age-only. On `Lost` carries retry headers.
async fn mcp_handler(
    State(st): State<Arc<AppState>>,
    query: Result<Query<HistoryParams>, QueryRejection>,
) -> impl IntoResponse {
    let q = match query {
        Ok(v) => v.0,
        Err(e) => return bad_query_response(e),
    };
    let source = db_source(&st.db_path);
    let (seq_opt, age) = timed(|| {
        StateStore::open_readonly(&st.db_path)
            .ok()
            .and_then(|s| s.max_change_seq().ok())
    });
    let stale_after = crate::staleness::stale_after_ms();
    let daemon_reachable = daemon_status().reachable;
    let mut agents: Vec<McpAgentDto> = BUNDLED_AGENTS
        .iter()
        .map(|(bin, name, ver)| {
            let found = path_probe(bin);
            McpAgentDto {
                name: (*name).into(),
                manifest_source: ManifestSource::Bundled,
                version: (*ver).into(),
                detected: found.is_some(),
                explain: match &found {
                    Some(p) => {
                        format!("bundled manifest v{ver}: binary `{bin}` found in PATH at {p}")
                    }
                    None => format!("bundled manifest v{ver}: binary `{bin}` not found in PATH"),
                },
                state: agent_state(found.is_some(), daemon_reachable).into(),
            }
        })
        .collect();
    // Override manifest: a real JSON file when configured, parse errors
    // reported as a legible blocked record (fail closed, never silent).
    if let Ok(path) = std::env::var("STUDIO_MCP_MANIFEST") {
        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<serde_json::Value>(&text) {
                Ok(v) => {
                    let list = v
                        .get("agents")
                        .and_then(|a| a.as_array())
                        .cloned()
                        .unwrap_or_default();
                    for a in list {
                        let name = a
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("unnamed")
                            .to_owned();
                        let ver = a
                            .get("version")
                            .and_then(|n| n.as_str())
                            .unwrap_or("?")
                            .to_owned();
                        let found = path_probe(&name);
                        agents.push(McpAgentDto {
                            name: name.clone(),
                            manifest_source: ManifestSource::Override,
                            version: ver.clone(),
                            detected: found.is_some(),
                            explain: format!(
                                "override manifest {path} v{ver}: {}",
                                match &found {
                                    Some(p) => format!("binary `{name}` found at {p}"),
                                    None => format!("binary `{name}` not in PATH"),
                                }
                            ),
                            state: agent_state(found.is_some(), daemon_reachable).into(),
                        });
                    }
                }
                Err(e) => agents.push(McpAgentDto {
                    name: "override-manifest".into(),
                    manifest_source: ManifestSource::Override,
                    version: "unknown".into(),
                    detected: false,
                    explain: format!("override manifest {path} unparseable: {e}"),
                    state: "blocked".into(),
                }),
            },
            Err(e) => agents.push(McpAgentDto {
                name: "override-manifest".into(),
                manifest_source: ManifestSource::Override,
                version: "unknown".into(),
                detected: false,
                explain: format!("override manifest {path} unreadable: {e}"),
                state: "blocked".into(),
            }),
        }
    }
    // Remote manifest: listed, never fetched silently.
    match std::env::var("STUDIO_MCP_MANIFEST_URL") {
        Ok(url) => agents.push(McpAgentDto {
            name: "remote-manifest".into(),
            manifest_source: ManifestSource::Remote,
            version: "unknown".into(),
            detected: false,
            explain: format!("remote manifest {url} listed but never fetched silently — set STUDIO_MCP_MANIFEST for a local override"),
            state: "idle".into(),
        }),
        Err(_) => agents.push(McpAgentDto {
            name: "remote-manifest".into(),
            manifest_source: ManifestSource::Remote,
            version: "unknown".into(),
            detected: false,
            explain: "no remote manifest configured (STUDIO_MCP_MANIFEST_URL unset) — unknown fallback".into(),
            state: "idle".into(),
        }),
    }
    agents.sort_by(|a, b| a.name.cmp(&b.name));
    let seq = seq_opt.unwrap_or(0);
    let dto = McpRegistryDto {
        source: source.clone(),
        db_age_ms: age,
        staleness: staleness_of(&source, age, seq, stale_after, st.hub.seq_gap_for(q.since)),
        change_seq: seq,
        agents,
    };
    if dto.staleness.state == crate::api::StalenessState::Lost {
        let floor = st.hub.hub_report().floor_seq;
        (StatusCode::OK, lost_headers(floor), Json(dto)).into_response()
    } else {
        (StatusCode::OK, Json(dto)).into_response()
    }
}

/// Credential-file presence probe (f05-oh-acp-authprobe): existence only —
/// file contents (secrets) are never read.
fn cred_file_present(path: &str) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.is_file())
}

fn home_join(rel: &str) -> Option<String> {
    std::env::var("HOME").ok().map(|h| format!("{h}/{rel}"))
}

/// GET /api/providers — health-probe cache with TTL + budgets over the real
/// `token_ledger` + per-harness ACP auth-status probes (presence checks real,
/// inapplicable mechanisms report `unknown`, never fake-ok).
/// Badge parity (fix 8): `?since=` threads like war-room; omitted is honest
/// age-only. On `Lost` carries retry headers.
async fn providers_handler(
    State(st): State<Arc<AppState>>,
    query: Result<Query<HistoryParams>, QueryRejection>,
) -> impl IntoResponse {
    let q = match query {
        Ok(v) => v.0,
        Err(e) => return bad_query_response(e),
    };
    let source = db_source(&st.db_path);
    let ((budgets, lifetime, seq_opt), age) = timed(|| {
        let store = StateStore::open_readonly(&st.db_path).ok();
        let budgets = store
            .as_ref()
            .and_then(|s| s.token_ledger_by_session().ok())
            .unwrap_or_default();
        let lifetime = store
            .as_ref()
            .and_then(|s| s.lifetime_spend().ok())
            .unwrap_or(0);
        let seq = store.as_ref().and_then(|s| s.max_change_seq().ok());
        (budgets, lifetime, seq)
    });
    let stale_after = crate::staleness::stale_after_ms();
    let daemon_probe = probe_cached(&st.probe_cache, "daemon", PROBE_TTL_MS, || {
        let d = daemon_status();
        format!(
            "{}: reachable={} ({}; {})",
            "daemon", d.reachable, d.pid_file, d.detail
        )
    });
    let db_probe =
        probe_cached(
            &st.probe_cache,
            "db",
            PROBE_TTL_MS,
            || match StateStore::open_readonly(&st.db_path) {
                Ok(s) => match s.schema_version() {
                    Ok(v) => format!("db ok: {} readable, schema_version={v}", st.db_path),
                    Err(e) => format!("db readable but schema unreadable: {e}"),
                },
                Err(e) => format!("db unreachable: {e}"),
            },
        );
    let budgets_dto: Vec<BudgetDto> = budgets
        .into_iter()
        .map(|b| BudgetDto {
            session: b.session,
            total: b.total,
            cache_hits: b.cache_hits,
        })
        .collect();
    let mut auth = Vec::new();
    for (harness, rel) in [
        ("opencode", ".local/share/opencode/auth.json"),
        ("claude", ".claude/.credentials.json"),
        ("codex", ".codex/auth.json"),
    ] {
        match home_join(rel) {
            Some(p) if cred_file_present(&p) => auth.push(AuthProbeDto {
                harness: harness.into(),
                method: "credential-file".into(),
                status: "present".into(),
                detail: format!("{rel} exists (presence only — contents never read)"),
            }),
            Some(_) => auth.push(AuthProbeDto {
                harness: harness.into(),
                method: "credential-file".into(),
                status: "missing".into(),
                detail: format!("{rel} absent — harness not onboarded here"),
            }),
            None => auth.push(AuthProbeDto {
                harness: harness.into(),
                method: "credential-file".into(),
                status: "unknown".into(),
                detail: "HOME unset — cannot check (non-unix residual)".into(),
            }),
        }
    }
    // Mechanisms with no checkable path report unknown (never fake-ok).
    for (harness, method) in [("claude", "loggedIn-json"), ("codex", "stderr-text")] {
        auth.push(AuthProbeDto {
            harness: harness.into(),
            method: method.into(),
            status: "unknown".into(),
            detail: "no live harness probe configured — credential-file presence used instead"
                .into(),
        });
    }
    auth.sort_by(|a, b| a.harness.cmp(&b.harness).then(a.method.cmp(&b.method)));
    let seq = seq_opt.unwrap_or(0);
    let dto = ProvidersDto {
        source: source.clone(),
        db_age_ms: age,
        staleness: staleness_of(&source, age, seq, stale_after, st.hub.seq_gap_for(q.since)),
        change_seq: seq,
        probes: vec![daemon_probe, db_probe],
        budgets: budgets_dto,
        lifetime_total: lifetime,
        auth,
    };
    if dto.staleness.state == crate::api::StalenessState::Lost {
        let floor = st.hub.hub_report().floor_seq;
        (StatusCode::OK, lost_headers(floor), Json(dto)).into_response()
    } else {
        (StatusCode::OK, Json(dto)).into_response()
    }
}

/// GET /api/runs — multi-run compare slots (f05-openchamber-multirun, capped
/// at 5) over real `worktrees` rows + receipt evidence linkage (a receipt
/// belongs to a run when its task id names the run slug).
///
/// Per-model columns + guided walkthrough are DEFERRED: schema v2 tasks
/// carry no run/model linkage, so a per-model matrix would be mock data —
/// stated, not faked.
/// Badge parity (fix 8): `?since=` threads like war-room; omitted is honest
/// age-only. On `Lost` carries retry headers.
async fn runs_handler(
    State(st): State<Arc<AppState>>,
    query: Result<Query<HistoryParams>, QueryRejection>,
) -> impl IntoResponse {
    let q = match query {
        Ok(v) => v.0,
        Err(e) => return bad_query_response(e),
    };
    let source = db_source(&st.db_path);
    let ((records, snap_opt), age) = timed(|| {
        let store = StateStore::open_readonly(&st.db_path).ok();
        let records = store
            .as_ref()
            .and_then(|s| s.worktree_records().ok())
            .unwrap_or_default();
        let snap = store.as_ref().and_then(|s| s.snapshot().ok());
        (records, snap)
    });
    let stale_after = crate::staleness::stale_after_ms();
    let receipts = snap_opt
        .as_ref()
        .map(|s| s.receipts.clone())
        .unwrap_or_default();
    let seq = snap_opt.as_ref().map(|s| s.change_seq).unwrap_or(0);
    let mut runs: Vec<RunSlotDto> = records
        .into_iter()
        .take(5)
        .map(|w| {
            let n = receipts
                .iter()
                .filter(|r| {
                    r.task_id == w.slug
                        || r.task_id.contains(&w.slug)
                        || r.evidence.contains(&w.slug)
                })
                .count();
            RunSlotDto {
                slug: w.slug,
                path: w.path,
                state: w.state,
                receipts: n,
            }
        })
        .collect();
    runs.sort_by(|a, b| a.slug.cmp(&b.slug));
    let dto = RunsDto {
        source: source.clone(),
        db_age_ms: age,
        staleness: staleness_of(&source, age, seq, stale_after, st.hub.seq_gap_for(q.since)),
        change_seq: seq,
        runs,
        deferred: "per-model columns + guided changes walkthrough need task→run/model linkage (schema change) — deferred; run slots + evidence counts above are live worktree/receipt data.".into(),
    };
    if dto.staleness.state == crate::api::StalenessState::Lost {
        let floor = st.hub.hub_report().floor_seq;
        (StatusCode::OK, lost_headers(floor), Json(dto)).into_response()
    } else {
        (StatusCode::OK, Json(dto)).into_response()
    }
}

#[derive(Debug, Deserialize)]
struct HistoryParams {
    since: Option<i64>,
    /// Signed so `?limit=-1` parses and clamps to 1 like HTTP (fix 3);
    /// non-numeric (`abc`, `1.5`, empty) still rejects as typed 400 JSON.
    limit: Option<i64>,
}

/// Clamp the signed `limit` query into the shared [1,1024] window.
fn clamp_limit(limit: Option<i64>) -> usize {
    limit.unwrap_or(128).clamp(1, 1024) as usize
}

/// GET /api/events?since=N&limit=M — history cursor with Lost gap semantics.
/// Future cursors (`since > head`) and stale epoch cursors are `Lost` with
/// no rows (fixes 1-2); on `Lost` the response carries `X-Resync-From` +
/// `Retry-After` (fix 8). Invalid queries are typed 400 JSON (fix 5).
async fn events_handler(
    State(st): State<Arc<AppState>>,
    query: Result<Query<HistoryParams>, QueryRejection>,
) -> impl IntoResponse {
    let p = match query {
        Ok(v) => v.0,
        Err(e) => return bad_query_response(e),
    };
    let dto: HistoryDto = st.hub.history(p.since.unwrap_or(0), clamp_limit(p.limit));
    if dto.gap == GapKind::Lost {
        (StatusCode::OK, lost_headers(dto.floor_seq), Json(dto)).into_response()
    } else {
        (StatusCode::OK, Json(dto)).into_response()
    }
}

/// GET /api/events/stream?since=N&limit=M — SSE: replay missed history, then
/// live. A `gap` marker is emitted whenever the cursor predates the floor
/// OR runs ahead of the head (future cursor, fixes 1-2). `limit` threads
/// from the query with the shared [1,1024] clamp (fix 3, no 512 hardcode);
/// negative clamps to 1 like HTTP. No `ready` frame on SSE by design (fix
/// 12, WS-only). Liveness is poll-driven at the CDC cadence: the stream
/// re-reads the hub ring rather than holding a broadcast subscription, so a
/// slow consumer can never silently resume mid-gap. Invalid queries are
/// typed 400 JSON (fix 5), never `text/plain`.
async fn stream_handler(
    State(st): State<Arc<AppState>>,
    query: Result<Query<HistoryParams>, QueryRejection>,
) -> impl IntoResponse {
    let p = match query {
        Ok(v) => v.0,
        Err(e) => return bad_query_response(e),
    };
    let since = p.since.unwrap_or(0).max(0);
    let limit = clamp_limit(p.limit);
    Sse::new(PollStream::new(st.hub.clone(), since, limit)).into_response()
}

/// Poll-driven SSE stream over the hub ring. Hand-rolled `Stream` (one tiny
/// `futures-core` dep, no stream crates): the sleep poll registers the task
/// waker correctly. `limit` bounds the initial replay AND live refresh pages
/// (fix 3); on `Lost` the cursor jumps to `head_seq` (resync guidance).
struct PollStream {
    hub: Hub,
    cursor: i64,
    limit: usize,
    pending: std::vec::IntoIter<serde_json::Value>,
    sleep: std::pin::Pin<Box<tokio::time::Sleep>>,
}

impl PollStream {
    fn new(hub: Hub, since: i64, limit: usize) -> Self {
        let limit = limit.clamp(1, 1024);
        let since = since.max(0);
        let page = hub.history(since, limit);
        let mut pending = Vec::with_capacity(page.events.len() + 1);
        if page.gap == GapKind::Lost {
            pending.push(json!({"gap": "Lost", "head_seq": page.head_seq}));
        }
        for e in page.events {
            pending.push(serde_json::to_value(e).unwrap_or_default());
        }
        // On Lost jump to head (resync guidance); otherwise track max.
        let cursor = if page.gap == GapKind::Lost {
            page.head_seq
        } else {
            page.head_seq.max(since)
        };
        Self {
            hub,
            cursor,
            limit,
            pending: pending.into_iter(),
            sleep: Box::pin(tokio::time::sleep(std::time::Duration::from_millis(0))),
        }
    }

    fn refresh_from_ring(&mut self) {
        let page = self.hub.history(self.cursor, self.limit);
        let mut pending = Vec::with_capacity(page.events.len() + 1);
        if page.gap == GapKind::Lost {
            self.cursor = page.head_seq;
            pending.push(json!({"gap": "Lost", "head_seq": page.head_seq}));
        }
        for e in page.events {
            pending.push(serde_json::to_value(e).unwrap_or_default());
        }
        self.pending = pending.into_iter();
    }
}

impl futures_core::Stream for PollStream {
    type Item = Result<Event, Infallible>;
    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        if let Some(v) = self.pending.next() {
            if let Some(seq) = v.get("seq").and_then(|s| s.as_i64()) {
                self.cursor = self.cursor.max(seq);
            }
            let ev = if v.get("gap").is_some() {
                Event::default()
                    .event("gap")
                    .json_data(v)
                    .unwrap_or_default()
            } else {
                Event::default()
                    .event("event")
                    .json_data(v)
                    .unwrap_or_default()
            };
            return std::task::Poll::Ready(Some(Ok(ev)));
        }
        self.refresh_from_ring();
        if let Some(v) = self.pending.next() {
            if let Some(seq) = v.get("seq").and_then(|s| s.as_i64()) {
                self.cursor = self.cursor.max(seq);
            }
            let ev = if v.get("gap").is_some() {
                Event::default()
                    .event("gap")
                    .json_data(v)
                    .unwrap_or_default()
            } else {
                Event::default()
                    .event("event")
                    .json_data(v)
                    .unwrap_or_default()
            };
            return std::task::Poll::Ready(Some(Ok(ev)));
        }
        // Nothing new: park on the CDC cadence (registers the waker).
        match self.sleep.as_mut().poll(cx) {
            std::task::Poll::Ready(()) => {
                self.sleep
                    .as_mut()
                    .reset(tokio::time::Instant::now() + std::time::Duration::from_millis(100));
                cx.waker().wake_by_ref();
                std::task::Poll::Pending
            }
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }
}

/// GET /api/health — liveness. Always 200 + structured body (missing daemon
/// or DB reported legibly inside, never a panic dump).
async fn health_handler(State(st): State<Arc<AppState>>) -> impl IntoResponse {
    let daemon = daemon_status();
    let (db_ok, schema_version) = match StateStore::open_readonly(&st.db_path) {
        Ok(s) => (true, s.schema_version().ok()),
        Err(_) => (false, None),
    };
    let dto = HealthDto {
        version: st.version.to_owned(),
        db_path: st.db_path.clone(),
        db_ok,
        schema_version,
        daemon,
        hub: st.hub.hub_report(),
    };
    (StatusCode::OK, Json(dto)).into_response()
}

async fn api_fallback() -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        Json(json!({"error": "unknown_api_route"})),
    )
}

/// Build the router. F1: the `/api` scope is a NESTED router with its own
/// JSON 404 (`unknown_api_route`) — unknown API paths can never fall through
/// to the SPA static service (previously `fallback_service` shadowed the API
/// fallback and served `index.html` as text/html for `/api/*` typos).
pub fn build_router(state: AppState) -> axum::Router {
    let shared = Arc::new(state);
    let static_dir = shared.static_dir.clone();
    let has_dist = static_dir.join("index.html").is_file();
    let api = axum::Router::new()
        .route("/status", get(status_handler))
        .route("/ask", get(ask_handler))
        .route("/ask/{id}/decide", post(decide_handler))
        .route("/war-room", get(war_room_handler))
        .route("/agent-stream", get(agent_stream_handler))
        .route("/audit", get(audit_handler))
        .route("/mcp", get(mcp_handler))
        .route("/providers", get(providers_handler))
        .route("/runs", get(runs_handler))
        .route("/events", get(events_handler))
        .route("/events/stream", get(stream_handler))
        .route("/health", get(health_handler))
        .fallback(api_fallback);
    let app = axum::Router::new().nest("/api", api).with_state(shared);
    if has_dist {
        let spa = ServeDir::new(&static_dir).not_found_service(
            tower_http::services::ServeFile::new(static_dir.join("index.html")),
        );
        app.fallback_service(spa)
    } else {
        // No dist: non-/api paths get a legible 503 HTML instead of a tower
        // 404 — fail closed, legible. The /api JSON 404 above is unaffected.
        // F1 note: the explicit `/api/{*api_rest}` route is required here
        // because the `/{*path}` wildcard would otherwise shadow the nested
        // /api fallback (explicit routes beat nested fallbacks); with a dist
        // the SPA fallback_service is itself lowest-priority, so no pin needed.
        app.route("/api/{*api_rest}", get(api_fallback))
            .route("/", get(missing_frontend))
            .route("/{*path}", get(missing_frontend))
    }
}

async fn missing_frontend() -> impl IntoResponse {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Html(
            "<h1>cockpit not built</h1><p>Run <code>npm run build</code> in cockpit/ — the API is still live at /api/*.</p>",
        ),
    )
}

/// DTO key set for the red-on-stale contract test: changing [`StatusDto`]
/// without updating this const AND the fixtures fails the gate.
pub const EXPECTED_STATUS_KEYS: &[&str] = &[
    "source",
    "db_age_ms",
    "staleness",
    "change_seq",
    "counts",
    "tasks",
    "daemon_reachable",
];

/// Red-on-stale key sets for the P05 views (same rule as
/// [`EXPECTED_STATUS_KEYS`]: DTO change without const + fixture updates
/// fails the gate).
pub const EXPECTED_WAR_ROOM_KEYS: &[&str] = &[
    "source",
    "db_age_ms",
    "staleness",
    "change_seq",
    "buckets",
    "notifications",
];

pub const EXPECTED_AGENT_STREAM_KEYS: &[&str] = &[
    "source",
    "db_age_ms",
    "staleness",
    "change_seq",
    "gap",
    "floor_seq",
    "provider_protocol_version",
    "providers",
    "events",
];

pub const EXPECTED_AUDIT_KEYS: &[&str] = &[
    "source",
    "db_age_ms",
    "staleness",
    "change_seq",
    "columns",
    "feedback",
];

pub const EXPECTED_MCP_KEYS: &[&str] =
    &["source", "db_age_ms", "staleness", "change_seq", "agents"];

pub const EXPECTED_PROVIDERS_KEYS: &[&str] = &[
    "source",
    "db_age_ms",
    "staleness",
    "change_seq",
    "probes",
    "budgets",
    "lifetime_total",
    "auth",
];

pub const EXPECTED_RUNS_KEYS: &[&str] = &[
    "source",
    "db_age_ms",
    "staleness",
    "change_seq",
    "runs",
    "deferred",
];

pub const EXPECTED_ASK_KEYS: &[&str] = &["source", "decision_contract", "items"];

/// Sorted top-level keys of a JSON value (empty vec for non-objects).
pub fn status_keys(v: &serde_json::Value) -> Vec<String> {
    v.as_object()
        .map(|m| {
            let mut ks: Vec<String> = m.keys().cloned().collect();
            ks.sort();
            ks
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::GapKind;
    use studio_core::state::StateStore;

    fn seeded_db(n_tasks: usize) -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("studio.db");
        let p = path.to_str().unwrap().to_owned();
        let store = StateStore::open(&p).unwrap();
        for i in 0..n_tasks.max(1) {
            store
                .upsert_task(&format!("t{i}"), "coder", "building")
                .unwrap();
        }
        (dir, p)
    }

    #[tokio::test]
    async fn v1_sse_parity_gap_marker_then_live() {
        // V1 SSE parity: PollStream replays history identically — a cursor
        // below the floor yields a leading {"gap":"Lost"} marker, then the
        // live rows; cursor advances to head. FAILS pre-fix if the stream
        // ever resumes mid-gap silently or mis-tracks the cursor.
        let (_dir, p) = seeded_db(10);
        let hub = Hub::new_with_cap(p, 2000, 4);
        assert!(hub.poll_once());
        let page = hub.history(0, 512);
        assert_eq!(page.gap, GapKind::Lost);
        let floor = page.floor_seq;
        let head = page.head_seq;
        // Stale cursor: gap marker first, cursor jumps to head.
        let mut stream = PollStream::new(hub.clone(), 0, 512);
        assert_eq!(stream.cursor, head.max(0));
        let first: Vec<serde_json::Value> = stream.pending.by_ref().collect();
        assert!(!first.is_empty());
        assert_eq!(first[0].get("gap").and_then(|g| g.as_str()), Some("Lost"));
        assert_eq!(
            first[0].get("head_seq").and_then(|h| h.as_i64()),
            Some(head)
        );
        assert_eq!(first.len(), 1, "Lost carries no rows, only the marker");
        // Fresh cursor at the floor: no gap, contiguous live rows.
        let mut live = PollStream::new(hub.clone(), floor, 512);
        assert_eq!(live.cursor, head.max(floor));
        let rows: Vec<serde_json::Value> = live.pending.by_ref().collect();
        assert!(!rows.is_empty());
        assert!(rows.iter().all(|v| v.get("gap").is_none()));
        assert_eq!(rows.len(), 4);
        let mut prev = floor;
        for v in &rows {
            let seq = v.get("seq").and_then(|s| s.as_i64()).unwrap();
            assert!(seq > prev);
            prev = seq;
        }
        assert_eq!(prev, head);
    }

    #[tokio::test]
    async fn v1_sse_since_floor_never_spurious_gap() {
        // V1 second leg: since<0 clamps (F2) — SSE from a nonsense cursor on
        // a wide ring must NOT emit a spurious gap marker.
        let (_dir, p) = seeded_db(3);
        let hub = Hub::new_with_cap(p, 2000, 512);
        assert!(hub.poll_once());
        let mut stream = PollStream::new(hub, -5, 512);
        let pending: Vec<serde_json::Value> = stream.pending.by_ref().collect();
        assert!(pending.iter().all(|v| v.get("gap").is_none()));
        assert_eq!(pending.len(), 3);
    }

    #[tokio::test]
    async fn sse_limit_threads_into_pollstream() {
        // Fix 3: PollStream threads limit (no 512 hardcode); negative-equivalent
        // (0) clamps to 1 like HTTP. FAILS pre-fix (10 rows despite limit=1).
        let (_dir, p) = seeded_db(10);
        let hub = Hub::new_with_cap(p, 2000, 512);
        assert!(hub.poll_once());
        let mut one = PollStream::new(hub.clone(), 0, 1);
        let rows: Vec<serde_json::Value> = one.pending.by_ref().collect();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get("seq").and_then(|s| s.as_i64()), Some(1));
        let mut clamped = PollStream::new(hub.clone(), 0, 0);
        let rows: Vec<serde_json::Value> = clamped.pending.by_ref().collect();
        assert_eq!(rows.len(), 1, "limit 0 must clamp to 1");
        let mut neg = PollStream::new(hub.clone(), -5, 1);
        let rows: Vec<serde_json::Value> = neg.pending.by_ref().collect();
        assert_eq!(rows.len(), 1, "clamped negative cursor honors limit");
        // Future cursor still Lost with the threaded limit (no rows).
        let mut fut = PollStream::new(hub.clone(), 999_999, 1);
        let rows: Vec<serde_json::Value> = fut.pending.by_ref().collect();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get("gap").and_then(|g| g.as_str()), Some("Lost"));
        // No Ready frame on SSE by design (fix 12, WS-only).
        for v in rows {
            assert!(v.get("ready").is_none(), "SSE must carry no Ready");
        }
    }
}
