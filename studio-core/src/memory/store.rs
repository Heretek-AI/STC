//! L1 facts + FTS5 store (lives in the same `studio.db` connection family).

use rusqlite::{params, Connection};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MemoryError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io: {0}")]
    Io(String),
}

pub struct MemoryStore {
    conn: Connection,
}

impl MemoryStore {
    pub fn open_in_memory() -> Result<Self, MemoryError> {
        let conn = Connection::open_in_memory()?;
        let s = Self { conn };
        s.migrate()?;
        Ok(s)
    }

    fn migrate(&self) -> Result<(), MemoryError> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS memory_docs(doc_id TEXT PRIMARY KEY, level TEXT, body TEXT, created_ms INTEGER);
             CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(doc_id, body);
             CREATE TABLE IF NOT EXISTS memory_lifecycle(doc_id TEXT PRIMARY KEY, veracity REAL, superseded_by TEXT, trust REAL);",
        )?;
        Ok(())
    }

    pub fn put(
        &self,
        doc_id: &str,
        level: &str,
        body: &str,
        now_ms: i64,
    ) -> Result<(), MemoryError> {
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
}
