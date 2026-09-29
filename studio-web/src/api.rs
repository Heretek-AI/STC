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
    #[ts(type = "number")]
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
    #[ts(type = "number")]
    pub schema_version: Option<i64>,
    pub daemon: DaemonDto,
    pub hub: HubDto,
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
