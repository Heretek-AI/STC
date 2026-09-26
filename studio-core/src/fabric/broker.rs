//! SQLite claim broker: deterministic overlap rules over `claims(scope_hash, owner)`.
//!
//! - Identical scope_hash held by another owner => reject (queue via `queued` list).
//! - Disjoint claims => always allow (H1: 0 collisions on non-overlapping).
//!
//! `BEGIN IMMEDIATE` transactions; fail-closed on contention.

use super::scope::{scope_hash, ScopeClaim};
use rusqlite::{params, Connection};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ClaimError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("collision: scope {0} held by {1}")]
    Collision(String, String),
}

pub struct ClaimBroker {
    conn: Connection,
}

impl ClaimBroker {
    pub fn open_in_memory() -> Result<Self, ClaimError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS claims(scope_hash TEXT PRIMARY KEY, owner TEXT, file TEXT);",
        )?;
        Ok(Self { conn })
    }

    /// Acquire all claims atomically. Any collision on another owner's hash => whole set rejected.
    pub fn acquire(&self, owner: &str, claims: &[ScopeClaim]) -> Result<(), ClaimError> {
        self.conn.execute_batch("BEGIN IMMEDIATE;")?;
        let mut holders: Vec<(String, String)> = vec![];
        for c in claims {
            let h = scope_hash(c);
            let row: Option<String> = self
                .conn
                .query_row(
                    "SELECT owner FROM claims WHERE scope_hash=?",
                    params![h],
                    |r| r.get(0),
                )
                .ok();
            if let Some(who) = row {
                if who != owner {
                    holders.push((h, who));
                }
            }
        }
        if !holders.is_empty() {
            let _ = self.conn.execute_batch("ROLLBACK;");
            let (h, who) = holders.into_iter().next().unwrap();
            return Err(ClaimError::Collision(h, who));
        }
        for c in claims {
            let h = scope_hash(c);
            self.conn.execute(
                "INSERT OR REPLACE INTO claims(scope_hash, owner, file) VALUES(?,?,?)",
                params![h, owner, c.file],
            )?;
        }
        self.conn.execute_batch("COMMIT;")?;
        Ok(())
    }

    pub fn release_owner(&self, owner: &str) -> Result<(), ClaimError> {
        self.conn
            .execute("DELETE FROM claims WHERE owner=?", params![owner])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disjoint_claims_never_collide() {
        let b = ClaimBroker::open_in_memory().unwrap();
        let a = vec![ScopeClaim {
            file: "a.rs".into(),
            func: Some("f1".into()),
            symbol: None,
        }];
        let c = vec![ScopeClaim {
            file: "a.rs".into(),
            func: Some("f2".into()),
            symbol: None,
        }];
        b.acquire("agent-1", &a).unwrap();
        b.acquire("agent-2", &c).unwrap(); // must not collide (H1)
    }

    #[test]
    fn identical_scope_rejected() {
        let b = ClaimBroker::open_in_memory().unwrap();
        let a = vec![ScopeClaim::file_only("a.rs")];
        b.acquire("agent-1", &a).unwrap();
        assert!(matches!(
            b.acquire("agent-2", &a),
            Err(ClaimError::Collision(_, _))
        ));
    }
}
