//! Contract & verification runtime (phase 02): the moat mechanism.
//!
//! Port of v1 `studio-core/src/verify/*`
//! (`c1e49e97f6429dec4604dbcf76e3e4a27fa577284c0e0a82254dddce1ac8c29f`,
//! FILE_HASH_ONLY) behind the Q9 bar. What v1 got right and is kept:
//! frozen-candidate risk tiers with bounded lens depth, DoD receipts with
//! tap-out refusal, exactly-one correction budget, burn-once ack ledger.
//! What was rewritten where the contract was wrong: v1 froze `(head_sha,
//! file paths, tier)` only, so the review gates ran against the LIVE tree
//! while the receipt named a FROZEN candidate — a file edited mid-review
//! silently changed what was "reviewed". Every target's CONTENT sha256 is
//! now bound at freeze time, and [`FrozenCandidate::verify_tree`] must pass
//! against the live tree before any gate evaluates it (typed
//! [`FreezeError::TreeDiverged`], never a silent pass). All `String` errors
//! are now typed. Candidates + ack tokens persist in the v2 DB (phase-01
//! additive tables; schema_version stays 2).
//!
//! Gate order (repo DoD) and where each gate lives:
//! `format → fallow audit → Semgrep → tests + tree-sitter → RDD freeze → one
//! bounded correction → burned ack → merge`. The head (format / fallow /
//! Semgrep / test evidence) is repo CI (`prek`) plus caller-supplied
//! [`GateEvidence`]; this runtime executes the tail end-to-end via
//! [`run_gate_order`]: freeze → prove live==frozen → DoD evaluate (+ real
//! tree-sitter syntax over delivered Rust) → burned ack → CAS land. The
//! exactly-one correction is runtime-enforced per freeze hash in the DB
//! ([`claim_correction`], `correction_attempts` table): no caller-held budget
//! object exists — retries with fresh state and the same frozen content are
//! refused with [`GateError::CorrectionSpent`]; only a re-freeze (new content
//! hash) opens a new attempt.

pub mod dod;
pub mod freeze;
pub mod landing;
pub mod lease;
pub mod receipts;

pub use dod::{evaluate_done, syntax_check, DodCheck, DodReceipt, TapOut};
pub use freeze::{
    burn_token, claim_correction, freeze_candidate, issue_token, load_candidate, save_candidate,
    validation_depth, AckLedger, FreezeError, FrozenCandidate, RiskTier, CORRECTION_MAX_ATTEMPTS,
    GENESIS_LINEAGE,
};
pub use landing::{
    land, next_transition, queue, BurnedToken, DeliveryRecord, DeliveryState, GitBackend,
    LandingError, NextStep,
};
pub use lease::{FenceError, LeaseFence, MutatingOp};
pub use receipts::{
    build_envelope, check_envelope_schema, produce_release_receipt, ExecutionEnvelope,
    ExecutionStatus, OwnerAuthorization, ProviderObservation, ReceiptError, ReleaseReceipt,
    EXECUTION_ENVELOPE_V1, RELEASE_AUTHORIZATION_V1, RELEASE_RECEIPT_V1,
};

use thiserror::Error;

use crate::state::StateStore;

/// Caller-supplied CI evidence for one gate run. The runtime never measures
/// these itself (command-first: `cargo test`, `prek`); it gates on them.
pub struct GateEvidence {
    pub tests_green: bool,
    pub diagnostics_budget_ok: bool,
    pub files_done: usize,
    pub files_planned: usize,
    pub claims_done: bool,
}

/// One fixture task through the gate order.
pub struct GateTask {
    pub task_id: String,
    pub target_ref: String,
    pub expected_old: String,
    pub source_sha: String,
    /// Frozen content: `(path, content)` — content-bound at freeze time.
    pub files: Vec<(String, String)>,
    /// Live tree at gate time. The honest path equals `files`; any divergence
    /// is a typed refusal (Q9: gates never evaluate a stale candidate).
    pub live: Vec<(String, String)>,
    pub message: String,
    pub declared_paths: Vec<String>,
    pub parent: Option<FrozenCandidate>,
}

/// Gate-order success: what was frozen, at what tier, landed under which
/// burned token.
#[derive(Debug)]
pub struct GateOutcome {
    pub freeze_hash: String,
    pub tier: RiskTier,
    pub ack_token: String,
    pub integrated_sha: String,
}

/// Typed gate-order failures. Every variant names the gate that refused.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum GateError {
    #[error("freeze gate: {0}")]
    Freeze(#[from] FreezeError),
    #[error("tree gate: live tree diverged before evaluation: {0}")]
    TreeDiverged(FreezeError),
    #[error("dod gate blocked: {reasons}")]
    DodBlocked { reasons: String },
    #[error("correction budget spent for candidate {freeze_hash}: re-freeze the new candidate")]
    CorrectionSpent { freeze_hash: String },
    #[error("landing gate: {0}")]
    Landing(#[from] LandingError),
    #[error("state: {0}")]
    State(String),
}

fn lang_of(path: &str) -> &str {
    match path.rsplit('.').next().unwrap_or("") {
        "rs" => "rs",
        other => other,
    }
}

/// Execute the runtime tail of the gate order end-to-end on one task:
/// freeze (immutable + reproducible) → prove live==frozen → DoD evaluate
/// (tap-out / scope / evidence / real tree-sitter syntax) → issue + burn ack
/// → CAS land. Any refusal is typed and names its gate.
///
/// Correction bound (QA-R1): a blocked run consumes the single per-freeze
/// correction attempt in the DB ([`claim_correction`]). The bound needs NO
/// caller-held budget object — retries with fresh state and the same frozen
/// content are refused with [`GateError::CorrectionSpent`]; only a re-freeze
/// (new content hash) opens a new attempt.
pub fn run_gate_order(
    store: &StateStore,
    task: &GateTask,
    evidence: &GateEvidence,
    git: &dyn GitBackend,
    repo: &str,
) -> Result<GateOutcome, GateError> {
    // RDD freeze: content-bound, lineage-chained, persisted.
    let candidate = freeze_candidate(
        &task.target_ref,
        task.files.clone(),
        &task.message,
        task.parent.as_ref(),
    );
    save_candidate(store, &candidate)?;
    // Tree gate FIRST: prove the live tree still equals the frozen snapshot
    // before any evaluation touches it.
    candidate
        .verify_tree(&task.live)
        .map_err(GateError::TreeDiverged)?;
    // DoD gate: declared intent vs frozen delta + CI evidence + real syntax.
    let actual_paths: Vec<String> = candidate.targets.clone();
    let syntax_ok = task.files.iter().all(|(p, c)| syntax_check(c, lang_of(p)));
    let check = DodCheck {
        declared_paths: task.declared_paths.clone(),
        actual_paths,
        claims_done: evidence.claims_done,
        message: task.message.clone(),
        tests_green: evidence.tests_green,
        syntax_ok,
        diagnostics_budget_ok: evidence.diagnostics_budget_ok,
        files_done: evidence.files_done,
        files_planned: evidence.files_planned,
    };
    let receipt = evaluate_done(&check);
    if !receipt.done() {
        // QA-R1 (hardened V02-B/H2): the runtime owns the exactly-one
        // correction bound IN THE DB — a retry with fresh caller state (or
        // none at all) gets no more corrections for this candidate; only a
        // re-freeze (new hash) resets. No caller-held budget type exists.
        match claim_correction(store, &candidate.freeze_hash) {
            Ok(()) => {}
            Err(FreezeError::BudgetSpent) => {
                return Err(GateError::CorrectionSpent {
                    freeze_hash: candidate.freeze_hash,
                });
            }
            Err(e) => return Err(GateError::Freeze(e)),
        }
        return Err(GateError::DodBlocked {
            reasons: receipt.reasons.join("; "),
        });
    }
    // Burned ack: issue for this exact candidate AND task, burn exactly once.
    let token = issue_token(store, &candidate.freeze_hash, &task.task_id)?;
    burn_token(store, &token, &candidate.freeze_hash)?;
    let burned = BurnedToken::from_store(store, &token, &candidate.freeze_hash, &task.task_id)
        .map_err(GateError::Landing)?;
    // Merge: CAS land under the burned token; append-only delivery receipt.
    let mut record = queue(
        &task.task_id,
        &task.target_ref,
        &task.expected_old,
        &task.source_sha,
        &candidate.freeze_hash,
    );
    let integrated = land(&mut record, &burned, git, repo, store)?;
    Ok(GateOutcome {
        freeze_hash: candidate.freeze_hash,
        tier: candidate.tier,
        ack_token: token,
        integrated_sha: integrated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

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

    pub fn fixture_task() -> GateTask {
        let files = vec![("a.rs".into(), "fn a() -> i32 { 1 }".into())];
        GateTask {
            task_id: "t1".into(),
            target_ref: "refs/heads/main".into(),
            expected_old: "oldsha".into(),
            source_sha: "newsha".into(),
            files: files.clone(),
            live: files,
            message: "implement feature".into(),
            declared_paths: vec!["a.rs".into()],
            parent: None,
        }
    }

    pub fn green_evidence() -> GateEvidence {
        GateEvidence {
            tests_green: true,
            diagnostics_budget_ok: true,
            files_done: 1,
            files_planned: 1,
            claims_done: true,
        }
    }

    #[test]
    fn gate_order_lands_green_fixture_end_to_end() {
        let store = StateStore::open_in_memory().unwrap();
        let git = FakeGit {
            current: Mutex::new("oldsha".into()),
        };
        let out = run_gate_order(&store, &fixture_task(), &green_evidence(), &git, "repo").unwrap();
        assert_eq!(out.integrated_sha, "newsha");
        assert_eq!(out.tier, RiskTier::Low);
        // Freeze persisted; ack burned exactly once.
        assert!(load_candidate(&store, &out.freeze_hash).unwrap().is_some());
        assert_eq!(
            burn_token(&store, &out.ack_token, &out.freeze_hash).unwrap_err(),
            FreezeError::AlreadySpent
        );
    }

    #[test]
    fn gate_order_refuses_diverged_tree_before_evaluation() {
        let store = StateStore::open_in_memory().unwrap();
        let git = FakeGit {
            current: Mutex::new("oldsha".into()),
        };
        let mut task = fixture_task();
        task.live = vec![("a.rs".into(), "fn a() -> i32 { EDITED_MID_REVIEW }".into())];
        let err = run_gate_order(&store, &task, &green_evidence(), &git, "repo").unwrap_err();
        assert!(matches!(err, GateError::TreeDiverged(_)));
        assert!(err.to_string().contains("changed:a.rs"));
    }

    #[test]
    fn gate_order_blocks_red_tests_by_name() {
        let store = StateStore::open_in_memory().unwrap();
        let git = FakeGit {
            current: Mutex::new("oldsha".into()),
        };
        let evidence = GateEvidence {
            tests_green: false,
            ..green_evidence()
        };
        let err = run_gate_order(&store, &fixture_task(), &evidence, &git, "repo").unwrap_err();
        assert!(matches!(err, GateError::DodBlocked { .. }));
        assert!(err.to_string().contains("tests not green"));
    }

    #[test]
    fn runtime_bounds_corrections_without_caller_budget() {
        // QA-R1 (V02-B): no caller-held budget object anywhere in this test — the
        // runtime enforces the bound per freeze hash in the DB.
        let store = StateStore::open_in_memory().unwrap();
        let git = FakeGit {
            current: Mutex::new("oldsha".into()),
        };
        let red = GateEvidence {
            tests_green: false,
            ..green_evidence()
        };
        // First blocked run: DodBlocked, single attempt recorded.
        let err = run_gate_order(&store, &fixture_task(), &red, &git, "repo").unwrap_err();
        assert!(matches!(err, GateError::DodBlocked { .. }));
        // Second blocked run, same frozen content: spent, typed.
        let err = run_gate_order(&store, &fixture_task(), &red, &git, "repo").unwrap_err();
        assert!(matches!(err, GateError::CorrectionSpent { .. }));
        assert!(err.to_string().contains("re-freeze"));
        // A green run still lands: the budget bounds corrections, not delivery.
        let out = run_gate_order(&store, &fixture_task(), &green_evidence(), &git, "repo").unwrap();
        assert_eq!(out.integrated_sha, "newsha");
    }

    #[test]
    fn gate_order_blocks_broken_syntax_by_name() {
        let store = StateStore::open_in_memory().unwrap();
        let git = FakeGit {
            current: Mutex::new("oldsha".into()),
        };
        let mut task = fixture_task();
        task.files = vec![("a.rs".into(), "fn a( { let x = ;".into())];
        task.live = task.files.clone();
        let err = run_gate_order(&store, &task, &green_evidence(), &git, "repo").unwrap_err();
        assert!(matches!(err, GateError::DodBlocked { .. }));
        assert!(err.to_string().contains("syntax check failed"));
    }
}
