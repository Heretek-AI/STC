//! Retention + reapers from day one. Reaping requires explicit bounded caps;
//! an unbounded reap request is refused (the never-delete guard works both ways:
//! data must flow out only through a bounded, audited reaper).

use super::store::MemoryStore;

#[derive(Debug, Clone)]
pub struct RetentionPolicy {
    pub max_bytes: i64,
    pub max_docs: i64,
}

impl RetentionPolicy {
    pub fn validate(&self) -> Result<(), String> {
        if self.max_bytes <= 0 || self.max_docs <= 0 {
            return Err(
                "retention caps must be positive and bounded (refusing unbounded reap)".into(),
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReapReport {
    pub removed_docs: i64,
    pub bytes_before: i64,
    pub bytes_after: i64,
}

pub fn reap(store: &MemoryStore, policy: &RetentionPolicy) -> Result<ReapReport, String> {
    policy.validate()?;
    store
        .reap(policy.max_bytes, policy.max_docs)
        .map(|(removed_docs, bytes_before, bytes_after)| ReapReport {
            removed_docs,
            bytes_before,
            bytes_after,
        })
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_unbounded_reap() {
        let p = RetentionPolicy {
            max_bytes: 0,
            max_docs: 100,
        };
        assert!(p.validate().is_err());
    }

    #[test]
    fn bounded_reap_removes_oldest_first() {
        let m = super::super::store::MemoryStore::open_in_memory().unwrap();
        m.put("old", "L1", "oldest doc body", 1).unwrap();
        m.put("new", "L1", "newest doc body", 2).unwrap();
        let rep = reap(
            &m,
            &RetentionPolicy {
                max_bytes: 1_000_000,
                max_docs: 1,
            },
        )
        .unwrap();
        assert_eq!(rep.removed_docs, 1);
        assert!(rep.bytes_after < rep.bytes_before);
        let hits = m.search_fts("oldest", 10).unwrap();
        assert!(!hits.iter().any(|(id, _)| id == "old"));
    }
}
