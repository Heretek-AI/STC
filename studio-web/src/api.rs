//! API DTOs for the cockpit shell (P04).
//!
//! Every contract type derives `ts-rs::TS`; the TypeScript mirror is
//! generated (see [`export_ts`]) and committed at
//! `cockpit/src/api-types.ts`. The contract test
//! `ts_export_matches_committed` fails red when the DTO changes without
//! regenerating — the DTO is the single source of truth, never the `.ts`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Fresh/stale/lost taxonomy (V8). Time-stale vs sequence-lost are distinct:
/// - `Fresh`: data age within budget and no sequence gap.
/// - `Stale`: time-stale — the last successful DB read is older than budget.
/// - `Lost`: sequence-lost — the client's cursor (or the live stream) fell
///   behind the retained ring floor; the client must re-sync from `head_seq`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub enum StalenessState {
    Fresh,
    Stale,
    Lost,
}

/// Staleness badge payload. `label` is the exact badge text the UI renders
/// (ms1_gate.sh asserts on it); machines read `state`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct StalenessDto {
    pub state: StalenessState,
    pub label: String,
    #[ts(type = "number")]
    pub db_age_ms: u64,
    #[ts(type = "number")]
    pub change_seq: i64,
}

/// One task row (projection of `tasks`, never canonical state).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TaskDto {
    pub id: String,
    pub kind: String,
    pub status: String,
    pub lane: String,
}

/// Lane counts for the status view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct CountsDto {
    pub building: usize,
    pub validating: usize,
    pub in_review: usize,
    pub ready: usize,
}

/// Status view payload — the ONE catalog entry of V7 (`status`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct StatusDto {
    pub source: String,
    #[ts(type = "number")]
    pub db_age_ms: u64,
    pub staleness: StalenessDto,
    #[ts(type = "number")]
    pub change_seq: i64,
    pub counts: CountsDto,
    pub tasks: Vec<TaskDto>,
    pub daemon_reachable: bool,
}

/// One Ask queue row (read-only projection of `contract_approvals`).
/// `decision_enabled` is false for every row in P04: there is no POST decide
/// endpoint, so the UI renders the button visibly disabled (never
/// fake-enabled). Decide-later wiring lands with P05.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AskItemDto {
    pub id: String,
    pub task_id: String,
    pub tool: String,
    pub status: String,
    pub reason: Option<String>,
    #[ts(type = "number")]
    pub created_ms: u64,
    #[ts(type = "number | null")]
    pub decided_ms: Option<u64>,
    pub decision_enabled: bool,
}

/// Read-only Ask queue feed (A1 scope).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AskFeedDto {
    pub source: String,
    pub decision_contract: String,
    pub items: Vec<AskItemDto>,
}

/// One CDC event (projection of `change_log`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct EventDto {
    #[ts(type = "number")]
    pub seq: i64,
    pub tbl: String,
    pub row: String,
    pub op: String,
    pub old: Option<String>,
    pub new: Option<String>,
}

/// History gap semantics (f04-herdr-api c032-c033 clean-room):
/// - `None`: `events` cover `(since, head_seq]` contiguously.
/// - `Lost`: `since` predates the retained floor — rows were dropped from the
///   ring and the client must re-sync from `head_seq`.
///
/// There is no silent truncation: overflow is always reported as `Lost`.
///
/// V4/V5 WS contract pre-registration (deferred transport, settled shape):
/// the future WebSocket transport (P05+) MUST reuse this exact `GapKind`
/// (`"None"` / `"Lost"` over the wire) and the `Ready` envelope below —
/// `{"ready": {"head_seq": N, "floor_seq": M, "gap": <GapKind>}}` as the first
/// WS frame after subscribe, then `event` frames identical to the SSE
/// `EventDto` JSON. No WS endpoint exists in this phase (SSE only per F2);
/// this doc + `v4_v5_ws_prereg_gapkind_serde` pin the wire shape so the
/// later transport cannot silently diverge (e.g. renaming `Lost` or dropping
/// the floor). F2 ViewShell keep: the shared envelope (badge/states/retry)
/// stays transport-agnostic; only the frame pump changes.
///
/// WS Ready deferral (fix 12, explicitly pinned): the `Ready` envelope is
/// WS-ONLY. SSE (`/api/events/stream`) carries NO `ready` frame — a stale
/// cursor yields a `gap` marker (`{"gap":"Lost","head_seq":H}`), a fresh
/// cursor yields `event` frames directly. An `Upgrade: websocket` request to
/// the SSE endpoint is never 101 (still 200 `text/event-stream`); the
/// contract test `ws_ready_deferred_no_upgrade` pins this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub enum GapKind {
    None,
    Lost,
}

/// History cursor page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct HistoryDto {
    #[ts(type = "number")]
    pub floor_seq: i64,
    #[ts(type = "number")]
    pub head_seq: i64,
    pub gap: GapKind,
    pub events: Vec<EventDto>,
}

/// Daemon ensure-running probe (f04-sandbox-daemon semantics inform this).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DaemonDto {
    pub reachable: bool,
    pub pid_file: String,
    pub version_file: String,
    pub pid: Option<u32>,
    pub version: Option<String>,
    pub detail: String,
}

/// Hub retention report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct HubDto {
    #[ts(type = "number")]
    pub floor_seq: i64,
    #[ts(type = "number")]
    pub head_seq: i64,
    pub retained: usize,
    pub cap: usize,
}

/// Liveness report. Always 200 with a structured body: a missing daemon or
/// an unreadable DB is reported legibly here (fail closed), never a panic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct HealthDto {
    pub version: String,
    pub db_path: String,
    pub db_ok: bool,
    #[ts(type = "number | null")]
    pub schema_version: Option<i64>,
    pub daemon: DaemonDto,
    pub hub: HubDto,
}

// ---------------------------------------------------------------------------
// P05 operator views (all read projections with staleness badges; UI never
// owns state). Every DTO below derives `ts-rs::TS` and is exported by
// [`export_ts`]; the red-on-stale contract test fails when a DTO changes
// without regenerating `cockpit/src/api-types.ts`.
// ---------------------------------------------------------------------------

/// One attention bucket (f05-paseo-buckets clean-room, Apache-2.0).
/// Display order is `order` ascending: needs_input(0) > failed(1) >
/// running(2) > attention(3) > done(4) — deterministic cockpit ordering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct BucketDto {
    pub name: String,
    #[ts(type = "number")]
    pub order: u32,
    pub tasks: Vec<TaskDto>,
}

/// Permission notification kind (f05-paseo-buckets): a pending approval
/// surfaced as an attention payload (desktop notification pattern only —
/// no mobile code, no push infra).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct NotificationDto {
    pub id: String,
    pub task_id: String,
    pub tool: String,
    pub kind: String,
    pub label: String,
}

/// War Room payload: attention-first task buckets + permission notifications.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct WarRoomDto {
    pub source: String,
    #[ts(type = "number")]
    pub db_age_ms: u64,
    pub staleness: StalenessDto,
    #[ts(type = "number")]
    pub change_seq: i64,
    pub buckets: Vec<BucketDto>,
    pub notifications: Vec<NotificationDto>,
}

/// One normalized agent event (f05-px-events clean-room, MIT).
/// `kind` is the normalized union (`message` | `tool_call` | `tool_result` |
/// `status` | `unknown`); rows declaring an unsupported `protocol_version`
/// are surfaced as `unknown` with `refused: true` — refused, never dropped
/// silently and never executed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AgentEventDto {
    #[ts(type = "number")]
    pub seq: i64,
    pub kind: String,
    pub provider: String,
    #[ts(type = "number")]
    pub protocol_version: u32,
    pub payload: String,
    #[ts(type = "number | null")]
    pub ts_ms: Option<u64>,
    pub refused: bool,
}

/// Hook provider interface record (f05-px-events): name + protocol version.
/// `compatible` is false for unknown versions — the server refuses them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct HookProviderDto {
    pub name: String,
    #[ts(type = "number")]
    pub protocol_version: u32,
    pub compatible: bool,
}

/// Agent Stream payload: normalized CDC events + latest agent-origin rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AgentStreamDto {
    pub source: String,
    #[ts(type = "number")]
    pub db_age_ms: u64,
    pub staleness: StalenessDto,
    #[ts(type = "number")]
    pub change_seq: i64,
    pub gap: GapKind,
    #[ts(type = "number")]
    pub floor_seq: i64,
    #[ts(type = "number")]
    pub provider_protocol_version: u32,
    pub providers: Vec<HookProviderDto>,
    pub events: Vec<AgentEventDto>,
}

/// One kanban card (f05-vk-kanban-diff clean-room, Apache-2.0): task +
/// receipt evidence ids.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct KanbanCardDto {
    pub id: String,
    pub kind: String,
    pub status: String,
    pub evidence: Vec<String>,
}

/// One kanban column: lane name + cards in id order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct KanbanColumnDto {
    pub name: String,
    pub cards: Vec<KanbanCardDto>,
}

/// Head-SHA-fenced feedback (f05-vk-kanban-diff): a review note recorded
/// against exactly one frozen head (`head_sha` = `freeze_hash`). A consumer
/// applies it only when the task's current head still equals `head_sha`;
/// otherwise it is shown fenced (stale review, never silently applied).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct FeedbackDto {
    pub id: String,
    pub task_id: String,
    pub head_sha: String,
    pub target_ref: String,
    pub tier: String,
    pub body: String,
    pub current_head: bool,
}

/// Audit + Gatekeeper payload: kanban board + fenced feedback.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AuditDto {
    pub source: String,
    #[ts(type = "number")]
    pub db_age_ms: u64,
    pub staleness: StalenessDto,
    #[ts(type = "number")]
    pub change_seq: i64,
    pub columns: Vec<KanbanColumnDto>,
    pub feedback: Vec<FeedbackDto>,
}

/// Where an agent detection manifest came from (f05-herdr-detect clean-room,
/// Apache-2.0; ghostty-vt explicitly rejected as a dependency — PATTERN ONLY).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub enum ManifestSource {
    Bundled,
    Remote,
    Override,
}

/// One agent detection record: manifest provenance + PATH-probe outcome +
/// explain string + working/blocked/idle flag derived from real signals
/// (binary presence × daemon reachability).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct McpAgentDto {
    pub name: String,
    pub manifest_source: ManifestSource,
    pub version: String,
    pub detected: bool,
    pub explain: String,
    pub state: String,
}

/// MCP Registry payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct McpRegistryDto {
    pub source: String,
    #[ts(type = "number")]
    pub db_age_ms: u64,
    pub staleness: StalenessDto,
    #[ts(type = "number")]
    pub change_seq: i64,
    pub agents: Vec<McpAgentDto>,
}

/// One cached health probe (f05-openfang-providers clean-room, MIT OR
/// Apache-2.0): the last real probe result + TTL. `fresh` is false when the
/// cached result is older than `ttl_ms` — the UI re-probes (scoped retry).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ProbeDto {
    pub name: String,
    pub fresh: bool,
    #[ts(type = "number")]
    pub last_probe_ms: u64,
    #[ts(type = "number")]
    pub ttl_ms: u64,
    pub detail: String,
}

/// One budget row over the real `token_ledger` (billed spend, cache hits
/// tracked separately — never netted).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct BudgetDto {
    pub session: String,
    #[ts(type = "number")]
    pub total: i64,
    #[ts(type = "number")]
    pub cache_hits: i64,
}

/// Per-harness ACP auth-status probe (f05-oh-acp-authprobe clean-room, MIT):
/// credential-file presence only (never reads secrets) with an `unknown`
/// fallback when the harness declares no checkable path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AuthProbeDto {
    pub harness: String,
    pub method: String,
    pub status: String,
    pub detail: String,
}

/// Providers payload: probe cache + budgets + auth probes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ProvidersDto {
    pub source: String,
    #[ts(type = "number")]
    pub db_age_ms: u64,
    pub staleness: StalenessDto,
    #[ts(type = "number")]
    pub change_seq: i64,
    pub probes: Vec<ProbeDto>,
    pub budgets: Vec<BudgetDto>,
    #[ts(type = "number")]
    pub lifetime_total: i64,
    pub auth: Vec<AuthProbeDto>,
}

/// One run slot for multi-run compare (f05-openchamber-multirun clean-room,
/// MIT): a real `worktrees` row + its receipt evidence count. Capped at 5.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RunSlotDto {
    pub slug: String,
    pub path: String,
    pub state: String,
    #[ts(type = "number")]
    pub receipts: usize,
}

/// Multi-run compare payload. Per-model columns + guided walkthrough are
/// DEFERRED (see `deferred`): tasks carry no run/model linkage in schema v2.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RunsDto {
    pub source: String,
    #[ts(type = "number")]
    pub db_age_ms: u64,
    pub staleness: StalenessDto,
    #[ts(type = "number")]
    pub change_seq: i64,
    pub runs: Vec<RunSlotDto>,
    pub deferred: String,
}

/// POST /api/ask/:id/decide body (A1 one-tap decision contract).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DecideBody {
    pub approved: bool,
    pub reason: String,
}

/// POST /api/ask/:id/decide outcome: the durable ledger row after CAS.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DecideOutcomeDto {
    pub id: String,
    pub status: String,
    #[ts(type = "number | null")]
    pub decided_ms: Option<u64>,
}

/// Render the committed TypeScript mirror. Order is fixed (field order =
/// declaration order) so the committed file diff is stable.
pub fn export_ts() -> String {
    use ts_rs::TS as _;
    let cfg = ts_rs::Config::default();
    let mut out = String::from(
        "// GENERATED by `cargo run -p studio-web --bin studio-web-gen-ts` — do not hand-edit.\n// Source of truth: studio-web/src/api.rs (ts-rs 12.x, NOT the xazukx fork).\n",
    );
    for decl in [
        StalenessState::decl(&cfg),
        StalenessDto::decl(&cfg),
        TaskDto::decl(&cfg),
        CountsDto::decl(&cfg),
        StatusDto::decl(&cfg),
        AskItemDto::decl(&cfg),
        AskFeedDto::decl(&cfg),
        EventDto::decl(&cfg),
        GapKind::decl(&cfg),
        HistoryDto::decl(&cfg),
        DaemonDto::decl(&cfg),
        HubDto::decl(&cfg),
        HealthDto::decl(&cfg),
        BucketDto::decl(&cfg),
        NotificationDto::decl(&cfg),
        WarRoomDto::decl(&cfg),
        AgentEventDto::decl(&cfg),
        HookProviderDto::decl(&cfg),
        AgentStreamDto::decl(&cfg),
        KanbanCardDto::decl(&cfg),
        KanbanColumnDto::decl(&cfg),
        FeedbackDto::decl(&cfg),
        AuditDto::decl(&cfg),
        ManifestSource::decl(&cfg),
        McpAgentDto::decl(&cfg),
        McpRegistryDto::decl(&cfg),
        ProbeDto::decl(&cfg),
        BudgetDto::decl(&cfg),
        AuthProbeDto::decl(&cfg),
        ProvidersDto::decl(&cfg),
        RunSlotDto::decl(&cfg),
        RunsDto::decl(&cfg),
        DecideBody::decl(&cfg),
        DecideOutcomeDto::decl(&cfg),
    ] {
        for line in decl.split('\n') {
            if line.is_empty() {
                continue;
            }
            if let Some(rest) = line.strip_prefix("type ") {
                out.push_str("export type ");
                out.push_str(rest);
            } else {
                out.push_str(line);
            }
            out.push('\n');
        }
        out.push('\n');
    }
    out
}
