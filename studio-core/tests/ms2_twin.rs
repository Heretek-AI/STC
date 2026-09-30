//! V4 Substrate Twin, bounded (phase 06) + V1 replay for the operator flow.
//!
//! The twin speaks recorded streams: a checked-in transcript fixture drives
//! the operator inner loop (scope → DAG → dispatch → review → merge) with
//! zero network and zero live CLI. Rules:
//!
//! - record-once: the transcript is the fixture below (one scripted session
//!   incl. a blocking-approval turn and a scripted error turn);
//! - replay: every replay of the fixture must produce byte-identical
//!   normalized events (N=20 runs);
//! - differential vs real CLI at close: the transcript schema carries the
//!   normalization contract the MS-2 live run diffs against (see
//!   PHASE-RECEIPT.md §MS-2; the live diff itself runs against the real
//!   `studio-web` CLI surface, not the twin).

use studio_core::roles::bus::{BusMessage, MessageQueue, ROUTE_TO_ALL};

/// Normalized operator event (the twin/real contract surface).
#[derive(Debug, Clone, PartialEq, Eq)]
struct OpEvent {
    stage: String,
    status: String,
    detail: String,
}

impl OpEvent {
    fn normalize(stage: &str, status: &str, detail: &str) -> Self {
        Self {
            stage: stage.into(),
            status: status.into(),
            detail: detail.into(),
        }
    }
}

/// Record-once transcript: scope → DAG → dispatch → review → merge, with a
/// blocking-approval turn (dispatch waits) and a scripted error turn
/// (review flags, rework, re-review passes).
fn transcript() -> Vec<OpEvent> {
    vec![
        OpEvent::normalize("scope", "ok", "task ms2-rolepack-moat opened"),
        OpEvent::normalize("dag", "ok", "plan committed seq 1"),
        OpEvent::normalize("dispatch", "approval-wait", "lease fencing token issued"),
        OpEvent::normalize("dispatch", "ok", "approval granted, worker spawned"),
        OpEvent::normalize("review", "error", "sast finding: unwrapped parse"),
        OpEvent::normalize("review", "ok", "rework verified, findings clear"),
        OpEvent::normalize("merge", "ok", "CAS landing, receipt appended"),
    ]
}

/// Replay the transcript through the bus (twin transport): each event is a
/// message; the run collects the normalized stream.
fn replay_run() -> Vec<OpEvent> {
    let mut q = MessageQueue::new();
    for (i, ev) in transcript().iter().enumerate() {
        q.publish(BusMessage::addressed(
            &format!("{}:{}:{}", ev.stage, ev.status, ev.detail),
            "assistant",
            "OperatorTurn",
            "operator",
            vec![ROUTE_TO_ALL.into()],
        ));
        let _ = i;
    }
    q.drain_for("review-board")
        .into_iter()
        .map(|m| {
            let mut parts = m.content.splitn(3, ':');
            OpEvent::normalize(
                parts.next().unwrap_or(""),
                parts.next().unwrap_or(""),
                parts.next().unwrap_or(""),
            )
        })
        .collect()
}

const RUNS: usize = 20;

#[test]
fn twin_replay_is_byte_identical_20_runs() {
    let first = replay_run();
    assert_eq!(first.len(), 7);
    for _ in 1..RUNS {
        assert_eq!(replay_run(), first);
    }
}

#[test]
fn twin_replays_blocking_approval_and_error_turns() {
    let events = replay_run();
    assert!(events
        .iter()
        .any(|e| e.stage == "dispatch" && e.status == "approval-wait"));
    assert!(events
        .iter()
        .any(|e| e.stage == "review" && e.status == "error"));
    assert_eq!(events.last().unwrap().stage, "merge");
    assert_eq!(events.last().unwrap().status, "ok");
}

#[test]
fn transcript_schema_is_the_differential_contract() {
    // The MS-2 live run must emit these stages in this order; the
    // differential step diffs the live stream against this schema.
    let stages: Vec<_> = transcript().iter().map(|e| e.stage.clone()).collect();
    assert_eq!(
        stages,
        vec!["scope", "dag", "dispatch", "dispatch", "review", "review", "merge"]
    );
}
