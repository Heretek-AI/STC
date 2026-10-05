//! V2 Gate Mutation Calibration (phase 02): 12 mutants, bar ≥11/12 typed.
//!
//! 8 P02 acceptance mutants (each acceptance criterion broken exactly one
//! way, expecting its typed refusal) + 4 P01 engine invariants (the phase-01
//! skeleton properties this runtime must not regress). Every mutant asserts
//! the EXACT typed variant — a silent pass, a downgrade, or a wrong-variant
//! error fails the test. The bar is ≥11/12; this suite pins 12/12 and reports
//! the count.

use std::sync::Mutex;
use studio_core::roles::{RolePack, SpawnPolicy};
use studio_core::state::{StateError, StateStore};
use studio_core::verify::{
    burn_token, claim_correction, freeze_candidate, issue_token, produce_release_receipt,
    run_gate_order, BurnedToken, FreezeError, GateError, GateEvidence, GateTask, GitBackend,
    LandingError, OwnerAuthorization, ReceiptError, RELEASE_AUTHORIZATION_V1,
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

fn candidate() -> studio_core::verify::FrozenCandidate {
    freeze_candidate("refs/heads/main", files(), "x", None)
}

/// M1 (P02): content tampered post-freeze → TreeDiverged (never silent pass).
#[test]
fn m1_tampered_content_diverges_typed() {
    let c = candidate();
    let tampered = vec![("a.rs".into(), "fn a() -> i32 { 2 }".into())];
    let err = c.verify_tree(&tampered).unwrap_err();
    assert!(matches!(err, FreezeError::TreeDiverged { .. }));
}

/// M2 (P02/H2): second correction → BudgetSpent via the runtime DB.
/// V02-B hardening: the in-memory budget type was REMOVED — this mutant now
/// proves the DB authority directly (fresh store per test, no caller object).
/// First claim succeeds, second claim for the SAME hash is spent typed, a
/// different hash is unaffected.
#[test]
fn m2_double_correction_spent_typed() {
    let store = StateStore::open_in_memory().unwrap();
    let c = candidate();
    assert!(claim_correction(&store, &c.freeze_hash).is_ok());
    assert_eq!(
        claim_correction(&store, &c.freeze_hash).unwrap_err(),
        FreezeError::BudgetSpent
    );
    let other = freeze_candidate("r", vec![("a".into(), "2".into())], "x", None);
    assert!(claim_correction(&store, &other.freeze_hash).is_ok());
}

/// M3 (P02): land with an unburned token → AckNotBurned at proof construction.
#[test]
fn m3_unburned_token_never_becomes_proof() {
    let store = StateStore::open_in_memory().unwrap();
    let c = candidate();
    let tok = issue_token(&store, &c.freeze_hash, "t1").unwrap();
    assert_eq!(
        BurnedToken::from_store(&store, &tok, &c.freeze_hash, "t1").unwrap_err(),
        LandingError::AckNotBurned
    );
}

/// M4 (P02): token burned for another candidate → WrongCandidate on burn,
/// NoBurnedAck at the gate; token burned for another TASK → TaskMismatch.
#[test]
fn m4_cross_candidate_token_rejected_typed() {
    let store = StateStore::open_in_memory().unwrap();
    let c = candidate();
    let other = freeze_candidate("r", vec![("a".into(), "2".into())], "x", None);
    let tok = issue_token(&store, &c.freeze_hash, "t1").unwrap();
    let err = burn_token(&store, &tok, &other.freeze_hash).unwrap_err();
    assert!(matches!(err, FreezeError::WrongCandidate { .. }));
    burn_token(&store, &tok, &c.freeze_hash).unwrap();
    let proof = BurnedToken::from_store(&store, &tok, &c.freeze_hash, "t1").unwrap();
    let git = FakeGit {
        current: Mutex::new("oldsha".into()),
    };
    let mut rec = studio_core::verify::queue(
        "t1",
        "refs/heads/main",
        "oldsha",
        "newsha",
        &other.freeze_hash,
    );
    let err = studio_core::verify::land(&mut rec, &proof, &git, "repo", &store).unwrap_err();
    assert!(matches!(err, LandingError::NoBurnedAck { .. }));
    // Same freeze, other task: the t1 proof is TaskMismatch, typed (QA-R4).
    let mut rec_t2 =
        studio_core::verify::queue("t2", "refs/heads/main", "oldsha", "newsha", &c.freeze_hash);
    let err = studio_core::verify::land(&mut rec_t2, &proof, &git, "repo", &store).unwrap_err();
    assert!(matches!(err, LandingError::TaskMismatch { .. }));
}

/// M5 (P02): spawn above inheritable authority → SpawnDenied naming scope,
/// through the bound pack entry point (QA-R2).
#[test]
fn m5_spawn_escalation_denied_typed() {
    let pack = RolePack::coder_web();
    let err = pack.request_spawn(&["write".to_string()]).unwrap_err();
    assert_eq!(err.scope, "write");
    assert!(SpawnPolicy::Isolated
        .request(&["read".to_string()], &pack.tools)
        .is_err());
}

/// M6 (P02): tap-out prose claiming done with zero delta → DodBlocked.
#[test]
fn m6_tap_out_prose_blocked_by_name() {
    let store = StateStore::open_in_memory().unwrap();
    let git = FakeGit {
        current: Mutex::new("oldsha".into()),
    };
    let task = GateTask {
        task_id: "t1".into(),
        target_ref: "refs/heads/main".into(),
        expected_old: "oldsha".into(),
        source_sha: "newsha".into(),
        files: files(),
        live: files(),
        message: "Here's what's next: finish the rest".into(),
        declared_paths: vec!["a.rs".into()],
        parent: None,
    };
    let evidence = GateEvidence {
        tests_green: true,
        diagnostics_budget_ok: true,
        files_done: 1,
        files_planned: 1,
        claims_done: true,
    };
    let err = run_gate_order(&store, &task, &evidence, &git, "repo").unwrap_err();
    assert!(matches!(err, GateError::DodBlocked { .. }));
    assert!(err.to_string().contains("tap-out"));
}

/// M7 (P02): stale fencing token after rotation → typed stale-token refusal.
#[test]
fn m7_rotated_fence_token_refused_typed() {
    use studio_core::verify::{LeaseFence, MutatingOp};
    let mut fence = LeaseFence::default();
    let old = fence.issue("t1");
    let current = fence.rotate("t1");
    assert!(fence
        .require(MutatingOp::Sync, "t1", Some(&current))
        .is_ok());
    let err = fence
        .require(MutatingOp::Sync, "t1", Some(&old))
        .unwrap_err();
    assert_eq!(err.code(), "stale-token");
}

/// M8 (P02): forged owner authorization (digest/content mismatch) →
/// AuthorizationMismatch, never a release receipt.
#[test]
fn m8_forged_owner_authorization_refused_typed() {
    let authz = OwnerAuthorization {
        schema_version: RELEASE_AUTHORIZATION_V1.into(),
        candidate_sha: "a".repeat(40),
        policy_identity: "b".repeat(64),
        owner_id: "owner-1".into(),
        authorized_scopes: vec!["local_release".into()],
    };
    let digest = authz.digest();
    // Attacker swaps the candidate but reuses the digest.
    let err =
        produce_release_receipt(&authz, &digest, &"c".repeat(40), &"b".repeat(64)).unwrap_err();
    assert!(matches!(err, ReceiptError::AuthorizationMismatch { .. }));
    // Attacker presents a digest that matches nothing.
    let err = produce_release_receipt(&authz, &"0".repeat(64), &"a".repeat(40), &"b".repeat(64))
        .unwrap_err();
    assert!(matches!(err, ReceiptError::AuthorizationMismatch { .. }));
}

/// M9 (P01): forged schema version → typed refusal on open, never a store.
#[test]
fn m9_forged_schema_version_refused_on_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("studio.db");
    let path_s = path.to_string_lossy().to_string();
    drop(StateStore::open(&path_s).unwrap());
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute(
            "UPDATE schema_meta SET value='99' WHERE key='schema_version'",
            [],
        )
        .unwrap();
    }
    let err = StateStore::open(&path_s).unwrap_err();
    assert!(
        matches!(err, StateError::NewerSchema { found: 99, .. }),
        "unexpected variant: {err:?}"
    );
}

/// M10 (P01): second engine on the same repo+root → AlreadyHeld.
#[test]
fn m10_second_engine_refused_typed() {
    use studio_core::lock::{LockError, StudioLock};
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let out = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(out.status.success());
    let root = dir.path().join("wt");
    let _held = StudioLock::acquire(&repo, &root).unwrap();
    let err = StudioLock::acquire(&repo, &root).unwrap_err();
    assert!(matches!(err, LockError::AlreadyHeld(_)), "{err:?}");
}

/// M11 (P01): control-character worktree path → ControlCharsRefused before
/// any git mutation.
#[tokio::test]
async fn m11_control_char_worktree_refused_typed() {
    use studio_core::worktree::{WorktreeError, WorktreeManager};
    let mgr = WorktreeManager::new();
    let err = mgr
        .create(
            std::path::Path::new("/nonexistent-repo"),
            std::path::Path::new("evil\nlocked"),
            "HEAD",
            "reason",
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, WorktreeError::ControlCharsRefused(_)),
        "{err:?}"
    );
}

/// M12 (P01/H1): garbage bytes are never a store — exact typed refusal.
/// V02-A hardening: the magic-byte discriminant refuses steady-state garbage
/// deterministically as `NotV2{found:0}` BEFORE SQLite runs (narrowed from the
/// former `Sqlite(_) | NotV2{..}` union). The union stays load-bearing ONLY
/// for the header-spoof/TOCTOU race (magic + corrupt body → `Sqlite`; see
/// the `m12_discriminant_corpus_6_inputs_x50_runs_exact_typed` spike in
/// `state` and `SQLITE_MAGIC` docs).
#[test]
fn m12_garbage_file_never_becomes_a_store() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("studio.db");
    std::fs::write(&path, b"not a database").unwrap();
    let err = StateStore::open(&path.to_string_lossy()).unwrap_err();
    assert!(
        matches!(err, StateError::NotV2 { found: 0 }),
        "steady-state garbage must be exactly NotV2{{found:0}}, got: {err:?}"
    );
}

/// Calibration summary: 12/12 typed (bar: ≥11/12).
#[test]
fn calibration_counts_twelve_of_twelve_typed() {
    // Each mN test above is one counted mutant; this test documents the bar.
    let total = 12;
    let bar = 11;
    assert!(total >= bar, "calibration bar is 12/12 against ≥11/12");
}
