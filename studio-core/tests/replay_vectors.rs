//! V1 Replay Flight Recorder (phase 02): 5 trace fixtures × 20 runs.
//!
//! T1 freeze→correction→ack · T2 2nd-correction typed-reject ·
//! T3 land-without-token denial · T4 spawn-escalation reject ·
//! T5 next-transition determinism. Every trace runs 20× and must produce
//! byte-identical outcomes (freeze hashes, typed errors, transitions).
//! Tokens embed wall-clock nonces and are excluded from the determinism
//! comparison by design (documented, not asserted).
//!
//! No-socket guard: the socket-fd count in `/proc/self/fd` must not grow
//! across the whole replay (Linux). Nothing here may bind, connect, spawn a
//! subprocess, or call `acp_http_adapter::run_server` — fakes only.

use std::sync::Mutex;
use studio_core::roles::{RolePack, SpawnPolicy};
use studio_core::state::StateStore;
use studio_core::verify::{
    burn_token, freeze_candidate, issue_token, next_transition, queue, run_gate_order, BurnedToken,
    DeliveryState, FreezeError, GateError, GateEvidence, GateTask, GitBackend, LandingError,
    NextStep,
};

const RUNS: usize = 20;

struct FakeGit {
    current: Mutex<String>,
}

impl FakeGit {
    fn new(sha: &str) -> Self {
        Self {
            current: Mutex::new(sha.into()),
        }
    }
}

impl GitBackend for FakeGit {
    fn read_ref(&self, _repo: &str, _target_ref: &str) -> Result<String, LandingError> {
        Ok(self.current.lock().unwrap().clone())
    }

    fn cas_update(
        &self,
        _repo: &str,
        _target_ref: &str,
        expected_old: &str,
        new_sha: &str,
    ) -> Result<(), LandingError> {
        let mut cur = self.current.lock().unwrap();
        if *cur != expected_old {
            return Err(LandingError::CasConflict {
                expected: expected_old.into(),
                live: cur.clone(),
            });
        }
        *cur = new_sha.into();
        Ok(())
    }
}

fn files() -> Vec<(String, String)> {
    vec![
        ("b.rs".into(), "fn b() {}".into()),
        ("a.rs".into(), "fn a() -> i32 { 1 }".into()),
    ]
}

fn task() -> GateTask {
    GateTask {
        task_id: "t1".into(),
        target_ref: "refs/heads/main".into(),
        expected_old: "oldsha".into(),
        source_sha: "newsha".into(),
        files: files(),
        live: files(),
        message: "implement feature".into(),
        declared_paths: vec!["a.rs".into(), "b.rs".into()],
        parent: None,
    }
}

fn green() -> GateEvidence {
    GateEvidence {
        tests_green: true,
        diagnostics_budget_ok: true,
        files_done: 2,
        files_planned: 2,
        claims_done: true,
    }
}

fn red() -> GateEvidence {
    GateEvidence {
        tests_green: false,
        ..green()
    }
}

/// T1: freeze → blocked-on-red (runtime records the attempt) → green lands.
/// Deterministic outputs: freeze hash + integrated sha, 20/20 identical.
/// No budget object: enforcement is runtime-side (QA-R1).
#[test]
fn t1_freeze_correction_ack_replays_identically() {
    let mut freezes = vec![];
    let mut landed = vec![];
    for _ in 0..RUNS {
        let store = StateStore::open_in_memory().unwrap();
        let git = FakeGit::new("oldsha");
        let err = run_gate_order(&store, &task(), &red(), &git, "repo").unwrap_err();
        assert!(matches!(err, GateError::DodBlocked { .. }));
        let out = run_gate_order(&store, &task(), &green(), &git, "repo").unwrap();
        freezes.push(out.freeze_hash);
        landed.push(out.integrated_sha);
    }
    assert!(freezes.windows(2).all(|w| w[0] == w[1]), "freeze drifted");
    assert!(landed.iter().all(|s| s == "newsha"));
}

/// T2: the runtime refuses a second blocked attempt for the same freeze
/// (no budget object anywhere), 20/20 identical.
#[test]
fn t2_second_correction_replays_typed_reject() {
    for _ in 0..RUNS {
        let store = StateStore::open_in_memory().unwrap();
        let git = FakeGit::new("oldsha");
        let err = run_gate_order(&store, &task(), &red(), &git, "repo").unwrap_err();
        assert!(matches!(err, GateError::DodBlocked { .. }));
        let err = run_gate_order(&store, &task(), &red(), &git, "repo").unwrap_err();
        assert!(matches!(err, GateError::CorrectionSpent { .. }));
    }
}

/// T3: landing without a burned token is denied typed, 20/20 identical —
/// including cross-task presentation of a burned token (QA-R4).
#[test]
fn t3_land_without_token_replays_denial() {
    for _ in 0..RUNS {
        let store = StateStore::open_in_memory().unwrap();
        let candidate = freeze_candidate("refs/heads/main", files(), "x", None);
        let token = issue_token(&store, &candidate.freeze_hash, "t1").unwrap();
        assert_eq!(
            BurnedToken::from_store(&store, &token, &candidate.freeze_hash, "t1").unwrap_err(),
            LandingError::AckNotBurned
        );
        // Burn-then-double-burn: second burn is AlreadySpent, still no land.
        burn_token(&store, &token, &candidate.freeze_hash).unwrap();
        assert_eq!(
            burn_token(&store, &token, &candidate.freeze_hash).unwrap_err(),
            FreezeError::AlreadySpent
        );
        // Burned for t1, presented for t2: TaskMismatch, typed.
        assert!(matches!(
            BurnedToken::from_store(&store, &token, &candidate.freeze_hash, "t2").unwrap_err(),
            LandingError::TaskMismatch { .. }
        ));
    }
}

/// T4: spawn escalation is a typed rejection naming the scope through the
/// BOUND pack entry point (QA-R2), 20/20 — plus empty-child denial.
#[test]
fn t4_spawn_escalation_replays_typed_reject() {
    let pack = RolePack::coder_web();
    for _ in 0..RUNS {
        let err = pack.request_spawn(&["write".to_string()]).unwrap_err();
        assert_eq!(err.scope, "write");
        assert_eq!(pack.request_spawn(&[]).unwrap_err().scope, "<empty-child>");
        assert!(SpawnPolicy::Isolated
            .request(&["read".to_string()], &pack.tools)
            .is_err());
    }
}

/// T5: delivery next-transitions are a pure function of record state —
/// Queued→Land, Conflict→Requeue, GitIntegrated→Terminal — 20/20 identical.
#[test]
fn t5_next_transition_is_deterministic() {
    for _ in 0..RUNS {
        let mut rec = queue("t1", "refs/heads/main", "oldsha", "newsha", "freeze-1");
        assert_eq!(next_transition(&rec), NextStep::Land);
        // Target moved under us: conflict transition (pure state flip).
        rec.state = DeliveryState::Conflict;
        assert_eq!(next_transition(&rec), NextStep::Requeue);
        rec.state = DeliveryState::GitIntegrated;
        assert_eq!(next_transition(&rec), NextStep::Terminal);
    }
}

/// No-socket guard: the full replay (all five traces × 20) must not open a
/// single socket. Counts `socket:` fds before/after on Linux.
#[test]
fn replay_opens_no_sockets() {
    #[cfg(target_os = "linux")]
    fn socket_fd_count() -> usize {
        std::fs::read_dir("/proc/self/fd")
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .filter(|e| {
                        std::fs::read_link(e.path())
                            .map(|p| p.to_string_lossy().starts_with("socket:"))
                            .unwrap_or(false)
                    })
                    .count()
            })
            .unwrap_or(0)
    }

    #[cfg(target_os = "linux")]
    {
        // NOTE: a tokio runtime opens its own driver sockets at
        // construction (3 on this kernel/tokio line) — setup that happens
        // BEFORE the baseline, so the guard measures only what the replay
        // itself opens (bar: zero).
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let before = socket_fd_count();
        t1_freeze_correction_ack_replays_identically();
        t2_second_correction_replays_typed_reject();
        t3_land_without_token_replays_denial();
        t4_spawn_escalation_replays_typed_reject();
        t5_next_transition_is_deterministic();
        // ACP fake bridge: gated posts without sockets.
        rt.block_on(async {
            let fake = studio_core::acp::FakeTransport::new(serde_json::json!({"ok": true}));
            let out = studio_core::acp::post_gated(
                &fake,
                "read",
                "allow-once",
                None,
                serde_json::json!({"m": 1}),
            )
            .await
            .unwrap();
            assert_eq!(out, serde_json::json!({"ok": true}));
        });
        let after = socket_fd_count();
        assert_eq!(
            before, after,
            "replay opened sockets (before={before}, after={after})"
        );
    }
    #[cfg(not(target_os = "linux"))]
    {
        // Guard is Linux-specific (`/proc/self/fd`); elsewhere the replay
        // still runs (structural property: fakes only, no `run_server`).
        t1_freeze_correction_ack_replays_identically();
        t2_second_correction_replays_typed_reject();
        t3_land_without_token_replays_denial();
        t4_spawn_escalation_replays_typed_reject();
        t5_next_transition_is_deterministic();
    }
}
