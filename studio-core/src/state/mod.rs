//! Event-sourced SQLite state store: WAL, BEGIN IMMEDIATE, append-only ledger.
//! Anchor (by symbol): agent-orchestrator `cdc/event.go` + `poller.go` (`change_log`, `DefaultPollInterval`),
//! munder-difflin `costLifetime.ts` (cumulative-counter reset hid 59% — hence ledger, not counter).

use rusqlite::{params, Connection};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StateError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("migration: {0}")]
    Migrate(#[from] crate::migrate::MigrateError),
    #[error("io: {0}")]
    Io(String),
}

/// Current schema version. v1 = Phase 0–5 baseline (incl. billed_kind,
/// change_log triggers); v2 = WS4 indexes; v3 = #8 project autonomy table.
pub const SCHEMA_VERSION: i64 = 3;

pub struct StateStore {
    conn: Connection,
}

impl StateStore {
    pub fn open_in_memory() -> Result<Self, StateError> {
        let conn = Connection::open_in_memory()?;
        let s = Self { conn };
        s.migrate(None)?;
        Ok(s)
    }

    pub fn open(path: &str) -> Result<Self, StateError> {
        let conn = Connection::open(path)?;
        let s = Self { conn };
        s.migrate(Self::backup_sibling(path))?;
        Ok(s)
    }

    /// `<db>.bak.<millis>` sibling for pre-migration snapshots.
    /// Public so sibling stores (memory) share one backup convention.
    pub fn backup_sibling_for(path: &str) -> Option<std::path::PathBuf> {
        Self::backup_sibling(path)
    }

    fn backup_sibling(path: &str) -> Option<std::path::PathBuf> {
        let p = std::path::Path::new(path);
        let name = p.file_name()?.to_string_lossy().to_string();
        let ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        Some(p.with_file_name(format!("{name}.bak.{ms}")))
    }

    pub fn schema_version(&self) -> Result<i64, StateError> {
        Ok(crate::migrate::current_version(&self.conn)?)
    }

    /// Online snapshot: `VACUUM INTO dest` (works on a live WAL database).
    pub fn backup_to(&self, dest: &std::path::Path) -> Result<(), StateError> {
        if let Some(p) = dest.parent() {
            std::fs::create_dir_all(p).map_err(|e| StateError::Io(e.to_string()))?;
        }
        let sql = format!(
            "VACUUM INTO '{}'",
            dest.to_string_lossy().replace('\'', "''")
        );
        self.conn.execute_batch(&sql)?;
        Ok(())
    }

    /// All receipts for export (evidence JSON included verbatim).
    pub fn export_receipts(&self) -> Result<Vec<crate::projection::ReceiptRow>, StateError> {
        Ok(self.snapshot()?.receipts)
    }

    fn migrate(&self, backup: Option<std::path::PathBuf>) -> Result<(), StateError> {
        self.migrate_legacy_repair()?;
        crate::migrate::run_migrations(
            &self.conn,
            SCHEMA_VERSION,
            &Self::steps(),
            backup.as_deref(),
        )?;
        Ok(())
    }

    fn steps() -> Vec<crate::migrate::Migration> {
        use crate::migrate::Migration;
        vec![
            Migration {
                version: 1,
                name: "phase0-5 baseline",
                sql: "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS tasks(id TEXT PRIMARY KEY, parent_id TEXT, kind TEXT, intent_hash TEXT, scope_hash TEXT, status TEXT, depends_on TEXT);
             CREATE TABLE IF NOT EXISTS claims(scope_hash TEXT PRIMARY KEY, owner TEXT);
             CREATE TABLE IF NOT EXISTS events(seq INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT, payload TEXT);
             CREATE TABLE IF NOT EXISTS receipts(id TEXT PRIMARY KEY, task_id TEXT, kind TEXT, evidence TEXT);
             CREATE TABLE IF NOT EXISTS token_ledger(seq INTEGER PRIMARY KEY AUTOINCREMENT, session TEXT, amount INTEGER, cache_hit INTEGER, billed_kind TEXT NOT NULL DEFAULT 'api_key');
             CREATE TABLE IF NOT EXISTS merges(id TEXT PRIMARY KEY, status TEXT, detail TEXT);
             CREATE TABLE IF NOT EXISTS change_log(seq INTEGER PRIMARY KEY AUTOINCREMENT, tbl TEXT, row TEXT, op TEXT, old TEXT, new TEXT);
             CREATE TRIGGER IF NOT EXISTS trg_tasks_log AFTER INSERT ON tasks BEGIN INSERT INTO change_log(tbl,row,op,new) VALUES('tasks',NEW.id,'insert',NEW.status); END;
             CREATE TRIGGER IF NOT EXISTS trg_tasks_upd AFTER UPDATE OF status ON tasks BEGIN INSERT INTO change_log(tbl,row,op,old,new) VALUES('tasks',NEW.id,'update',OLD.status,NEW.status); END;",
            },
            Migration {
                version: 2,
                name: "ws4 scoreboard/poll indexes",
                sql: "CREATE INDEX IF NOT EXISTS idx_token_ledger_session ON token_ledger(session);
             CREATE INDEX IF NOT EXISTS idx_receipts_task ON receipts(task_id);
             CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);",
            },
            Migration {
                version: 3,
                name: "issue-8 project autonomy dial",
                sql: "CREATE TABLE IF NOT EXISTS project_autonomy(repo TEXT PRIMARY KEY, mode TEXT NOT NULL DEFAULT 'full');",
            },
        ]
    }

    /// Repair for pre-Phase-6 databases created before billed_kind existed.
    /// Tolerant: fresh databases have no tables yet when this runs.
    fn migrate_legacy_repair(&self) -> Result<(), StateError> {
        let has_table: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='token_ledger'",
            [],
            |r| r.get(0),
        )?;
        if has_table == 0 {
            return Ok(());
        }
        let has_billed: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('token_ledger') WHERE name='billed_kind'",
            [],
            |r| r.get(0),
        )?;
        if has_billed == 0 {
            self.conn.execute_batch(
                "ALTER TABLE token_ledger ADD COLUMN billed_kind TEXT NOT NULL DEFAULT 'api_key';",
            )?;
        }
        Ok(())
    }

    /// Persist the per-project autonomy mode (issue #8). Unknown mode strings
    /// are rejected; reads fail closed to advisory on corrupt values.
    pub fn set_autonomy(
        &self,
        repo: &str,
        mode: crate::scheduler::autonomy::Autonomy,
    ) -> Result<(), StateError> {
        self.conn.execute(
            "INSERT INTO project_autonomy(repo,mode) VALUES(?,?)
             ON CONFLICT(repo) DO UPDATE SET mode=excluded.mode",
            params![repo, mode.as_str()],
        )?;
        Ok(())
    }

    /// Read the per-project autonomy mode. Missing rows default to full
    /// authority (grill decision); corrupt values fail closed to advisory.
    pub fn get_autonomy(
        &self,
        repo: &str,
    ) -> Result<crate::scheduler::autonomy::Autonomy, StateError> {
        let mode: Option<String> = self
            .conn
            .query_row(
                "SELECT mode FROM project_autonomy WHERE repo=?",
                params![repo],
                |r| r.get(0),
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })?;
        Ok(match mode {
            None => crate::scheduler::autonomy::Autonomy::default_for_new_project(),
            Some(m) => crate::scheduler::autonomy::Autonomy::parse(&m),
        })
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
        let autonomy_mode = self
            .get_autonomy(&crate::scheduler::autonomy::repo_key_from_env())
            .map(|a| a.as_str().to_string())
            .unwrap_or_else(|_| "advisory".into());
        Ok(crate::projection::Snapshot {
            db_age_ms: age_ms,
            source: "studio.db".into(),
            fleet,
            stream,
            receipts,
            burn,
            change_seq: change_seq.unwrap_or(0),
            runtime_mode: crate::projection::runtime_mode_from_env(),
            autonomy_mode,
        })
    }

    /// Append-only token ledger. Never update cumulative counters; cache hits tracked separately.
    /// `billed_kind` records how the spend was billed (e.g. `subscription`,
    /// `per_token`) and is allowed to flip subscription→overage across rows.
    pub fn record_tokens_billed(
        &self,
        session: &str,
        amount: i64,
        cache_hit: bool,
        billed_kind: &str,
    ) -> Result<(), StateError> {
        self.conn.execute_batch("BEGIN IMMEDIATE;")?;
        let r = self.conn.execute(
            "INSERT INTO token_ledger(session, amount, cache_hit, billed_kind) VALUES(?,?,?,?)",
            params![session, amount, cache_hit as i32, billed_kind],
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

    pub fn record_tokens(
        &self,
        session: &str,
        amount: i64,
        cache_hit: bool,
    ) -> Result<(), StateError> {
        self.record_tokens_billed(session, amount, cache_hit, "api_key")
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

    #[test]
    fn legacy_db_upgrades_with_backup() {
        // Simulate a pre-Phase-6 database: old schema, no billed_kind, version 0.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("studio.db");
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE token_ledger(seq INTEGER PRIMARY KEY AUTOINCREMENT, session TEXT, amount INTEGER, cache_hit INTEGER);",
            )
            .unwrap();
        }
        let before: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        assert!(!before.iter().any(|n| n.contains(".bak.")));
        let s = StateStore::open(path.to_str().unwrap()).unwrap();
        assert_eq!(s.schema_version().unwrap(), SCHEMA_VERSION);
        // Billed writes work post-upgrade (repair + migration converged).
        s.record_tokens_billed("a", 10, false, "subscription")
            .unwrap();
        // A pre-migration backup snapshot exists.
        let after: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        assert!(
            after.iter().any(|n| n.contains(".bak.")),
            "VACUUM INTO backup missing: {after:?}"
        );
    }

    #[test]
    fn refuses_newer_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("studio.db");
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch("PRAGMA user_version = 999").unwrap();
        }
        assert!(StateStore::open(path.to_str().unwrap()).is_err());
    }

    #[test]
    fn autonomy_defaults_full_persists_per_repo() {
        use crate::scheduler::autonomy::Autonomy;
        let s = StateStore::open_in_memory().unwrap();
        assert_eq!(s.schema_version().unwrap(), SCHEMA_VERSION);
        // Missing row: full authority (grill default).
        assert_eq!(s.get_autonomy("/repo/a").unwrap(), Autonomy::Full);
        s.set_autonomy("/repo/a", Autonomy::Advisory).unwrap();
        assert_eq!(s.get_autonomy("/repo/a").unwrap(), Autonomy::Advisory);
        // Per-repo isolation.
        assert_eq!(s.get_autonomy("/repo/b").unwrap(), Autonomy::Full);
        s.set_autonomy("/repo/b", Autonomy::Full).unwrap();
        assert_eq!(s.get_autonomy("/repo/a").unwrap(), Autonomy::Advisory);
        // Snapshot carries the STUDIO_REPO mode.
        unsafe {
            std::env::set_var("STUDIO_REPO", "/repo/a");
        }
        assert_eq!(s.snapshot().unwrap().autonomy_mode, "advisory");
        unsafe {
            std::env::remove_var("STUDIO_REPO");
        }
    }

    #[test]
    fn autonomy_corrupt_value_fails_closed() {
        use crate::scheduler::autonomy::Autonomy;
        let s = StateStore::open_in_memory().unwrap();
        s.conn
            .execute(
                "INSERT INTO project_autonomy(repo,mode) VALUES('r','root-equivalent')",
                [],
            )
            .unwrap();
        assert_eq!(s.get_autonomy("r").unwrap(), Autonomy::Advisory);
    }
}
