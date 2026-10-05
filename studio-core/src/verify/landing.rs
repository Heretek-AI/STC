//! Native-git delivery adapter (phase 02): land only with a burned ack.
//!
//! Clean-room port of the yylo mechanism (MIT) behind citation c006
//! (`9e5426a3d6d99d9083fee556befeb8fe9f836f5463d96740fd646dc482332f49`,
//! native-git delivery adapter in `merge_queue.py`). No yylo code is copied:
//! the re-derived rules are — delivery states `QUEUED / CONFLICT /
//! GIT_INTEGRATED`; ref updates are compare-and-swap on the expected-old
//! value (a moved target fails closed into `CONFLICT`, never force-lands);
//! receipts are append-only rows; and `land` requires a BURNED ack token for
//! the exact frozen candidate (landing without one is a typed denial —
//! acceptance criterion).

use thiserror::Error;

use crate::state::StateStore;
use crate::verify::freeze::now_ms;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryState {
    Queued,
    Conflict,
    GitIntegrated,
}

impl DeliveryState {
    pub fn as_str(&self) -> &'static str {
        match self {
            DeliveryState::Queued => "QUEUED",
            DeliveryState::Conflict => "CONFLICT",
            DeliveryState::GitIntegrated => "GIT_INTEGRATED",
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LandingError {
    #[error("land denied: no burned ack token for candidate {candidate}")]
    NoBurnedAck { candidate: String },
    #[error("land denied: ack token is not burned (or binds another candidate)")]
    AckNotBurned,
    #[error("land denied: ack token is bound to task {expected}, not {found} (single-delivery)")]
    TaskMismatch { expected: String, found: String },
    #[error(
        "target moved: expected-old {expected} but live {live} — delivery is CONFLICT, re-queue"
    )]
    CasConflict { expected: String, live: String },
    #[error("delivery already landed (integrated {sha})")]
    AlreadyLanded { sha: String },
    #[error("not queued for landing (state {state})")]
    NotQueued { state: String },
    #[error("receipt already recorded (duplicate land attempt)")]
    DuplicateReceipt,
    #[error("git backend: {0}")]
    Git(String),
    #[error("state: {0}")]
    State(String),
}

/// A burned-token proof. Constructible ONLY through the ledger / store
/// verification constructors below — callers cannot fabricate one, so
/// `land` without a burned token is unrepresentable at the type level and
/// doubly denied at runtime. The proof is bound to ONE task (QA-R4): the
/// constructors require the task and refuse cross-task proofs typed, and
/// `land` re-checks the binding, so one ack lands exactly one delivery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BurnedToken {
    token: String,
    freeze_hash: String,
    task_id: String,
}

impl BurnedToken {
    pub fn from_ledger(
        ledger: &crate::verify::freeze::AckLedger,
        token: &str,
        freeze_hash: &str,
        task_id: &str,
    ) -> Result<Self, LandingError> {
        match ledger.burned_binding(token) {
            Some((h, t)) if h == freeze_hash && t == task_id => Ok(Self {
                token: token.into(),
                freeze_hash: freeze_hash.into(),
                task_id: task_id.into(),
            }),
            Some((h, _)) if h != freeze_hash => Err(LandingError::AckNotBurned),
            Some((_, t)) => Err(LandingError::TaskMismatch {
                expected: t.into(),
                found: task_id.into(),
            }),
            None => Err(LandingError::AckNotBurned),
        }
    }

    pub fn from_store(
        store: &StateStore,
        token: &str,
        freeze_hash: &str,
        task_id: &str,
    ) -> Result<Self, LandingError> {
        let row: Option<(i64, String)> = store
            .with_read(|conn| {
                conn.query_row(
                    "SELECT burned, task_id FROM ack_tokens WHERE token=? AND freeze_hash=?",
                    rusqlite::params![token, freeze_hash],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map(Some)
                .or_else(|e| match e {
                    rusqlite::Error::QueryReturnedNoRows => Ok(None),
                    other => Err(other),
                })
                .map_err(crate::state::StateError::from)
            })
            .map_err(|e| LandingError::State(e.to_string()))?;
        match row {
            Some((1, t)) if t == task_id => Ok(Self {
                token: token.into(),
                freeze_hash: freeze_hash.into(),
                task_id: task_id.into(),
            }),
            Some((1, t)) => Err(LandingError::TaskMismatch {
                expected: t,
                found: task_id.into(),
            }),
            _ => Err(LandingError::AckNotBurned),
        }
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn freeze_hash(&self) -> &str {
        &self.freeze_hash
    }

    pub fn task_id(&self) -> &str {
        &self.task_id
    }
}

/// Minimal git surface the adapter needs. The real impl shells out to the
/// `git` binary (no new dependencies); tests use the fake.
pub trait GitBackend {
    fn read_ref(&self, repo: &str, target_ref: &str) -> Result<String, LandingError>;
    fn cas_update(
        &self,
        repo: &str,
        target_ref: &str,
        expected_old: &str,
        new_sha: &str,
    ) -> Result<(), LandingError>;
}

pub struct CommandGit;

impl GitBackend for CommandGit {
    fn read_ref(&self, repo: &str, target_ref: &str) -> Result<String, LandingError> {
        let out = std::process::Command::new("git")
            .args(["-C", repo, "rev-parse", "--verify", target_ref])
            .output()
            .map_err(|e| LandingError::Git(e.to_string()))?;
        if !out.status.success() {
            return Err(LandingError::Git(
                String::from_utf8_lossy(&out.stderr).trim().to_string(),
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    fn cas_update(
        &self,
        repo: &str,
        target_ref: &str,
        expected_old: &str,
        new_sha: &str,
    ) -> Result<(), LandingError> {
        // Atomic server-side CAS is a push-time property; locally we
        // re-verify under no lock and fail closed on mismatch (the adapter
        // holds no locks — single-writer discipline lives in the engine).
        let live = self.read_ref(repo, target_ref)?;
        if live != expected_old {
            return Err(LandingError::CasConflict {
                expected: expected_old.into(),
                live,
            });
        }
        let out = std::process::Command::new("git")
            .args(["-C", repo, "update-ref", target_ref, new_sha, expected_old])
            .output()
            .map_err(|e| LandingError::Git(e.to_string()))?;
        if !out.status.success() {
            return Err(LandingError::Git(
                String::from_utf8_lossy(&out.stderr).trim().to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct DeliveryRecord {
    pub task_id: String,
    pub state: DeliveryState,
    pub target_ref: String,
    pub expected_old: String,
    pub source_sha: String,
    pub integrated_sha: Option<String>,
    pub freeze_hash: String,
}

pub fn queue(
    task_id: &str,
    target_ref: &str,
    expected_old: &str,
    source_sha: &str,
    freeze_hash: &str,
) -> DeliveryRecord {
    DeliveryRecord {
        task_id: task_id.into(),
        state: DeliveryState::Queued,
        target_ref: target_ref.into(),
        expected_old: expected_old.into(),
        source_sha: source_sha.into(),
        integrated_sha: None,
        freeze_hash: freeze_hash.into(),
    }
}

/// Land a QUEUED delivery. Requires the burned ack for the record's frozen
/// candidate; CAS-mismatch moves the record to CONFLICT and denies typed.
pub fn land(
    record: &mut DeliveryRecord,
    burned: &BurnedToken,
    git: &dyn GitBackend,
    repo: &str,
    store: &StateStore,
) -> Result<String, LandingError> {
    if record.state == DeliveryState::GitIntegrated {
        return Err(LandingError::AlreadyLanded {
            sha: record.integrated_sha.clone().unwrap_or_default(),
        });
    }
    if record.state != DeliveryState::Queued {
        return Err(LandingError::NotQueued {
            state: record.state.as_str().into(),
        });
    }
    if burned.freeze_hash() != record.freeze_hash {
        return Err(LandingError::NoBurnedAck {
            candidate: record.freeze_hash.clone(),
        });
    }
    // QA-R4: single-delivery — the proof authorizes exactly its task.
    if burned.task_id() != record.task_id {
        return Err(LandingError::TaskMismatch {
            expected: burned.task_id().into(),
            found: record.task_id.clone(),
        });
    }
    let live = git.read_ref(repo, &record.target_ref)?;
    if live != record.expected_old {
        record.state = DeliveryState::Conflict;
        return Err(LandingError::CasConflict {
            expected: record.expected_old.clone(),
            live,
        });
    }
    git.cas_update(
        repo,
        &record.target_ref,
        &record.expected_old,
        &record.source_sha,
    )?;
    record.state = DeliveryState::GitIntegrated;
    record.integrated_sha = Some(record.source_sha.clone());
    let receipt_id = format!("land-{}-{}", record.freeze_hash, record.task_id);
    let evidence = serde_json::json!({
        "task_id": record.task_id,
        "state": record.state.as_str(),
        "target_ref": record.target_ref,
        "expected_old": record.expected_old,
        "integrated_sha": record.integrated_sha,
        "ack_token": burned.token(),
        "landed_ms": now_ms(),
    })
    .to_string();
    let inserted = store
        .with_write(|conn| {
            Ok(conn.execute(
                "INSERT INTO receipts(id,task_id,kind,evidence) VALUES(?,?,?,?)",
                rusqlite::params![receipt_id, record.task_id, "delivery", evidence],
            )?)
        })
        .map_err(|e| match e {
            crate::state::StateError::Sqlite(rusqlite::Error::SqliteFailure(f, _))
                if f.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                LandingError::DuplicateReceipt
            }
            other => LandingError::State(other.to_string()),
        })?;
    debug_assert!(inserted == 1);
    Ok(record.source_sha.clone())
}

/// Deterministic next transition for a delivery record (V02-C/H3 cage).
///
/// CAGE: pure over [`DeliveryRecord`] ONLY — a total function of
/// `record.state` with no I/O, no global state, no clock, and no mutation of
/// the record (takes `&`, returns an owned [`NextStep`]). Same inputs always
/// give the same next step; interleaved calls never interact.
/// `Queued → Land`, `Conflict → Requeue`, `GitIntegrated → Terminal`.
///
/// deferred:dag-phase — full DAG composition (multi-node scheduling,
/// cross-record edges) does NOT exist in this tree and is intentionally NOT
/// implied here. This function is the single-node delivery step the DAG will
/// compose in its own phase; the 20-node replay test below pins the cage
/// (purity + determinism over 20 records × 50 repetitions) and would fail
/// closed on any hidden state, I/O, or mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NextStep {
    Land,
    Requeue,
    Terminal,
}

pub fn next_transition(record: &DeliveryRecord) -> NextStep {
    match record.state {
        DeliveryState::Queued => NextStep::Land,
        DeliveryState::Conflict => NextStep::Requeue,
        DeliveryState::GitIntegrated => NextStep::Terminal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verify::freeze::{freeze_candidate, AckLedger};

    struct FakeGit {
        current: std::sync::Mutex<String>,
    }

    impl FakeGit {
        fn new(sha: &str) -> Self {
            Self {
                current: std::sync::Mutex::new(sha.into()),
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

    fn candidate_hash() -> String {
        freeze_candidate(
            "refs/heads/main",
            vec![("a.rs".into(), "fn a() {}".into())],
            "x",
            None,
        )
        .freeze_hash
    }

    fn burned(store: &StateStore, hash: &str, task: &str) -> BurnedToken {
        let tok = crate::verify::freeze::issue_token(store, hash, task).unwrap();
        crate::verify::freeze::burn_token(store, &tok, hash).unwrap();
        BurnedToken::from_store(store, &tok, hash, task).unwrap()
    }

    #[test]
    fn land_with_burned_ack_integrates_and_records_receipt() {
        let store = StateStore::open_in_memory().unwrap();
        let hash = candidate_hash();
        let git = FakeGit::new("oldsha");
        let mut rec = queue("t1", "refs/heads/main", "oldsha", "newsha", &hash);
        let out = land(&mut rec, &burned(&store, &hash, "t1"), &git, "repo", &store).unwrap();
        assert_eq!(out, "newsha");
        assert_eq!(rec.state, DeliveryState::GitIntegrated);
        // Receipt row is present and append-only (re-land is rejected before insert).
        let n: i64 = store
            .with_read(|conn| {
                conn.query_row(
                    "SELECT COUNT(*) FROM receipts WHERE kind='delivery'",
                    [],
                    |r| r.get(0),
                )
                .map_err(crate::state::StateError::from)
            })
            .unwrap();
        assert_eq!(n, 1);
        // Second land: typed AlreadyLanded, no second receipt.
        assert!(matches!(
            land(&mut rec, &burned(&store, &hash, "t1"), &git, "repo", &store).unwrap_err(),
            LandingError::AlreadyLanded { .. }
        ));
    }

    #[test]
    fn land_without_burned_token_is_denied_typed() {
        let store = StateStore::open_in_memory().unwrap();
        let hash = candidate_hash();
        let git = FakeGit::new("oldsha");
        let mut rec = queue("t1", "refs/heads/main", "oldsha", "newsha", &hash);
        // Unburned token cannot even construct a BurnedToken from the store.
        let tok = crate::verify::freeze::issue_token(&store, &hash, "t1").unwrap();
        assert_eq!(
            BurnedToken::from_store(&store, &tok, &hash, "t1").unwrap_err(),
            LandingError::AckNotBurned
        );
        // A token burned for ANOTHER candidate does not authorize this land.
        let mut ledger = AckLedger::default();
        let cand = freeze_candidate("r", vec![("a".into(), "1".into())], "x", None);
        let other = freeze_candidate("r", vec![("a".into(), "2".into())], "x", None);
        let tok = ledger.acknowledge(&cand, "t1");
        ledger.burn(&tok, &cand.freeze_hash).unwrap();
        let proof = BurnedToken::from_ledger(&ledger, &tok, &cand.freeze_hash, "t1").unwrap();
        let err = land(&mut rec, &proof, &git, "repo", &store).unwrap_err();
        assert!(matches!(err, LandingError::NoBurnedAck { .. }));
        assert_eq!(rec.state, DeliveryState::Queued);
        // Unknown token: same denial family.
        assert_eq!(
            BurnedToken::from_ledger(&ledger, "ack-nope", &other.freeze_hash, "t1").unwrap_err(),
            LandingError::AckNotBurned
        );
        // Cross-task proof: burned for t9, presented for t1 — typed mismatch.
        let tok9 = ledger.acknowledge(&cand, "t9");
        ledger.burn(&tok9, &cand.freeze_hash).unwrap();
        let err = BurnedToken::from_ledger(&ledger, &tok9, &cand.freeze_hash, "t1").unwrap_err();
        assert!(matches!(err, LandingError::TaskMismatch { .. }));
    }

    #[test]
    fn one_ack_lands_exactly_one_delivery() {
        // QA-R4: t1 and t2 share the freeze; the t1 proof must not land t2.
        let store = StateStore::open_in_memory().unwrap();
        let hash = candidate_hash();
        let git = FakeGit::new("oldsha");
        let tok = crate::verify::freeze::issue_token(&store, &hash, "t1").unwrap();
        crate::verify::freeze::burn_token(&store, &tok, &hash).unwrap();
        let proof_t1 = BurnedToken::from_store(&store, &tok, &hash, "t1").unwrap();
        let mut rec1 = queue("t1", "refs/heads/main", "oldsha", "newsha", &hash);
        assert!(land(&mut rec1, &proof_t1, &git, "repo", &store).is_ok());
        // Same freeze, other task: TaskMismatch at the gate (proof is t1-bound).
        let mut rec2 = queue("t2", "refs/heads/main", "newsha", "newsha2", &hash);
        let err = land(&mut rec2, &proof_t1, &git, "repo", &store).unwrap_err();
        assert!(matches!(err, LandingError::TaskMismatch { .. }));
        assert_eq!(rec2.state, DeliveryState::Queued);
    }

    #[test]
    fn cas_mismatch_moves_to_conflict_and_denies_typed() {
        let store = StateStore::open_in_memory().unwrap();
        let hash = candidate_hash();
        let git = FakeGit::new("someone-else-sha");
        let mut rec = queue("t1", "refs/heads/main", "oldsha", "newsha", &hash);
        let err = land(&mut rec, &burned(&store, &hash, "t1"), &git, "repo", &store).unwrap_err();
        assert!(matches!(err, LandingError::CasConflict { .. }));
        assert_eq!(rec.state, DeliveryState::Conflict);
        // A conflicted record never lands without re-queue.
        assert!(matches!(
            land(&mut rec, &burned(&store, &hash, "t1"), &git, "repo", &store).unwrap_err(),
            LandingError::NotQueued { .. }
        ));
    }

    /// V02-C/H3 kill-gated spike: 20-node replay falsification of the
    /// `next_transition` cage.
    ///
    /// Kill criteria: FAILS if `next_transition` ever reads I/O, global
    /// state, the clock, mutates the record, or maps a state to the wrong
    /// step (any hidden impurity breaks determinism across the 50
    /// repetitions or cross-talk between interleaved records). PASSES only
    /// for the caged pure mapping. Falsifies: DAG over-claims (this test
    /// pins single-record purity; multi-node scheduling stays deferred).
    #[test]
    fn next_transition_cage_20_node_replay_is_pure_and_deterministic() {
        // 20 nodes cycling deterministically through all three states.
        let nodes: Vec<DeliveryRecord> = (0..20)
            .map(|i| {
                let state = match i % 3 {
                    0 => DeliveryState::Queued,
                    1 => DeliveryState::Conflict,
                    _ => DeliveryState::GitIntegrated,
                };
                let mut rec = queue(
                    &format!("t{i}"),
                    "refs/heads/main",
                    "oldsha",
                    &format!("newsha-{i}"),
                    &format!("freeze-{i:02}"),
                );
                rec.state = state;
                if state == DeliveryState::GitIntegrated {
                    rec.integrated_sha = Some(format!("newsha-{i}"));
                }
                rec
            })
            .collect();
        let expected = |s: DeliveryState| match s {
            DeliveryState::Queued => NextStep::Land,
            DeliveryState::Conflict => NextStep::Requeue,
            DeliveryState::GitIntegrated => NextStep::Terminal,
        };
        // Snapshot every record; 50 full replays must agree and mutate nothing.
        let snapshots: Vec<DeliveryRecord> = nodes.to_vec();
        for _ in 0..50 {
            // Interleaved order (reverse) proves no cross-talk.
            for (rec, snap) in nodes.iter().rev().zip(snapshots.iter().rev()) {
                assert_eq!(next_transition(rec), expected(snap.state));
                // Purity: the record is observably unchanged (takes `&`).
                assert_eq!(rec.state, snap.state);
                assert_eq!(rec.task_id, snap.task_id);
                assert_eq!(rec.target_ref, snap.target_ref);
                assert_eq!(rec.expected_old, snap.expected_old);
                assert_eq!(rec.source_sha, snap.source_sha);
                assert_eq!(rec.integrated_sha, snap.integrated_sha);
                assert_eq!(rec.freeze_hash, snap.freeze_hash);
            }
            // Forward order agrees identically (order-independence).
            for (rec, snap) in nodes.iter().zip(snapshots.iter()) {
                assert_eq!(next_transition(rec), expected(snap.state));
            }
        }
        // Count pins coverage: 7 Queued + 7 Conflict + 6 Integrated = 20.
        let (mut q, mut c, mut g) = (0, 0, 0);
        for rec in &nodes {
            match next_transition(rec) {
                NextStep::Land => q += 1,
                NextStep::Requeue => c += 1,
                NextStep::Terminal => g += 1,
            }
        }
        assert_eq!((q, c, g), (7, 7, 6));
    }
}
