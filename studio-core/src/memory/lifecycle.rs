//! L3 lifecycle assertions: veracity / supersession / trust rows.

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
}

impl LifecycleRow {
    pub fn validate(&self) -> Result<(), String> {
        if !(0.0..=1.0).contains(&self.veracity) {
            return Err(format!("veracity {} out of range", self.veracity));
        }
        if !(0.0..=1.0).contains(&self.trust) {
            return Err(format!("trust {} out of range", self.trust));
        }
        Ok(())
    }

    pub fn is_live(&self) -> bool {
        self.superseded_by.is_none() && self.veracity >= 0.5
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_iff_verified_and_current() {
        let live = LifecycleRow {
            doc_id: "a".into(),
            veracity: 0.9,
            superseded_by: None,
            trust: 0.8,
        };
        assert!(live.is_live());
        let stale = LifecycleRow {
            superseded_by: Some("b".into()),
            ..live.clone()
        };
        assert!(!stale.is_live());
    }
}
