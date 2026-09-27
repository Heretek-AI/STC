//! L1 facts + FTS5 store (lives in the same `studio.db` connection family).

use rusqlite::{params, Connection};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MemoryError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("migration: {0}")]
    Migrate(#[from] crate::migrate::MigrateError),
    #[error("write refused: {0}")]
    Denied(String),
    #[error("io: {0}")]
    Io(String),
}

pub struct MemoryStore {
    conn: Connection,
}

impl MemoryStore {
    pub const SCHEMA_VERSION: i64 = 2;

    pub fn open_in_memory() -> Result<Self, MemoryError> {
        let conn = Connection::open_in_memory()?;
        let s = Self { conn };
        s.migrate(None)?;
        Ok(s)
    }

    pub fn open(path: &str) -> Result<Self, MemoryError> {
        let conn = Connection::open(path)?;
        let s = Self { conn };
        s.migrate(crate::state::StateStore::backup_sibling_for(path))?;
        Ok(s)
    }

    fn migrate(&self, backup: Option<std::path::PathBuf>) -> Result<(), MemoryError> {
        crate::migrate::run_migrations(
            &self.conn,
            Self::SCHEMA_VERSION,
            &[
                crate::migrate::Migration {
                    version: 1,
                    name: "memory baseline (docs, fts, lifecycle)",
                    sql: "CREATE TABLE IF NOT EXISTS memory_docs(doc_id TEXT PRIMARY KEY, level TEXT, body TEXT, created_ms INTEGER);
             CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(doc_id, body);
             CREATE TABLE IF NOT EXISTS memory_lifecycle(doc_id TEXT PRIMARY KEY, veracity REAL, superseded_by TEXT, trust REAL);",
                },
                crate::migrate::Migration {
                    version: 2,
                    name: "ws5 validity windows + usage tracking",
                    sql: "ALTER TABLE memory_lifecycle ADD COLUMN valid_from_seq INTEGER;
             ALTER TABLE memory_lifecycle ADD COLUMN valid_to_seq INTEGER;
             ALTER TABLE memory_lifecycle ADD COLUMN supersedes_id TEXT;
             ALTER TABLE memory_docs ADD COLUMN usage_count INTEGER NOT NULL DEFAULT 0;
             ALTER TABLE memory_docs ADD COLUMN last_used_ms INTEGER NOT NULL DEFAULT 0;",
                },
            ],
            backup.as_deref(),
        )?;
        Ok(())
    }

    /// Write-path deny-list (Claude Code's rule): refuse anything derivable
    /// from the repo, git history, the ledger, or receipts. Deltas belong in
    /// receipts; raw hashes resolve via anchors; oversize bodies must arrive
    /// as anchored summaries. Heuristic and deterministic by design.
    fn deny_listed(body: &str) -> Option<String> {
        if body.trim().is_empty() {
            return Some("empty body".into());
        }
        if body.len() > super::summary::L1_BODY_MAX_BYTES {
            return Some(format!(
                "body exceeds 8KB; store a ≤{}-token anchored summary instead",
                super::summary::SUMMARY_TOKEN_BUDGET
            ));
        }
        let is_hex40 = |w: &str| w.len() == 40 && w.chars().all(|c| c.is_ascii_hexdigit());
        if body.split_whitespace().any(is_hex40) {
            return Some(
                "raw 40-hex commit hash; reference (path, symbol, short-sha) anchors instead"
                    .into(),
            );
        }
        if body.contains("\n+++ ") || body.contains("\n--- ") || body.contains("\n@@ ") {
            return Some("unified diff content belongs in receipts, not memory".into());
        }
        None
    }

    /// Lifecycle upsert (validated). Retrieval excludes dead rows by default.
    pub fn set_lifecycle(&self, row: &super::LifecycleRow) -> Result<(), MemoryError> {
        row.validate().map_err(MemoryError::Denied)?;
        self.conn.execute(
            "INSERT INTO memory_lifecycle(doc_id, veracity, superseded_by, trust, valid_from_seq, valid_to_seq, supersedes_id)
             VALUES(?,?,?,?,?,?,?)
             ON CONFLICT(doc_id) DO UPDATE SET veracity=excluded.veracity, superseded_by=excluded.superseded_by,
               trust=excluded.trust, valid_from_seq=excluded.valid_from_seq, valid_to_seq=excluded.valid_to_seq,
               supersedes_id=excluded.supersedes_id",
            params![
                row.doc_id,
                row.veracity,
                row.superseded_by,
                row.trust,
                row.valid_from_seq,
                row.valid_to_seq,
                row.supersedes_id
            ],
        )?;
        Ok(())
    }

    pub fn get_lifecycle(&self, doc_id: &str) -> Result<Option<super::LifecycleRow>, MemoryError> {
        let mut stmt = self.conn.prepare(
            "SELECT doc_id, veracity, superseded_by, trust, valid_from_seq, valid_to_seq, supersedes_id
             FROM memory_lifecycle WHERE doc_id = ?",
        )?;
        let mut rows = stmt.query_map(params![doc_id], |r| {
            Ok(super::LifecycleRow {
                doc_id: r.get(0)?,
                veracity: r.get(1)?,
                superseded_by: r.get(2)?,
                trust: r.get(3)?,
                valid_from_seq: r.get(4)?,
                valid_to_seq: r.get(5)?,
                supersedes_id: r.get(6)?,
            })
        })?;
        match rows.next() {
            None => Ok(None),
            Some(r) => r.map(Some).map_err(|e| e.into()),
        }
    }

    /// Record a retrieval hit (feeds `usage_count` pruning). Callers run
    /// consolidation serialized and off the write path.
    pub fn record_use(&self, doc_id: &str, now_ms: i64) -> Result<(), MemoryError> {
        self.conn.execute(
            "UPDATE memory_docs SET usage_count = usage_count + 1, last_used_ms = ? WHERE doc_id = ?",
            params![now_ms, doc_id],
        )?;
        Ok(())
    }

    /// FTS search with lifecycle filtering: docs with a dead lifecycle row
    /// (superseded, expired, or low-veracity) are excluded by default.
    /// Docs with no lifecycle row count as live (unreviewed).
    pub fn search_live(
        &self,
        query: &str,
        limit: usize,
        at_seq: i64,
    ) -> Result<Vec<(String, String)>, MemoryError> {
        let hits = self.search_fts(query, limit * 3)?;
        let mut out = vec![];
        for (id, body) in hits {
            let live = match self.get_lifecycle(&id)? {
                None => true,
                Some(row) => row.live_at(at_seq),
            };
            if live {
                out.push((id, body));
            }
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }

    /// Serialized consolidation helper: delete never-used docs older than the
    /// cutoff. Returns removed count. Run off the write path, never concurrent.
    pub fn prune_stale(&self, max_unused_days: i64, now_ms: i64) -> Result<i64, MemoryError> {
        let cutoff = now_ms - max_unused_days * 86_400_000;
        let victims: Vec<String> = self
            .conn
            .prepare("SELECT doc_id FROM memory_docs WHERE usage_count = 0 AND created_ms < ?")?
            .query_map(params![cutoff], |r| r.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let n = victims.len() as i64;
        for id in victims {
            self.conn
                .execute("DELETE FROM memory_docs WHERE doc_id = ?", params![id])?;
            self.conn
                .execute("DELETE FROM memory_fts WHERE doc_id = ?", params![id])?;
            self.conn
                .execute("DELETE FROM memory_lifecycle WHERE doc_id = ?", params![id])?;
        }
        Ok(n)
    }

    pub fn put(
        &self,
        doc_id: &str,
        level: &str,
        body: &str,
        now_ms: i64,
    ) -> Result<(), MemoryError> {
        if let Some(reason) = Self::deny_listed(body) {
            return Err(MemoryError::Denied(reason));
        }
        self.conn.execute_batch("BEGIN IMMEDIATE;")?;
        let r = (|| {
            self.conn.execute(
                "INSERT INTO memory_docs(doc_id, level, body, created_ms) VALUES(?,?,?,?)
                 ON CONFLICT(doc_id) DO UPDATE SET level=excluded.level, body=excluded.body",
                params![doc_id, level, body, now_ms],
            )?;
            self.conn.execute(
                "INSERT INTO memory_fts(doc_id, body) VALUES(?,?)",
                params![doc_id, body],
            )?;
            Ok::<_, rusqlite::Error>(())
        })();
        match r {
            Ok(_) => {
                self.conn.execute_batch("COMMIT;")?;
                Ok(())
            }
            Err(e) => {
                let _ = self.conn.execute_batch("ROLLBACK;");
                Err(e.into())
            }
        }
    }

    /// L1 FTS5 search (the AST/FTS5 "triad" read path).
    pub fn search_fts(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<(String, String)>, MemoryError> {
        let mut stmt = self
            .conn
            .prepare("SELECT doc_id, body FROM memory_fts WHERE memory_fts MATCH ? LIMIT ?")?;
        let rows = stmt.query_map(params![query, limit as i64], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.into())
    }

    pub fn total_bytes(&self) -> Result<i64, MemoryError> {
        let v: Option<i64> =
            self.conn
                .query_row("SELECT SUM(LENGTH(body)) FROM memory_docs", [], |r| {
                    r.get(0)
                })?;
        Ok(v.unwrap_or(0))
    }

    pub fn doc_count(&self) -> Result<i64, MemoryError> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM memory_docs", [], |r| r.get(0))?)
    }

    /// Bounded reaper: delete oldest docs until within both caps. Returns
    /// (removed, bytes_before, bytes_after) with real counts.
    pub fn reap(&self, max_bytes: i64, max_docs: i64) -> Result<(i64, i64, i64), MemoryError> {
        let before = self.total_bytes()?;
        let mut removed: i64 = 0;
        loop {
            if self.doc_count()? <= max_docs && self.total_bytes()? <= max_bytes {
                break;
            }
            let victim: Option<String> = self
                .conn
                .query_row(
                    "SELECT doc_id FROM memory_docs ORDER BY created_ms ASC LIMIT 1",
                    [],
                    |r| r.get(0),
                )
                .ok();
            match victim {
                None => break,
                Some(id) => {
                    self.conn
                        .execute("DELETE FROM memory_docs WHERE doc_id=?", params![id])?;
                    self.conn
                        .execute("DELETE FROM memory_fts WHERE doc_id=?", params![id])?;
                    removed += 1;
                }
            }
        }
        Ok((removed, before, self.total_bytes()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fts_roundtrip() {
        let m = MemoryStore::open_in_memory().unwrap();
        m.put("d1", "L1", "worktree trash rename deferred teardown", 1)
            .unwrap();
        let hits = m.search_fts("trash", 10).unwrap();
        assert!(hits.iter().any(|(id, _)| id == "d1"));
    }

    #[test]
    fn deny_list_refuses_derivable_content() {
        let m = MemoryStore::open_in_memory().unwrap();
        assert!(m.put("e", "L1", "   ", 1).is_err());
        assert!(m
            .put(
                "h",
                "L1",
                "see commit a1b2c3d4e5f60718293a4b5c6d7e8f9012345678 for details",
                1
            )
            .is_err());
        assert!(m
            .put("d", "L1", "change:\n+++ b/a.rs\n@@ -1 +1 @@\n+x", 1)
            .is_err());
        assert!(m.put("b", "L1", &"y".repeat(9000), 1).is_err());
        assert!(m
            .put(
                "ok",
                "L1",
                "checkout writes 0600 token files with short TTL",
                1
            )
            .is_ok());
    }

    #[test]
    fn search_live_excludes_superseded() {
        use super::super::LifecycleRow;
        let m = MemoryStore::open_in_memory().unwrap();
        m.put("old", "L1", "checkout writes token files", 1)
            .unwrap();
        m.put("new", "L1", "checkout writes token files with expiry", 2)
            .unwrap();
        // Before supersession both are visible.
        assert_eq!(m.search_live("checkout", 10, 0).unwrap().len(), 2);
        m.set_lifecycle(&LifecycleRow {
            doc_id: "old".into(),
            veracity: 1.0,
            superseded_by: Some("new".into()),
            trust: 1.0,
            valid_from_seq: None,
            valid_to_seq: Some(5),
            supersedes_id: None,
        })
        .unwrap();
        let live = m.search_live("checkout", 10, 9).unwrap();
        assert!(live.iter().all(|(id, _)| id != "old"));
        assert!(live.iter().any(|(id, _)| id == "new"));
    }

    #[test]
    fn usage_and_stale_pruning() {
        let m = MemoryStore::open_in_memory().unwrap();
        m.put("hot", "L1", "used doc", 100).unwrap();
        m.put("cold", "L1", "unused doc", 100).unwrap();
        m.record_use("hot", 200).unwrap();
        // 10 days later, 7-day cutoff: cold (never used) pruned, hot kept.
        assert_eq!(m.prune_stale(7, 100 + 10 * 86_400_000).unwrap(), 1);
        assert_eq!(m.doc_count().unwrap(), 1);
    }

    /// H-A proxy (retrieval-hit, honestly labeled): 20 tasks, each answerable
    /// by one anchored summary and one diluted raw trajectory. Anchored must
    /// win on hit@1 and clear an absolute bar. This measures the selection
    /// policy, not end-task success (that needs a live model loop).
    #[test]
    fn anchored_summaries_beat_raw_trajectories() {
        let anchored = MemoryStore::open_in_memory().unwrap();
        let raw = MemoryStore::open_in_memory().unwrap();
        let mut queries = vec![];
        for i in 0..20 {
            let sym = format!("zzsym{i:02}");
            let fix = format!("zzfix{i:02}");
            anchored
                .put(&format!("a{i}"), "L1", &format!("{sym} {fix} resolved"), i)
                .unwrap();
            // Raw trajectory: same answer terms diluted with every other task.
            let mut noise = vec![format!("{sym} {fix} resolved with logs")];
            for j in 0..20 {
                if j != i {
                    noise.push(format!("tried zzsym{j:02} saw zzfix{j:02} output dump"));
                }
            }
            // Keep under the 8KB write cap while staying noisy.
            let mut body = noise.join(" ");
            body.truncate(7000);
            raw.put(&format!("r{i}"), "L1", &body, i).unwrap();
            queries.push((format!("{sym} {fix}"), format!("a{i}"), format!("r{i}")));
        }
        let mut a_hits = 0;
        let mut r_hits = 0;
        for (q, want_a, want_r) in &queries {
            if anchored.search_fts(q, 1).unwrap().first().map(|(id, _)| id) == Some(want_a) {
                a_hits += 1;
            }
            if raw.search_fts(q, 1).unwrap().first().map(|(id, _)| id) == Some(want_r) {
                r_hits += 1;
            }
        }
        assert!(a_hits >= 15, "anchored hit@1 too low: {a_hits}/20");
        assert!(
            a_hits >= r_hits,
            "anchored ({a_hits}) must beat raw ({r_hits}) on hit@1"
        );
    }
}
