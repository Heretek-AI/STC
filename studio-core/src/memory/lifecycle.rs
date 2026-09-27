//! L3 lifecycle assertions: veracity / supersession / trust rows, anchored to
//! plan-ledger hashes. `valid_from_seq` / `valid_to_seq` reference
//! `ledger::LedgerEvent.seq` (bi-temporal supersession with cryptographic
//! backing); retrieval excludes superseded rows by default.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifecycleRow {
    pub doc_id: String,
    /// 0.0 (refuted) .. 1.0 (verified)
    pub veracity: f64,
    /// superseding doc_id, if this row is stale
    pub superseded_by: Option<String>,
    /// trust weight for retrieval blending
    pub trust: f64,
    /// plan-ledger seq this fact becomes valid at (None = since forever)
    pub valid_from_seq: Option<i64>,
    /// plan-ledger seq this fact stops being valid at (None = still live)
    pub valid_to_seq: Option<i64>,
    /// ledger-anchored supersession link (supersedes_id, if this row replaced one)
    pub supersedes_id: Option<String>,
}

impl LifecycleRow {
    pub fn live(doc_id: &str) -> Self {
        Self {
            doc_id: doc_id.into(),
            veracity: 1.0,
            superseded_by: None,
            trust: 1.0,
            valid_from_seq: None,
            valid_to_seq: None,
            supersedes_id: None,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if !(0.0..=1.0).contains(&self.veracity) {
            return Err(format!("veracity {} out of range", self.veracity));
        }
        if !(0.0..=1.0).contains(&self.trust) {
            return Err(format!("trust {} out of range", self.trust));
        }
        if let (Some(f), Some(t)) = (self.valid_from_seq, self.valid_to_seq) {
            if f > t {
                return Err(format!("valid_from_seq {f} > valid_to_seq {t}"));
            }
        }
        Ok(())
    }

    pub fn is_live(&self) -> bool {
        self.superseded_by.is_none() && self.valid_to_seq.is_none() && self.veracity >= 0.5
    }

    /// Live as of a plan-ledger seq (bi-temporal read).
    pub fn live_at(&self, seq: i64) -> bool {
        if !self.is_live() {
            return false;
        }
        if let Some(f) = self.valid_from_seq {
            if seq < f {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_iff_verified_and_current() {
        let live = LifecycleRow::live("a");
        assert!(live.is_live());
        let stale = LifecycleRow {
            superseded_by: Some("b".into()),
            ..live.clone()
        };
        assert!(!stale.is_live());
        let expired = LifecycleRow {
            valid_to_seq: Some(7),
            ..live.clone()
        };
        assert!(!expired.is_live());
        assert!(!LifecycleRow {
            valid_from_seq: Some(10),
            ..live.clone()
        }
        .live_at(9));
        assert!(LifecycleRow {
            valid_from_seq: Some(10),
            ..live.clone()
        }
        .live_at(10));
        assert!(!LifecycleRow {
            veracity: 0.1,
            ..live.clone()
        }
        .is_live());
    }

    #[test]
    fn window_validated() {
        let mut r = LifecycleRow::live("a");
        r.valid_from_seq = Some(5);
        r.valid_to_seq = Some(3);
        assert!(r.validate().is_err());
    }
}
