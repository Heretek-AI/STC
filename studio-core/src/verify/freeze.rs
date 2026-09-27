//! Frozen-candidate evidence gate (Phase 6 WS8, the moat).
//! gentle-ai pattern extended to delivery: freeze the candidate diff → risk
//! tier → bounded lens depth → ONE bounded correction round → acknowledgement
//! receipt that BURNS. `MergeQueue::gated_merge_frozen` requires the burned
//! receipt to land. Repeat review of the same candidate finds the ack spent.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RiskTier {
    Low,
    Medium,
    High,
}

/// Risk from shape, not prose: sensitive paths, breadth, secret-adjacent words.
pub fn risk_tier(files: &[String], message: &str) -> RiskTier {
    let m = message.to_lowercase();
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrozenCandidate {
    pub freeze_hash: String,
    pub head_sha: String,
    pub files: Vec<String>,
    pub tier: RiskTier,
    /// Bounded review depth: Low 0 (structural readback), Medium 1, High 2.
    pub lens_depth: u32,
}

/// Freeze binds reviewed content: sorted files + head SHA + tier.
pub fn freeze_candidate(head_sha: &str, mut files: Vec<String>, message: &str) -> FrozenCandidate {
    files.sort();
    let tier = risk_tier(&files, message);
    let lens_depth = match tier {
        RiskTier::Low => 0,
        RiskTier::Medium => 1,
        RiskTier::High => 2,
    };
    let mut h = Sha256::new();
    h.update(head_sha.as_bytes());
    for f in &files {
        h.update([0u8]);
        h.update(f.as_bytes());
    }
    FrozenCandidate {
        freeze_hash: hex::encode(h.finalize()),
        head_sha: head_sha.into(),
        files,
        tier,
        lens_depth,
    }
}

/// Exactly one bounded correction per freeze. A second request fails closed:
/// re-freeze the new candidate instead of extending the round.
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

    pub fn request(&mut self) -> Result<(), String> {
        if self.attempts >= self.max {
            return Err("correction budget spent: re-freeze the new candidate".into());
        }
        self.attempts += 1;
        Ok(())
    }
}

impl Default for CorrectionBudget {
    fn default() -> Self {
        Self::new()
    }
}

/// Acknowledgement ledger: tokens are single-use. Burning twice (repeat review
/// of the same candidate) is rejected — the ack is spent.
#[derive(Debug, Default)]
pub struct AckLedger {
    issued: HashMap<String, String>, // token -> candidate freeze_hash
    burned: HashSet<String>,
    counter: u64,
}

impl AckLedger {
    /// Acknowledge a frozen candidate: mints a one-time token.
    pub fn acknowledge(&mut self, candidate: &FrozenCandidate) -> String {
        let tok = format!(
            "ack-{}-{}",
            self.counter,
            &candidate.freeze_hash[..8.min(candidate.freeze_hash.len())]
        );
        self.counter += 1;
        self.issued
            .insert(tok.clone(), candidate.freeze_hash.clone());
        tok
    }

    /// Burn a token: succeeds exactly once per token.
    pub fn burn(&mut self, token: &str) -> Result<String, String> {
        let hash = self
            .issued
            .get(token)
            .ok_or_else(|| "unknown acknowledgement token".to_string())?
            .clone();
        if !self.burned.insert(token.to_string()) {
            return Err("acknowledgement already spent".into());
        }
        Ok(hash)
    }

    /// True iff this token was burned for exactly this candidate.
    pub fn is_burned_for(&self, token: &str, candidate_hash: &str) -> bool {
        self.burned.contains(token)
            && self.issued.get(token).map(|h| h.as_str()) == Some(candidate_hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand() -> FrozenCandidate {
        freeze_candidate("abc123", vec!["b.rs".into(), "a.rs".into()], "fix typo")
    }

    #[test]
    fn freeze_is_deterministic_and_sorted() {
        let a = freeze_candidate("h", vec!["b".into(), "a".into()], "x");
        let b = freeze_candidate("h", vec!["a".into(), "b".into()], "x");
        assert_eq!(a.freeze_hash, b.freeze_hash);
        assert_eq!(a.files, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn risk_scales_with_shape() {
        assert_eq!(risk_tier(&["a.rs".into()], "fix typo"), RiskTier::Low);
        assert_eq!(
            risk_tier(
                &["a.rs".into(), "b.rs".into(), "c.rs".into(), "d.rs".into()],
                "x"
            ),
            RiskTier::Medium
        );
        assert_eq!(risk_tier(&["src/auth/mod.rs".into()], "x"), RiskTier::High);
        let c = cand();
        assert_eq!(c.lens_depth, 0);
        assert_eq!(
            freeze_candidate("h", vec!["src/vault/x".into()], "x").lens_depth,
            2
        );
    }

    #[test]
    fn correction_budget_is_single_use() {
        let mut b = CorrectionBudget::new();
        assert!(b.request().is_ok());
        assert!(b.request().is_err());
    }

    #[test]
    fn ack_burns_once() {
        let c = cand();
        let mut ledger = AckLedger::default();
        let tok = ledger.acknowledge(&c);
        let hash = ledger.burn(&tok).unwrap();
        assert_eq!(hash, c.freeze_hash);
        assert!(ledger.is_burned_for(&tok, &c.freeze_hash));
        // Repeat review of the same candidate: ack is spent.
        assert!(ledger.burn(&tok).is_err());
        assert!(ledger.burn("ack-unknown").is_err());
        // Wrong candidate hash never validates.
        assert!(!ledger.is_burned_for(&tok, "other"));
    }
}
