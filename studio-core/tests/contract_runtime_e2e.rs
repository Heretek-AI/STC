//! Phase 02 acceptance: gate order end-to-end on a fixture task.
//!
//! Covers all six GOAL.md criteria (the last is this file):
//! 1. gate order executable end-to-end on a fixture task
//! 2. freeze immutable + reproducible
//! 3. second correction rejected typed — RUNTIME-enforced (QA-R1: no
//!    `CorrectionBudget` object appears anywhere in this file; the bound
//!    lives in the DB per freeze hash)
//! 4. land denied without burned token + single-delivery task binding (QA-R4)
//! 5. spawn escalation rejected typed — via the bound pack entry point (QA-R2)
//! 6. tests cover all four (this file + `replay_vectors` + `mutation_calibration`)

use std::sync::Mutex;
use studio_core::roles::{RolePack, SpawnPolicy};
use studio_core::state::StateStore;
use studio_core::verify::{
    burn_token, freeze_candidate, issue_token, run_gate_order, BurnedToken, FreezeError, GateError,
    GateEvidence, GateTask, GitBackend, LandingError,
};

struct FakeGit {
    current: Mutex<String>,
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
    vec![("a.rs".into(), "fn a() -> i32 { 1 }".into())]
}

fn fixture_task() -> GateTask {
    GateTask {
        task_id: "t1".into(),
        target_ref: "refs/heads/main".into(),
        expected_old: "oldsha".into(),
        source_sha: "newsha".into(),
        files: files(),
        live: files(),
        message: "implement feature".into(),
        declared_paths: vec!["a.rs".into()],
        parent: None,
    }
}

fn green() -> GateEvidence {
    GateEvidence {
        tests_green: true,
        diagnostics_budget_ok: true,
        files_done: 1,
        files_planned: 1,
        claims_done: true,
    }
}

/// AC1: gate order runs end-to-end on a fixture task and lands.
#[test]
fn gate_order_lands_fixture_task_end_to_end() {
    let store = StateStore::open_in_memory().unwrap();
    let git = FakeGit {
        current: Mutex::new("oldsha".into()),
    };
    let out = run_gate_order(&store, &fixture_task(), &green(), &git, "repo").unwrap();
    assert_eq!(out.integrated_sha, "newsha");
    assert_eq!(git.current.lock().unwrap().as_str(), "newsha");
}

/// AC2: freeze is immutable + reproducible — identical inputs, identical
/// hash; any content change, new hash; live divergence is typed.
#[test]
fn freeze_is_immutable_and_reproducible() {
    let a = freeze_candidate("refs/heads/main", files(), "implement feature", None);
    let mut reversed = files();
    reversed.reverse();
    let b = freeze_candidate("refs/heads/main", reversed, "implement feature", None);
    assert_eq!(a.freeze_hash, b.freeze_hash);
    assert!(a.verify_tree(&files()).is_ok());

    let tampered = vec![("a.rs".into(), "fn a() -> i32 { 2 }".into())];
    let c = freeze_candidate(
        "refs/heads/main",
        tampered.clone(),
        "implement feature",
        None,
    );
    assert_ne!(a.freeze_hash, c.freeze_hash);
    let err = a.verify_tree(&tampered).unwrap_err();
    assert!(matches!(err, FreezeError::TreeDiverged { .. }));
}

/// AC3 (QA-R1): the RUNTIME bounds corrections per freeze hash in the DB.
/// No budget object exists in this test — deleting caller plumbing cannot
/// weaken the bound, because the bound never lived in the caller. Same
/// frozen content blocked twice → `CorrectionSpent`; a green run still
/// lands (the budget bounds corrections, not delivery); a second green run
/// hits single-delivery (`DuplicateReceipt`).
#[test]
fn second_correction_is_rejected_by_the_runtime_typed() {
    let store = StateStore::open_in_memory().unwrap();
    let git = FakeGit {
        current: Mutex::new("oldsha".into()),
    };

    // Attempt 1: red tests block the run by name; runtime records the attempt.
    let red = GateEvidence {
        tests_green: false,
        ..green()
    };
    let err = run_gate_order(&store, &fixture_task(), &red, &git, "repo").unwrap_err();
    assert!(matches!(err, GateError::DodBlocked { .. }));
    assert!(err.to_string().contains("tests not green"));

    // Attempt 2: same frozen content, fresh everything, NO budget object —
    // still refused, typed, by the runtime.
    let err = run_gate_order(&store, &fixture_task(), &red, &git, "repo").unwrap_err();
    assert!(matches!(err, GateError::CorrectionSpent { .. }));
    assert!(err.to_string().contains("re-freeze"));

    // Attempt 3 (corrected evidence, same files): lands.
    let out = run_gate_order(&store, &fixture_task(), &green(), &git, "repo").unwrap();
    assert_eq!(out.integrated_sha, "newsha");

    // Attempt 4: same task + freeze cannot deliver twice. The retry tracks
    // the moved head (expected_old=newsha) so CAS passes and the delivery
    // receipt itself refuses the duplicate, typed.
    let mut retry = fixture_task();
    retry.expected_old = "newsha".into();
    retry.source_sha = "newsha-fixed".into();
    let err = run_gate_order(&store, &retry, &green(), &git, "repo").unwrap_err();
    assert!(
        matches!(err, GateError::Landing(LandingError::DuplicateReceipt)),
        "unexpected: {err:?}"
    );
}

/// AC4 (QA-R4): landing without a burned token is denied typed — at proof
/// construction (unburned token never becomes a `BurnedToken`), at the gate
/// (proof bound to another candidate never lands this one), and across tasks
/// (a t1-bound proof never lands t2: single-delivery).
#[test]
fn land_without_burned_token_is_denied() {
    let store = StateStore::open_in_memory().unwrap();
    let git = FakeGit {
        current: Mutex::new("oldsha".into()),
    };
    let candidate = freeze_candidate("refs/heads/main", files(), "x", None);

    // Issued but never burned: no proof constructible.
    let token = issue_token(&store, &candidate.freeze_hash, "t1").unwrap();
    assert_eq!(
        BurnedToken::from_store(&store, &token, &candidate.freeze_hash, "t1").unwrap_err(),
        LandingError::AckNotBurned
    );

    // Burned for ANOTHER candidate: proof exists but the gate denies it.
    let other = freeze_candidate(
        "refs/heads/main",
        vec![("b.rs".into(), "2".into())],
        "x",
        None,
    );
    let token2 = issue_token(&store, &other.freeze_hash, "t1").unwrap();
    burn_token(&store, &token2, &other.freeze_hash).unwrap();
    let foreign = BurnedToken::from_store(&store, &token2, &other.freeze_hash, "t1").unwrap();
    let mut record = studio_core::verify::queue(
        "t1",
        "refs/heads/main",
        "oldsha",
        "newsha",
        &candidate.freeze_hash,
    );
    let err = studio_core::verify::land(&mut record, &foreign, &git, "repo", &store).unwrap_err();
    assert!(matches!(err, LandingError::NoBurnedAck { .. }));
    assert_eq!(record.state, studio_core::verify::DeliveryState::Queued);
}

/// AC4b (QA-R4): t1 and t2 share one freeze; each lands with its OWN token,
/// and the t1 proof presented for t2 is denied typed — one ack, one delivery.
#[test]
fn one_ack_lands_exactly_one_delivery_same_freeze() {
    let store = StateStore::open_in_memory().unwrap();
    let candidate = freeze_candidate("refs/heads/main", files(), "x", None);
    let git1 = FakeGit {
        current: Mutex::new("oldsha".into()),
    };
    let git2 = FakeGit {
        current: Mutex::new("oldsha".into()),
    };
    // Same frozen content, two tasks, two tokens: both land legitimately.
    for (task, git, target) in [
        ("t1", &git1, "refs/heads/t1"),
        ("t2", &git2, "refs/heads/t2"),
    ] {
        let tok = issue_token(&store, &candidate.freeze_hash, task).unwrap();
        burn_token(&store, &tok, &candidate.freeze_hash).unwrap();
        let proof = BurnedToken::from_store(&store, &tok, &candidate.freeze_hash, task).unwrap();
        let mut rec =
            studio_core::verify::queue(task, target, "oldsha", "newsha", &candidate.freeze_hash);
        assert!(studio_core::verify::land(&mut rec, &proof, git, "repo", &store).is_ok());
    }
    // Cross-present: the t1 proof for a t2 record is TaskMismatch, typed.
    let tok = issue_token(&store, &candidate.freeze_hash, "t1").unwrap();
    burn_token(&store, &tok, &candidate.freeze_hash).unwrap();
    let proof_t1 = BurnedToken::from_store(&store, &tok, &candidate.freeze_hash, "t1").unwrap();
    assert!(matches!(
        BurnedToken::from_store(&store, &tok, &candidate.freeze_hash, "t2").unwrap_err(),
        LandingError::TaskMismatch { .. }
    ));
    let mut rec = studio_core::verify::queue(
        "t2",
        "refs/heads/t2",
        "oldsha",
        "evil-sha",
        &candidate.freeze_hash,
    );
    let err = studio_core::verify::land(&mut rec, &proof_t1, &git2, "repo", &store).unwrap_err();
    assert!(matches!(err, LandingError::TaskMismatch { .. }));
    assert_eq!(rec.state, studio_core::verify::DeliveryState::Queued);
}

/// AC5 (QA-R2): spawn escalation is a typed rejection through the BOUND pack
/// entry point — the parent basis is the pack's declared tools, never a
/// caller claim. A forged basis makes the raw primitive allow, proving the
/// binding is load-bearing.
#[test]
fn spawn_escalation_is_rejected_typed() {
    let pack = RolePack::coder_web();
    // Declared inheritable scope: granted through the bound entry point.
    assert!(pack.request_spawn(&["read".to_string()]).is_ok());
    // Write-class scope above the inheritable set: typed denial naming it.
    let err = pack.request_spawn(&["write".to_string()]).unwrap_err();
    assert_eq!(err.scope, "write");
    // Forged-basis contrast on a dispatcher shape (sast inheritable on
    // paper, not held): the primitive trusts the forgery and allows…
    let dispatcher = RolePack {
        name: "dispatcher-shape".into(),
        version: "1".into(),
        tools: vec!["read".into(), "plan_open".into()],
        skill_lockfile_hash: RolePack::lockfile_hash("x"),
        system_prompt: String::new(),
        model_slot: "manager".into(),
        harness_profile: Default::default(),
        spawns: SpawnPolicy::Supervised {
            inheritable_scopes: vec!["read".into(), "plan_open".into(), "sast".into()],
        },
        output_schema: None,
    };
    let forged = vec!["read".into(), "plan_open".into(), "sast".into()];
    assert!(dispatcher
        .spawns
        .request(&["sast".to_string()], &forged)
        .is_ok());
    // …but the bound pack entry point denies.
    assert_eq!(
        dispatcher
            .request_spawn(&["sast".to_string()])
            .unwrap_err()
            .scope,
        "sast"
    );
    // Isolation: even held scopes never propagate.
    assert!(SpawnPolicy::Isolated
        .request(&["read".to_string()], &pack.tools)
        .is_err());
    // Empty-child spawns fail closed, never vacuously allow.
    assert_eq!(pack.request_spawn(&[]).unwrap_err().scope, "<empty-child>");
}
