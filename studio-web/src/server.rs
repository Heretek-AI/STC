//! Axum router: SPA static assets + `/api` projection reads.
//!
//! Browser is read-only by construction: every `/api` route registers a GET
//! handler only, so POST/PUT/DELETE/PATCH fall through to `405 Method Not
//! Allowed` (pinned by `browser_is_read_only`). There are NO write endpoints.
//! The server owns the projection (hub over `studio.db`); the browser reads.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{sse::Event, Html, IntoResponse, Sse},
    routing::get,
    Json,
};
use serde::Deserialize;
use serde_json::json;
use std::convert::Infallible;
use std::future::Future as _;
use std::sync::Arc;
use studio_core::approvals;
use studio_core::state::StateStore;
use tower_http::services::ServeDir;

use crate::api::{AskFeedDto, AskItemDto, DaemonDto, GapKind, HealthDto, HistoryDto};
use crate::daemon;
use crate::hub::Hub;

#[derive(Clone)]
pub struct AppState {
    pub hub: Hub,
    pub db_path: String,
    pub static_dir: std::path::PathBuf,
    pub version: &'static str,
}

fn daemon_status() -> DaemonDto {
    daemon::probe(env!("CARGO_PKG_VERSION"))
}

/// GET /api/status — the V7 catalog entry. 503 (legible JSON) when no poll
/// ever succeeded (DB missing): fail closed, never an empty-200 masquerading
/// as data. F4: `?since=` threads the caller's event cursor into the
/// taxonomy (cursor below the ring floor → `Lost`); omitted → age verdict.
async fn status_handler(
    State(st): State<Arc<AppState>>,
    Query(p): Query<HistoryParams>,
) -> impl IntoResponse {
    let daemon_reachable = daemon_status().reachable;
    match st.hub.status_view(daemon_reachable, p.since) {
        Some(mut dto) => {
            dto.daemon_reachable = daemon_reachable;
            (StatusCode::OK, Json(dto)).into_response()
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

/// GET /api/ask — A1 read-only Ask queue over the P02 approval store.
/// `decision_enabled: false` on every row: no decide endpoint exists in P04.
async fn ask_handler(State(st): State<Arc<AppState>>) -> impl IntoResponse {
    let items: Vec<AskItemDto> = StateStore::open_readonly(&st.db_path)
        .ok()
        .and_then(|store| approvals::list_requests(&store, 100).ok())
        .map(|rows| {
            rows.into_iter()
                .map(|r| AskItemDto {
                    id: r.id,
                    task_id: r.task_id,
                    tool: r.tool,
                    status: r.status.as_str().to_owned(),
                    reason: r.reason,
                    created_ms: r.created_ms,
                    decided_ms: r.decided_ms,
                    decision_enabled: false,
                })
                .collect()
        })
        .unwrap_or_default();
    let feed = AskFeedDto {
        source: std::path::Path::new(&st.db_path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "studio.db".into()),
        decision_contract: "read-only in P04: one-tap decision is honored only where the approval service exposes it (no decide endpoint yet) — buttons render disabled, never fake-enabled. Decide-later wiring lands with P05.".into(),
        items,
    };
    (StatusCode::OK, Json(feed)).into_response()
}

#[derive(Debug, Deserialize)]
struct HistoryParams {
    since: Option<i64>,
    limit: Option<usize>,
}

/// GET /api/events?since=N&limit=M — history cursor with Lost gap semantics.
async fn events_handler(
    State(st): State<Arc<AppState>>,
    Query(p): Query<HistoryParams>,
) -> impl IntoResponse {
    let dto: HistoryDto = st.hub.history(p.since.unwrap_or(0), p.limit.unwrap_or(128));
    (StatusCode::OK, Json(dto)).into_response()
}

/// GET /api/events/stream?since=N — SSE: replay missed history, then live.
/// A `gap` marker is emitted whenever the cursor predates the floor
/// (Lost/Unavailable semantics). Liveness is poll-driven at the CDC cadence:
/// the stream re-reads the hub ring rather than holding a broadcast
/// subscription, so a slow consumer can never silently resume mid-gap — any
/// eviction under the cursor surfaces as a `gap` marker.
async fn stream_handler(
    State(st): State<Arc<AppState>>,
    Query(p): Query<HistoryParams>,
) -> impl IntoResponse {
    Sse::new(PollStream::new(st.hub.clone(), p.since.unwrap_or(0)))
}

/// Poll-driven SSE stream over the hub ring. Hand-rolled `Stream` (one tiny
/// `futures-core` dep, no stream crates): the sleep poll registers the task
/// waker correctly.
struct PollStream {
    hub: Hub,
    cursor: i64,
    pending: std::vec::IntoIter<serde_json::Value>,
    sleep: std::pin::Pin<Box<tokio::time::Sleep>>,
}

impl PollStream {
    fn new(hub: Hub, since: i64) -> Self {
        let page = hub.history(since, 512);
        let mut pending = Vec::with_capacity(page.events.len() + 1);
        if page.gap == GapKind::Lost {
            pending.push(json!({"gap": "Lost", "head_seq": page.head_seq}));
        }
        for e in page.events {
            pending.push(serde_json::to_value(e).unwrap_or_default());
        }
        let cursor = page.head_seq.max(since);
        Self {
            hub,
            cursor,
            pending: pending.into_iter(),
            sleep: Box::pin(tokio::time::sleep(std::time::Duration::from_millis(0))),
        }
    }

    fn refresh_from_ring(&mut self) {
        let page = self.hub.history(self.cursor, 512);
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
