//! Frozen-candidate evidence gate (phase 02): the moat mechanism.
//!
//! Port of v1 `studio-core/src/verify/freeze.rs`
//! (`c1e49e97f6429dec4604dbcf76e3e4a27fa577284c0e0a82254dddce1ac8c29f`,
//! FILE_HASH_ONLY) **with the contract rewritten where v1 was wrong** (Q9
//! bar): v1 froze `(head_sha, file paths, tier)` only, so the review gates
//! ran against the LIVE tree while the receipt named a FROZEN candidate — a
//! file edited mid-review silently changed what was "reviewed". This rewrite
//! binds every target's CONTENT sha256 at freeze time, and every gate must
//! call [`FrozenCandidate::verify_tree`] against the live tree before
//! evaluating: any divergence is a typed [`FreezeError::TreeDiverged`]
//! (re-freeze the new candidate), never a silent pass.
//!
//! What v1 got right and is kept: sorted-file determinism, shape-based risk
//! tiers with bounded lens depth, exactly-one correction budget, burn-once
//! ack ledger. What changed besides content binding: typed errors everywhere
//! (v1 used `String`), `lineage_hash` + `revision` chain (GOAL: lineage /
//! revision / target hashes), a fourth `Release` tier for owner-authorized
//! local releases (yylo `release` tier, clean-room c012/c013), and durable
//! persistence of candidates + ack tokens in the v2 DB (phase-01 additive
//! tables; schema_version stays 2).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use thiserror::Error;

use crate::state::StateStore;

pub const GENESIS_LINEAGE: &str = "genesis";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FreezeError {
    #[error("live tree diverged from frozen candidate {freeze_hash}: {detail}")]
    TreeDiverged { freeze_hash: String, detail: String },
    #[error("correction budget spent: re-freeze the new candidate")]
    BudgetSpent,
    #[error("unknown acknowledgement token")]
    UnknownToken,
    #[error("acknowledgement already spent")]
    AlreadySpent,
    #[error("ack token {token} was issued for {issued_for}, not {candidate}")]
    WrongCandidate {
        token: String,
        issued_for: String,
        candidate: String,
    },
    #[error("state: {0}")]
    State(String),
}

/// Risk from shape, not prose: sensitive paths, breadth, secret-adjacent
/// words. Ported from v1 (same Low/Medium/High rules); `Release` is new —
/// owner-authorized local release, the highest validation depth (yylo
/// `release` tier, clean-room reading of c012
/// `1c282de21deb4a7a69431c3c20ee6dc2b6f380a0128ee5c0401629c559aef86a` and c013
/// `82bcea6a0531409e41074b6b7519dbafc6f59fa01e0edc2371d5b3c93bde67e7`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RiskTier {
    Low,
    Medium,
    High,
    Release,
}

pub fn risk_tier(files: &[String], message: &str) -> RiskTier {
    let m = message.to_lowercase();
    if m.contains("[release]") || m.contains("owner-authorized release") {
        return RiskTier::Release;
    }
    let sensitive = [
        "auth", "secret", "crypto", "ledger", "migrate", "vault", "keyring",
    ];
    if files.len() > 10
        || files.iter().any(|f| {
            let f = f.to_lowercase();
            sensitive.iter().any(|s| f.contains(s))
        })
        || ["vuln", "exploit", "bypass", "secret"]
            .iter()
            .any(|w| m.contains(w))
    {
        return RiskTier::High;
    }
    if files.len() > 3 || m.contains("refactor") || m.contains("migration") {
        return RiskTier::Medium;
    }
    RiskTier::Low
}

/// Bounded review depth per tier: Low 0 (structural readback), Medium 1,
/// High 2, Release 2 + full release gate. `full_suite` mirrors yylo
/// `full_suite_tiers` (clean-room): only High/Release run the full suite.
pub fn validation_depth(tier: RiskTier) -> (u32, bool) {
    match tier {
        RiskTier::Low => (0, false),
        RiskTier::Medium => (1, false),
        RiskTier::High => (2, true),
        RiskTier::Release => (2, true),
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// An immutable frozen candidate. `freeze_hash` binds: lineage (parent chain),
/// revision (monotonic), target ref, every target path AND its content digest
/// at freeze time, and the tier. Two freezes of identical inputs produce
/// identical hashes (reproducible); any content change produces a new hash.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FrozenCandidate {
    pub freeze_hash: String,
    pub lineage_hash: String,
    pub revision: u64,
    pub target_ref: String,
    pub targets: Vec<String>,
    pub content_hashes: HashMap<String, String>,
    pub tier: RiskTier,
    pub lens_depth: u32,
    pub full_suite: bool,
}

/// Freeze a candidate. `files` is `(path, content)` — the CONTENT is hashed,
/// so the receipt names what was actually reviewed, not what the paths
/// happened to hold later. `parent` chains lineage (`GENESIS_LINEAGE` when
/// `None`) and bumps the revision.
pub fn freeze_candidate(
    target_ref: &str,
    mut files: Vec<(String, String)>,
    message: &str,
    parent: Option<&FrozenCandidate>,
) -> FrozenCandidate {
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let paths: Vec<String> = files.iter().map(|(p, _)| p.clone()).collect();
    let tier = risk_tier(&paths, message);
    let (lens_depth, full_suite) = validation_depth(tier);
    let (lineage_hash, revision) = match parent {
        Some(p) => (p.freeze_hash.clone(), p.revision + 1),
        None => (GENESIS_LINEAGE.to_string(), 0),
    };
    let mut content_hashes = HashMap::new();
    let mut h = Sha256::new();
    h.update(lineage_hash.as_bytes());
    h.update([0u8]);
    h.update(revision.to_be_bytes());
    h.update([0u8]);
    h.update(target_ref.as_bytes());
    for (path, content) in &files {
        let digest = sha256_hex(content.as_bytes());
        content_hashes.insert(path.clone(), digest.clone());
        h.update([0u8]);
        h.update(path.as_bytes());
        h.update([0u8]);
        h.update(digest.as_bytes());
    }
    FrozenCandidate {
        freeze_hash: hex::encode(h.finalize()),
        lineage_hash,
        revision,
        target_ref: target_ref.into(),
        targets: paths,
        content_hashes,
        tier,
        lens_depth,
        full_suite,
    }
}

impl FrozenCandidate {
    /// Q9 enforcement: prove the live tree still equals the frozen snapshot
    /// before any gate evaluates it. Returns the exact divergence (changed /
    /// added / removed paths) as a typed rejection — the caller re-freezes
    /// the new candidate instead of reviewing stale content.
    pub fn verify_tree(&self, live: &[(String, String)]) -> Result<(), FreezeError> {
        let mut live_map = HashMap::new();
        for (p, c) in live {
            live_map.insert(p.clone(), sha256_hex(c.as_bytes()));
        }
        let mut problems = vec![];
        for t in &self.targets {
            match live_map.get(t) {
                Some(d) if d == &self.content_hashes[t] => {}
                Some(_) => problems.push(format!("changed:{t}")),
                None => problems.push(format!("removed:{t}")),
            }
        }
        for p in live_map.keys() {
            if !self.content_hashes.contains_key(p) {
                problems.push(format!("added:{p}"));
            }
        }
        if problems.is_empty() {
            Ok(())
        } else {
            problems.sort();
            Err(FreezeError::TreeDiverged {
                freeze_hash: self.freeze_hash.clone(),
                detail: problems.join(","),
            })
        }
    }
}

/// Exactly one bounded correction per freeze — RUNTIME-ENFORCED (QA-R1).
/// A blocked gate run consumes the single per-freeze attempt via
/// [`claim_correction`], persisted in the DB, so enforcement never depends on
/// the caller holding (or honestly reusing) a `CorrectionBudget` object: a
/// fresh object per attempt buys nothing. This struct remains as an advisory
/// pre-flight helper; the DB is the authority.
#[derive(Debug)]
pub struct CorrectionBudget {
    attempts: u32,
    pub max: u32,
}

impl CorrectionBudget {
    pub fn new() -> Self {
        Self {
            attempts: 0,
            max: 1,
        }
    }

    pub fn request(&mut self) -> Result<(), FreezeError> {
        if self.attempts >= self.max {
            return Err(FreezeError::BudgetSpent);
        }
        self.attempts += 1;
        Ok(())
    }

    pub fn spent(&self) -> bool {
        self.attempts >= self.max
    }
}

impl Default for CorrectionBudget {
    fn default() -> Self {
        Self::new()
    }
}

/// QA-R1: consume one correction attempt for a frozen candidate, in the DB.
/// First blocked run per `freeze_hash` succeeds (the caller may correct once
/// and retry); any further blocked run for the SAME hash fails closed with
/// [`FreezeError::BudgetSpent`] — even if the caller passes a fresh budget
/// object or none at all. Check-and-increment runs inside one
/// `BEGIN IMMEDIATE` write txn, so concurrent claimers cannot both succeed.
pub const CORRECTION_MAX_ATTEMPTS: u32 = 1;

pub fn claim_correction(store: &StateStore, freeze_hash: &str) -> Result<(), FreezeError> {
    let exhausted: bool = store
        .with_write(|conn| {
            let current: Option<i64> = conn
                .query_row(
                    "SELECT attempts FROM correction_attempts WHERE freeze_hash=?",
                    rusqlite::params![freeze_hash],
                    |r| r.get(0),
                )
                .map(Some)
                .or_else(|e| match e {
                    rusqlite::Error::QueryReturnedNoRows => Ok(None),
                    other => Err(other),
                })?;
            if current.unwrap_or(0) >= CORRECTION_MAX_ATTEMPTS as i64 {
                return Ok(true);
            }
            conn.execute(
                "INSERT INTO correction_attempts(freeze_hash,attempts) VALUES(?,1) ON CONFLICT(freeze_hash) DO UPDATE SET attempts=attempts+1",
                rusqlite::params![freeze_hash],
            )?;
            Ok(false)
        })
        .map_err(|e| FreezeError::State(e.to_string()))?;
    if exhausted {
        return Err(FreezeError::BudgetSpent);
    }
    Ok(())
}

/// Acknowledgement ledger: tokens are single-use and candidate-bound AND
/// task-bound (QA-R4: one ack lands exactly one delivery — a token burned for
/// task t1 never authorizes task t2, even for the same freeze). Burning twice
/// (repeat review of the same candidate) is a typed rejection — the ack
/// is spent. (Ported from v1; `String` errors replaced by [`FreezeError`].)
#[derive(Debug, Default)]
pub struct AckLedger {
    issued: HashMap<String, (String, String)>,
    burned: HashMap<String, (String, String)>,
    counter: u64,
}

impl AckLedger {
    /// Acknowledge a frozen candidate for one task: mints a one-time token
    /// bound to both. `burn` spends without checking the task; the BINDING is
    /// enforced at proof construction (`BurnedToken::from_ledger`, which
    /// requires the task) and re-checked at `land`.
    pub fn acknowledge(&mut self, candidate: &FrozenCandidate, task_id: &str) -> String {
        let tok = format!(
            "ack-{}-{}",
            self.counter,
            &candidate.freeze_hash[..8.min(candidate.freeze_hash.len())]
        );
        self.counter += 1;
        self.issued
            .insert(tok.clone(), (candidate.freeze_hash.clone(), task_id.into()));
        tok
    }

    /// Burn a token for exactly the candidate it was issued for. Succeeds
    /// exactly once per token.
    pub fn burn(&mut self, token: &str, candidate_hash: &str) -> Result<(), FreezeError> {
        let (issued_for, issued_task) = self
            .issued
            .get(token)
            .ok_or(FreezeError::UnknownToken)?
            .clone();
        if issued_for != candidate_hash {
            return Err(FreezeError::WrongCandidate {
                token: token.into(),
                issued_for,
                candidate: candidate_hash.into(),
            });
        }
        if self.burned.contains_key(token) {
            return Err(FreezeError::AlreadySpent);
        }
        self.burned
            .insert(token.into(), (candidate_hash.into(), issued_task));
        Ok(())
    }

    /// True iff this token was burned for exactly this candidate.
    pub fn is_burned_for(&self, token: &str, candidate_hash: &str) -> bool {
        self.burned.get(token).map(|(h, _)| h.as_str()) == Some(candidate_hash)
    }

    /// The (candidate, task) binding a burned token carries, if burned.
    /// `BurnedToken::from_ledger` enforces both halves against this.
    pub fn burned_binding(&self, token: &str) -> Option<(&str, &str)> {
        self.burned
            .get(token)
            .map(|(h, t)| (h.as_str(), t.as_str()))
    }
}

/// Durable persistence of candidates + ack tokens (phase-01 additive tables).
pub fn save_candidate(store: &StateStore, c: &FrozenCandidate) -> Result<(), FreezeError> {
    let targets =
        serde_json::to_string(&c.targets).map_err(|e| FreezeError::State(e.to_string()))?;
    let contents =
        serde_json::to_string(&c.content_hashes).map_err(|e| FreezeError::State(e.to_string()))?;
    let tier = format!("{:?}", c.tier);
    let created = now_ms();
    store
        .with_write(|conn| {
            conn.execute(
                "INSERT OR IGNORE INTO frozen_candidates(freeze_hash,lineage_hash,revision,target_ref,targets_json,contents_json,tier,created_ms) VALUES(?,?,?,?,?,?,?,?)",
                rusqlite::params![
                    c.freeze_hash, c.lineage_hash, c.revision as i64, c.target_ref,
                    targets, contents, tier, created as i64
                ],
            )?;
            Ok(())
        })
        .map_err(|e| FreezeError::State(e.to_string()))
}

pub fn load_candidate(
    store: &StateStore,
    freeze_hash: &str,
) -> Result<Option<FrozenCandidate>, FreezeError> {
    store
        .with_read(|conn| {
            let mut stmt = conn.prepare(
                "SELECT lineage_hash,revision,target_ref,targets_json,contents_json,tier FROM frozen_candidates WHERE freeze_hash=?",
            )?;
            let mut rows = stmt.query(rusqlite::params![freeze_hash])?;
            let row = rows.next()?;
            match row {
                None => Ok(None),
                Some(r) => {
                    let lineage_hash: String = r.get(0)?;
                    let revision: i64 = r.get(1)?;
                    let target_ref: String = r.get(2)?;
                    let targets_json: String = r.get(3)?;
                    let contents_json: String = r.get(4)?;
                    let tier_s: String = r.get(5)?;
                    let targets: Vec<String> = serde_json::from_str(&targets_json)
                        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))?;
                    let content_hashes: HashMap<String, String> = serde_json::from_str(&contents_json)
                        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))?;
                    let tier = match tier_s.as_str() {
                        "Low" => RiskTier::Low,
                        "Medium" => RiskTier::Medium,
                        "High" => RiskTier::High,
                        _ => RiskTier::Release,
                    };
                    let (lens_depth, full_suite) = validation_depth(tier);
                    Ok(Some(FrozenCandidate {
                        freeze_hash: freeze_hash.into(),
                        lineage_hash,
                        revision: revision as u64,
                        target_ref,
                        targets,
                        content_hashes,
                        tier,
                        lens_depth,
                        full_suite,
                    }))
                }
            }
        })
        .map_err(|e| FreezeError::State(e.to_string()))
}

/// Durable ack tokens: issue + burn-once enforced inside one write txn each.
/// Tokens are task-bound at issue (QA-R4): the binding is stored on the row
/// and enforced at proof construction (`BurnedToken::from_store`) and at
/// `land`, so one ack lands exactly one delivery.
pub fn issue_token(
    store: &StateStore,
    candidate_hash: &str,
    task_id: &str,
) -> Result<String, FreezeError> {
    // Monotonic nonce: two issues inside the same millisecond must still mint
    // distinct tokens, or the PK insert would fail closed on a retry.
    use std::sync::atomic::{AtomicU64, Ordering};
    static ISSUE_COUNTER: AtomicU64 = AtomicU64::new(0);
    let nonce = ISSUE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let seed = format!("{candidate_hash}:{}:{nonce}", now_ms());
    let token = format!(
        "ack-{candidate_hash:.8}-{}",
        &sha256_hex(seed.as_bytes())[..12]
    );
    let created = now_ms();
    store
        .with_write(|conn| {
            conn.execute(
                "INSERT INTO ack_tokens(token,freeze_hash,task_id,issued_ms,burned) VALUES(?,?,?,?,0)",
                rusqlite::params![token, candidate_hash, task_id, created as i64],
            )?;
            Ok(())
        })
        .map_err(|e| FreezeError::State(e.to_string()))?;
    Ok(token)
}

pub fn burn_token(
    store: &StateStore,
    token: &str,
    candidate_hash: &str,
) -> Result<(), FreezeError> {
    // Two steps, fail-closed order: (1) read the binding for typed
    // Unknown/WrongCandidate; (2) guarded UPDATE inside one write txn — the
    // rows-affected count decides AlreadySpent atomically, so two concurrent
    // burners cannot both succeed (single-writer BEGIN IMMEDIATE; the loser
    // sees burned=1 and is rejected).
    let issued_for: Option<String> = store
        .with_read(|conn| {
            conn.query_row(
                "SELECT freeze_hash FROM ack_tokens WHERE token=?",
                rusqlite::params![token],
                |r| r.get(0),
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })
            .map_err(crate::state::StateError::from)
        })
        .map_err(|e| FreezeError::State(e.to_string()))?;
    let issued_for = issued_for.ok_or(FreezeError::UnknownToken)?;
    if issued_for != candidate_hash {
        return Err(FreezeError::WrongCandidate {
            token: token.into(),
            issued_for,
            candidate: candidate_hash.into(),
        });
    }
    let changed = store
        .with_write(|conn| {
            Ok(conn.execute(
                "UPDATE ack_tokens SET burned=1 WHERE token=? AND burned=0",
                rusqlite::params![token],
            )?)
        })
        .map_err(|e| FreezeError::State(e.to_string()))?;
    if changed == 0 {
        return Err(FreezeError::AlreadySpent);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files() -> Vec<(String, String)> {
        vec![
            ("b.rs".into(), "fn b() {}".into()),
            ("a.rs".into(), "fn a() {}".into()),
        ]
    }

    #[test]
    fn freeze_is_deterministic_sorted_and_content_bound() {
        let a = freeze_candidate("main", files(), "fix typo", None);
        let mut rev = files();
        rev.reverse();
        let b = freeze_candidate("main", rev, "fix typo", None);
        assert_eq!(a.freeze_hash, b.freeze_hash);
        assert_eq!(a.targets, vec!["a.rs".to_string(), "b.rs".to_string()]);
        assert_eq!(a.lineage_hash, GENESIS_LINEAGE);
        assert_eq!(a.revision, 0);
        // Same paths, different content -> different freeze.
        let c = freeze_candidate(
            "main",
            vec![
                ("b.rs".into(), "fn b() {}".into()),
                ("a.rs".into(), "fn CHANGED() {}".into()),
            ],
            "fix typo",
            None,
        );
        assert_ne!(a.freeze_hash, c.freeze_hash);
    }

    #[test]
    fn lineage_chains_and_revisions_increase() {
        let a = freeze_candidate("main", files(), "x", None);
        let b = freeze_candidate("main", files(), "x", Some(&a));
        assert_eq!(b.lineage_hash, a.freeze_hash);
        assert_eq!(b.revision, 1);
        assert_ne!(a.freeze_hash, b.freeze_hash);
    }

    #[test]
    fn verify_tree_accepts_identical_and_rejects_divergence_typed() {
        let c = freeze_candidate("main", files(), "x", None);
        assert!(c.verify_tree(&files()).is_ok());
        let changed = vec![
            ("b.rs".into(), "fn b() {}".into()),
            ("a.rs".into(), "EDITED".into()),
        ];
        let err = c.verify_tree(&changed).unwrap_err();
        assert!(matches!(err, FreezeError::TreeDiverged { .. }));
        assert!(err.to_string().contains("changed:a.rs"));
        let removed = vec![("a.rs".into(), "fn a() {}".into())];
        let err = c.verify_tree(&removed).unwrap_err();
        assert!(err.to_string().contains("removed:b.rs"));
        let added = [files(), vec![("c.rs".into(), "new".into())]].concat();
        let err = c.verify_tree(&added).unwrap_err();
        assert!(err.to_string().contains("added:c.rs"));
    }

    #[test]
    fn risk_scales_with_shape_and_release_tier_is_highest() {
        assert_eq!(risk_tier(&["a.rs".into()], "fix typo"), RiskTier::Low);
        assert_eq!(
            risk_tier(
                &["a.rs".into(), "b.rs".into(), "c.rs".into(), "d.rs".into()],
                "x"
            ),
            RiskTier::Medium
        );
        assert_eq!(risk_tier(&["src/auth/mod.rs".into()], "x"), RiskTier::High);
        assert_eq!(
            risk_tier(&["a.rs".into()], "[release] ship it"),
            RiskTier::Release
        );
        assert_eq!(validation_depth(RiskTier::Low), (0, false));
        assert_eq!(validation_depth(RiskTier::High), (2, true));
        assert_eq!(validation_depth(RiskTier::Release), (2, true));
    }

    #[test]
    fn correction_budget_is_single_use_typed() {
        let mut b = CorrectionBudget::new();
        assert!(b.request().is_ok());
        assert!(b.spent());
        assert_eq!(b.request().unwrap_err(), FreezeError::BudgetSpent);
    }

    #[test]
    fn claim_correction_is_per_freeze_and_survives_fresh_objects() {
        // QA-R1: the DB — not any in-memory object — is the authority.
        let store = StateStore::open_in_memory().unwrap();
        let c = freeze_candidate("main", files(), "x", None);
        assert!(claim_correction(&store, &c.freeze_hash).is_ok());
        assert_eq!(
            claim_correction(&store, &c.freeze_hash).unwrap_err(),
            FreezeError::BudgetSpent
        );
        // A different freeze hash is unaffected.
        let other = freeze_candidate("main", vec![("z.rs".into(), "1".into())], "x", None);
        assert!(claim_correction(&store, &other.freeze_hash).is_ok());
        assert_eq!(CORRECTION_MAX_ATTEMPTS, 1);
    }

    #[test]
    fn ack_burns_once_and_binds_candidate_and_task() {
        let c = freeze_candidate("main", files(), "x", None);
        let mut ledger = AckLedger::default();
        let tok = ledger.acknowledge(&c, "t1");
        assert!(ledger.burn(&tok, &c.freeze_hash).is_ok());
        assert!(ledger.is_burned_for(&tok, &c.freeze_hash));
        assert_eq!(
            ledger.burn(&tok, &c.freeze_hash).unwrap_err(),
            FreezeError::AlreadySpent
        );
        assert_eq!(
            ledger.burn("ack-unknown", &c.freeze_hash).unwrap_err(),
            FreezeError::UnknownToken
        );
        assert!(!ledger.is_burned_for(&tok, "other"));
        // Token issued for one candidate never burns for another.
        let tok2 = ledger.acknowledge(&c, "t1");
        let err = ledger.burn(&tok2, "other").unwrap_err();
        assert!(matches!(err, FreezeError::WrongCandidate { .. }));
        // Binding recorded: the ledger carries the task with the burn.
        assert_eq!(
            ledger.burned_binding(&tok),
            Some((c.freeze_hash.as_str(), "t1"))
        );
    }

    #[test]
    fn durable_candidate_and_token_round_trip() {
        let store = StateStore::open_in_memory().unwrap();
        let c = freeze_candidate("main", files(), "x", None);
        save_candidate(&store, &c).unwrap();
        let back = load_candidate(&store, &c.freeze_hash).unwrap().unwrap();
        assert_eq!(back, c);
        assert!(load_candidate(&store, "missing").unwrap().is_none());
        let tok = issue_token(&store, &c.freeze_hash, "t1").unwrap();
        assert!(burn_token(&store, &tok, &c.freeze_hash).is_ok());
        assert_eq!(
            burn_token(&store, &tok, &c.freeze_hash).unwrap_err(),
            FreezeError::AlreadySpent
        );
        assert_eq!(
            burn_token(&store, "nope", &c.freeze_hash).unwrap_err(),
            FreezeError::UnknownToken
        );
    }
}
