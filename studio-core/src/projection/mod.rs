//! Read-only DB projection for cockpit + TUI (Phase 4).
//! Invariant: UI never holds canonical state. Every view is derived from
//! `studio.db` at poll time and stamps `source: studio.db · updated Nms ago`.
//! Poll interval: 100ms CDC (`change_log`, batch 512); see `state::poll_changes`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRow {
    pub id: String,
    pub kind: String,
    pub status: String,
    pub lane: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventRow {
    pub seq: i64,
    pub kind: String,
    pub payload: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiptRow {
    pub id: String,
    pub task_id: String,
    pub kind: String,
    pub evidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BurnRow {
    pub session: String,
    pub total: i64,
}

/// Fleet-board lane mapping (War Room lanes).
pub fn lane_for(status: &str) -> &'static str {
    match status {
        "building" | "pending" | "ready" | "running" => "building",
        "validating" | "dispatched" => "validating",
        "in-review" | "review" => "in-review",
        "ready-to-merge" | "done" | "merged" => "ready",
        _ => "building",
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    /// ms since snapshot was taken (staleness badge source).
    pub db_age_ms: u64,
    pub source: String,
    pub fleet: Vec<TaskRow>,
    pub stream: Vec<EventRow>,
    pub receipts: Vec<ReceiptRow>,
    pub burn: Vec<BurnRow>,
    pub change_seq: i64,
    /// Lane runtime mode bound to `STUDIO_LANE_RUNTIME` (`rootless` default,
    /// `privileged-dev` only via explicit override). Cockpit renders a
    /// persistent dev-mode badge from this; UI never owns it.
    pub runtime_mode: String,
}

/// Resolve the lane runtime mode from the environment (fail closed to
/// `rootless` on unknown values; never panics on missing env).
pub fn runtime_mode_from_env() -> String {
    match std::env::var("STUDIO_LANE_RUNTIME")
        .unwrap_or_else(|_| "rootless".into())
        .trim()
        .to_lowercase()
        .as_str()
    {
        "privileged-dev" | "privileged" | "dev" => "privileged-dev".into(),
        _ => "rootless".into(),
    }
}

impl Snapshot {
    pub fn stale_badge(&self) -> String {
        format!("source: studio.db · updated {}ms ago", self.db_age_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lanes_map() {
        assert_eq!(lane_for("building"), "building");
        assert_eq!(lane_for("pending"), "building");
        assert_eq!(lane_for("validating"), "validating");
        assert_eq!(lane_for("in-review"), "in-review");
        assert_eq!(lane_for("done"), "ready");
    }

    #[test]
    fn badge_names_db() {
        let s = Snapshot {
            db_age_ms: 12,
            source: "studio.db".into(),
            fleet: vec![],
            stream: vec![],
            receipts: vec![],
            burn: vec![],
            change_seq: 0,
            runtime_mode: "rootless".into(),
        };
        assert!(s.stale_badge().contains("studio.db"));
    }

    #[test]
    fn runtime_mode_resolves_fail_closed() {
        let prev = std::env::var("STUDIO_LANE_RUNTIME").ok();
        unsafe {
            std::env::set_var("STUDIO_LANE_RUNTIME", "bogus-mode");
        }
        assert_eq!(super::runtime_mode_from_env(), "rootless");
        unsafe {
            std::env::set_var("STUDIO_LANE_RUNTIME", "privileged-dev");
        }
        assert_eq!(super::runtime_mode_from_env(), "privileged-dev");
        unsafe {
            if let Some(v) = prev {
                std::env::set_var("STUDIO_LANE_RUNTIME", v);
            } else {
                std::env::remove_var("STUDIO_LANE_RUNTIME");
            }
        }
    }

    #[test]
    fn snapshot_reflects_db() {
        let db = crate::state::StateStore::open_in_memory().unwrap();
        db.upsert_task("t1", "coder", "building").unwrap();
        db.upsert_task("t2", "reviewer", "in-review").unwrap();
        db.append_event("dispatch", "t1").unwrap();
        db.append_receipt("r1", "t1", "done", "{\"delta\":[\"a.rs\"]}")
            .unwrap();
        db.record_tokens("coder-1", 100, false).unwrap();
        let snap = db.snapshot().unwrap();
        assert_eq!(snap.fleet.len(), 2);
        assert_eq!(snap.stream.len(), 1);
        assert_eq!(snap.receipts.len(), 1);
        assert_eq!(snap.burn.len(), 1);
        // evidence-first: receipt carries delta evidence
        assert!(snap.receipts[0].evidence.contains("delta"));
        // UI never owns state: re-poll reflects a status change
        db.upsert_task("t1", "coder", "done").unwrap();
        let snap2 = db.snapshot().unwrap();
        assert!(snap2
            .fleet
            .iter()
            .any(|t| t.id == "t1" && t.lane == "ready"));
    }
}
