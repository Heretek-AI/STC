//! Event-sourced SQLite state store: WAL, BEGIN IMMEDIATE, append-only ledger.
//! Anchor (by symbol): agent-orchestrator `cdc/event.go` + `poller.go` (`change_log`, `DefaultPollInterval`),
//! munder-difflin `costLifetime.ts` (cumulative-counter reset hid 59% — hence ledger, not counter).

use rusqlite::{params, Connection};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StateError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

pub struct StateStore {
    conn: Connection,
}

impl StateStore {
    pub fn open_in_memory() -> Result<Self, StateError> {
        let conn = Connection::open_in_memory()?;
        let s = Self { conn };
        s.migrate()?;
        Ok(s)
    }

    pub fn open(path: &str) -> Result<Self, StateError> {
        let conn = Connection::open(path)?;
        let s = Self { conn };
        s.migrate()?;
        Ok(s)
    }

    fn migrate(&self) -> Result<(), StateError> {
        self.conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS tasks(id TEXT PRIMARY KEY, parent_id TEXT, kind TEXT, intent_hash TEXT, scope_hash TEXT, status TEXT, depends_on TEXT);
             CREATE TABLE IF NOT EXISTS claims(scope_hash TEXT PRIMARY KEY, owner TEXT);
             CREATE TABLE IF NOT EXISTS events(seq INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT, payload TEXT);
             CREATE TABLE IF NOT EXISTS receipts(id TEXT PRIMARY KEY, task_id TEXT, kind TEXT, evidence TEXT);
             CREATE TABLE IF NOT EXISTS token_ledger(seq INTEGER PRIMARY KEY AUTOINCREMENT, session TEXT, amount INTEGER, cache_hit INTEGER);
             CREATE TABLE IF NOT EXISTS merges(id TEXT PRIMARY KEY, status TEXT, detail TEXT);
             CREATE TABLE IF NOT EXISTS change_log(seq INTEGER PRIMARY KEY AUTOINCREMENT, tbl TEXT, row TEXT, op TEXT, old TEXT, new TEXT);
             CREATE TRIGGER IF NOT EXISTS trg_tasks_log AFTER INSERT ON tasks BEGIN INSERT INTO change_log(tbl,row,op,new) VALUES('tasks',NEW.id,'insert',NEW.status); END;
             CREATE TRIGGER IF NOT EXISTS trg_tasks_upd AFTER UPDATE OF status ON tasks BEGIN INSERT INTO change_log(tbl,row,op,old,new) VALUES('tasks',NEW.id,'update',OLD.status,NEW.status); END;",
        )?;
        Ok(())
    }

    /// Upsert a task (Manager scope -> DAG commit path). Idempotent on id.
    pub fn upsert_task(&self, id: &str, kind: &str, status: &str) -> Result<(), StateError> {
        self.conn.execute_batch("BEGIN IMMEDIATE;")?;
        let r = self.conn.execute(
            "INSERT INTO tasks(id,parent_id,kind,intent_hash,scope_hash,status,depends_on) VALUES(?,NULL,?,NULL,NULL,?,NULL)
             ON CONFLICT(id) DO UPDATE SET kind=excluded.kind, status=excluded.status",
            params![id, kind, status],
        );
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

    pub fn append_event(&self, kind: &str, payload: &str) -> Result<(), StateError> {
        self.conn.execute(
            "INSERT INTO events(kind,payload) VALUES(?,?)",
            params![kind, payload],
        )?;
        Ok(())
    }

    pub fn append_receipt(
        &self,
        id: &str,
        task_id: &str,
        kind: &str,
        evidence: &str,
    ) -> Result<(), StateError> {
        self.conn.execute(
            "INSERT INTO receipts(id,task_id,kind,evidence) VALUES(?,?,?,?)
             ON CONFLICT(id) DO UPDATE SET kind=excluded.kind, evidence=excluded.evidence",
            params![id, task_id, kind, evidence],
        )?;
        Ok(())
    }

    /// Read-only projection snapshot. UI calls this every 100ms; never caches canonically.
    pub fn snapshot(&self) -> Result<crate::projection::Snapshot, StateError> {
        use crate::projection::{lane_for, BurnRow, EventRow, ReceiptRow, TaskRow};
        let t0 = std::time::SystemTime::now();
        let mut stmt = self
            .conn
            .prepare("SELECT id, kind, status FROM tasks ORDER BY id")?;
        let fleet: Vec<TaskRow> = stmt
            .query_map([], |r| {
                let status: String = r.get(2)?;
                Ok(TaskRow {
                    id: r.get(0)?,
                    kind: r.get(1)?,
                    lane: lane_for(&status).into(),
                    status,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut stmt = self
            .conn
            .prepare("SELECT seq, kind, payload FROM events ORDER BY seq DESC LIMIT 100")?;
        let stream: Vec<EventRow> = stmt
            .query_map([], |r| {
                Ok(EventRow {
                    seq: r.get(0)?,
                    kind: r.get(1)?,
                    payload: r.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut stmt = self
            .conn
            .prepare("SELECT id, task_id, kind, evidence FROM receipts ORDER BY id")?;
        let receipts: Vec<ReceiptRow> = stmt
            .query_map([], |r| {
                Ok(ReceiptRow {
                    id: r.get(0)?,
                    task_id: r.get(1)?,
                    kind: r.get(2)?,
                    evidence: r.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut stmt = self.conn.prepare(
            "SELECT session, SUM(amount) FROM token_ledger WHERE cache_hit=0 GROUP BY session ORDER BY 2 DESC",
        )?;
        let burn: Vec<BurnRow> = stmt
            .query_map([], |r| {
                Ok(BurnRow {
                    session: r.get(0)?,
                    total: r.get(1)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let change_seq: Option<i64> = self
            .conn
            .query_row("SELECT MAX(seq) FROM change_log", [], |r| r.get(0))
            .unwrap_or(None);
        let age_ms = t0.elapsed().map(|d| d.as_millis() as u64).unwrap_or(0);
        Ok(crate::projection::Snapshot {
            db_age_ms: age_ms,
            source: "studio.db".into(),
            fleet,
            stream,
            receipts,
            burn,
            change_seq: change_seq.unwrap_or(0),
        })
    }

    /// Append-only token ledger. Never update cumulative counters; cache hits tracked separately.
    pub fn record_tokens(
        &self,
        session: &str,
        amount: i64,
        cache_hit: bool,
    ) -> Result<(), StateError> {
        self.conn.execute_batch("BEGIN IMMEDIATE;")?;
        let r = self.conn.execute(
            "INSERT INTO token_ledger(session, amount, cache_hit) VALUES(?,?,?)",
            params![session, amount, cache_hit as i32],
        );
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

    pub fn lifetime_spend(&self) -> Result<i64, StateError> {
        let v: Option<i64> = self.conn.query_row(
            "SELECT SUM(amount) FROM token_ledger WHERE cache_hit=0",
            [],
            |r| r.get(0),
        )?;
        Ok(v.unwrap_or(0))
    }

    pub fn poll_changes(
        &self,
        from_seq: i64,
        batch: usize,
    ) -> Result<Vec<(i64, String, String)>, StateError> {
        let mut stmt = self
            .conn
            .prepare("SELECT seq, tbl, op FROM change_log WHERE seq > ? ORDER BY seq LIMIT ?")?;
        let rows = stmt.query_map(params![from_seq, batch as i64], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_never_resets() {
        let s = StateStore::open_in_memory().unwrap();
        s.record_tokens("a", 100, false).unwrap();
        s.record_tokens("a", 50, true).unwrap(); // cache hit excluded from budget
        assert_eq!(s.lifetime_spend().unwrap(), 100);
        s.record_tokens("a", 25, false).unwrap();
        assert_eq!(s.lifetime_spend().unwrap(), 125);
    }
}
