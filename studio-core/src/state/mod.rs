//! Greenfield SQLite **v2** state store: the only source of truth for STC v2.
//!
//! This is a v2 rewrite, not a v1 port: v1 shipped a schema-migration stack
//! (`migrate.rs`, `SCHEMA_VERSION = 3`, legacy `billed_kind` repair). The v2
//! brief is explicit — *greenfield v2 schema, NO v1 migration* — so the schema
//! here is created idempotently and any database that is not exactly v2 is
//! refused (fail closed, typed reason).
//!
//! Crash-safety design (evidence hashes in `.roadmap/01-engine-skeleton/dossier.json`):
//! - **Atomicity.** WAL, `synchronous = NORMAL`. A crash mid-transaction rolls
//!   the partial transaction back on the next open (VERIFIED `sqlite.org/howtocorrupt.html`,
//!   sha256 `988dd43e...`).
//! - **Single writer.** Every write runs in `BEGIN IMMEDIATE` and all writes go
//!   through one connection behind a `Mutex`, so exactly one write transaction
//!   is ever open in-process; `BEGIN IMMEDIATE` extends that across processes
//!   (VERIFIED `sqlite.org/lang_transaction.html`, sha256 `0c3f296a...`).
//! - **Bounded WAL (honest).** An overlapping reader *can* starve checkpoints
//!   and grow the WAL without bound if a writer keeps writing (VERIFIED
//!   `sqlite.org/wal.html`, sha256 `b7a994ea...`). We therefore (a) retry the
//!   checkpoint as PASSIVE → RESTART → TRUNCATE and never discard the
//!   `(busy,log,checkpointed)` tuple, and (b) apply backpressure with
//!   [`StateStore::enforce_wal_bound`]: once the WAL exceeds the bound the
//!   writer stops making progress and either the checkpoint succeeds or the
//!   writer fails closed with a typed `WalBackpressure` error. We make no claim
//!   that the WAL "cannot grow".
//!
//! Discovery note (POSIX advisory locks): `close()` cancels POSIX advisory
//! locks for *all* file descriptors in the process (VERIFIED `988dd43e...`).
//! We therefore never rely on a user-space advisory lock to guard worktree
//! mutations while closing a second fd for the same file; worktree mutation
//! serialization is an in-process async mutex (`worktree::WorktreeOperationLock`),
//! and the SQLite lock lives only on the one long-lived writer connection.

use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;
use thiserror::Error;

/// v2 schema version. Bumping this requires a real migration step; the current
/// engine refuses anything that is not exactly this version.
pub const SCHEMA_VERSION: i64 = 2;

/// Greenfield v2 schema. Created idempotently (`IF NOT EXISTS`). No migration.
const SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS schema_meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
INSERT OR IGNORE INTO schema_meta(key, value) VALUES('schema_version', '2');

CREATE TABLE IF NOT EXISTS tasks(
    id         TEXT PRIMARY KEY,
    kind       TEXT NOT NULL,
    status     TEXT NOT NULL,
    updated_ms INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);

CREATE TABLE IF NOT EXISTS events(
    seq     INTEGER PRIMARY KEY AUTOINCREMENT,
    kind    TEXT NOT NULL,
    payload TEXT NOT NULL,
    ts_ms   INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS receipts(
    id       TEXT PRIMARY KEY,
    task_id  TEXT,
    kind     TEXT NOT NULL,
    evidence TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_receipts_task ON receipts(task_id);

CREATE TABLE IF NOT EXISTS token_ledger(
    seq        INTEGER PRIMARY KEY AUTOINCREMENT,
    session    TEXT NOT NULL,
    amount     INTEGER NOT NULL,
    cache_hit  INTEGER NOT NULL DEFAULT 0,
    billed_kind TEXT NOT NULL DEFAULT 'api_key',
    ts_ms      INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_token_ledger_session ON token_ledger(session);

CREATE TABLE IF NOT EXISTS worktrees(
    path       TEXT PRIMARY KEY,
    repo       TEXT NOT NULL,
    slug       TEXT NOT NULL,
    state      TEXT NOT NULL DEFAULT 'creating',
    created_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS change_log(
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    tbl TEXT NOT NULL,
    row TEXT NOT NULL,
    op  TEXT NOT NULL,
    old TEXT,
    new TEXT
);

CREATE TRIGGER IF NOT EXISTS trg_tasks_log AFTER INSERT ON tasks BEGIN
    INSERT INTO change_log(tbl,row,op,new) VALUES('tasks',NEW.id,'insert',NEW.status);
END;
CREATE TRIGGER IF NOT EXISTS trg_tasks_upd AFTER UPDATE OF status ON tasks BEGIN
    INSERT INTO change_log(tbl,row,op,old,new) VALUES('tasks',NEW.id,'update',OLD.status,NEW.status);
END;
";

#[derive(Debug, Error)]
pub enum StateError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io: {0}")]
    Io(String),
    #[error("refusing database with newer schema version {found} (engine supports {supported})")]
    NewerSchema { found: i64, supported: i64 },
    #[error("not a v2 studio database (schema_version={found})")]
    NotV2 { found: i64 },
    #[error("database not found: {0}")]
    NotFound(String),
    #[error("writer connection poisoned")]
    Poisoned,
    #[error(
        "WAL backpressure: {size} bytes exceeds bound {bound} and no checkpoint could reduce it"
    )]
    WalBackpressure { size: u64, bound: u64 },
}

/// Result of a WAL checkpoint attempt. The `(busy, log, checkpointed)` tuple is
/// **never discarded**: a `busy != 0` result means the checkpoint could not
/// complete (typically a concurrent reader pinning the WAL) and the caller must
/// apply backpressure rather than assume success.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CheckpointOutcome {
    /// 1 when a reader/other connection prevented the checkpoint.
    pub busy: i64,
    /// Frames in the WAL before the attempt.
    pub log: i64,
    /// Frames successfully checkpointed into the database.
    pub checkpointed: i64,
    /// True only when a `TRUNCATE` checkpoint completed and reset the WAL.
    pub truncated: bool,
}

/// One ledger worktree row, including the repo it was created for. Used by
/// `verify` to check containment, repo-match and on-disk kind (defect D).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeRecord {
    pub path: String,
    pub repo: String,
    pub slug: String,
    pub state: String,
}

/// Canonicalize a worktree path string to an absolute, lexical form for storage
/// and comparison.
fn canonical_path(p: &str) -> String {
    crate::worktree::normalize_path(Path::new(p))
        .to_string_lossy()
        .into_owned()
}

/// Defence-in-depth (C1): the database and its WAL/SHM sidecars are created
/// 0600, not the process umask's default. This does not stop a *same-user*
/// O_RDONLY reader (nothing local can — see the graceful-degradation path in
/// `studio daemon`), but it keeps the DB and sidecars off other local accounts.
#[cfg(unix)]
fn restrict_db_permissions(db_path: &str) {
    use std::os::unix::fs::PermissionsExt;
    let base = PathBuf::from(db_path);
    for candidate in [
        base.clone(),
        PathBuf::from(format!("{}-wal", base.to_string_lossy())),
        PathBuf::from(format!("{}-shm", base.to_string_lossy())),
    ] {
        if candidate.exists() {
            let _ = std::fs::set_permissions(&candidate, std::fs::Permissions::from_mode(0o600));
        }
    }
}

#[cfg(not(unix))]
fn restrict_db_permissions(_db_path: &str) {}

/// Durable state store. Cheap to clone by `Arc`; all writers serialize on one
/// connection.
#[derive(Debug)]
pub struct StateStore {
    conn: Mutex<Connection>,
    /// Path of the on-disk database (`None` for in-memory stores).
    db_path: Option<PathBuf>,
}

impl StateStore {
    /// Open (creating if absent) the v2 database at `path`, ensure WAL, and
    /// fail closed if the existing file is not exactly v2.
    pub fn open(path: &str) -> Result<Self, StateError> {
        if let Some(parent) = Path::new(path).parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| StateError::Io(e.to_string()))?;
            }
        }
        let conn = Connection::open(path)?;
        Self::configure(&conn, true)?;
        let store = Self {
            conn: Mutex::new(conn),
            db_path: Some(PathBuf::from(path)),
        };
        store.init_schema()?;
        store.check_version()?;
        restrict_db_permissions(path);
        Ok(store)
    }

    /// In-memory store for tests. Not WAL (SQLite has no WAL for `:memory:`).
    pub fn open_in_memory() -> Result<Self, StateError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        let store = Self {
            conn: Mutex::new(conn),
            db_path: None,
        };
        store.init_schema()?;
        store.check_version()?;
        Ok(store)
    }

    /// Read-only open for the CLI projection path. Never creates the DB and
    /// never runs DDL, so `studio status` works with no daemon running.
    pub fn open_readonly(path: &str) -> Result<Self, StateError> {
        if !Path::new(path).exists() {
            return Err(StateError::NotFound(path.to_string()));
        }
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY;
        let conn = Connection::open_with_flags(path, flags)?;
        conn.busy_timeout(std::time::Duration::from_millis(250))?;
        let store = Self {
            conn: Mutex::new(conn),
            db_path: Some(PathBuf::from(path)),
        };
        store.check_version()?;
        Ok(store)
    }

    fn configure(conn: &Connection, wal: bool) -> Result<(), StateError> {
        conn.busy_timeout(std::time::Duration::from_millis(5_000))?;
        conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA synchronous=NORMAL;")?;
        if wal {
            // journal_mode is a persistent property of the database file.
            let _mode: String = conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))?;
            conn.execute_batch("PRAGMA wal_autocheckpoint=256;")?;
        }
        Ok(())
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>, StateError> {
        self.conn.lock().map_err(|_| StateError::Poisoned)
    }

    fn init_schema(&self) -> Result<(), StateError> {
        let conn = self.lock()?;
        conn.execute_batch(SCHEMA_SQL)?;
        Ok(())
    }

    fn check_version(&self) -> Result<(), StateError> {
        let conn = self.lock()?;
        let has_meta: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='schema_meta'",
            [],
            |r| r.get(0),
        )?;
        if has_meta == 0 {
            return Err(StateError::NotV2 { found: 0 });
        }
        let value: Option<String> = conn
            .query_row(
                "SELECT value FROM schema_meta WHERE key='schema_version'",
                [],
                |r| r.get(0),
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })?;
        let found = value.and_then(|v| v.parse::<i64>().ok()).unwrap_or(-1);
        if found > SCHEMA_VERSION {
            return Err(StateError::NewerSchema {
                found,
                supported: SCHEMA_VERSION,
            });
        }
        if found != SCHEMA_VERSION {
            return Err(StateError::NotV2 { found });
        }
        Ok(())
    }

    pub fn schema_version(&self) -> Result<i64, StateError> {
        let conn = self.lock()?;
        let v: String = conn.query_row(
            "SELECT value FROM schema_meta WHERE key='schema_version'",
            [],
            |r| r.get(0),
        )?;
        Ok(v.parse().unwrap_or(-1))
    }

    /// Run `f` inside exactly one `BEGIN IMMEDIATE` transaction on the single
    /// writer connection. Any error rolls the transaction back and propagates.
    fn with_write<T>(
        &self,
        f: impl FnOnce(&Connection) -> Result<T, StateError>,
    ) -> Result<T, StateError> {
        let conn = self.lock()?;
        conn.execute_batch("BEGIN IMMEDIATE;")?;
        match f(&conn) {
            Ok(v) => {
                conn.execute_batch("COMMIT;")?;
                Ok(v)
            }
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK;");
                Err(e)
            }
        }
    }

    fn now_ms(&self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }

    /// Test-only probe: run a `with_write` transaction that blocks inside its
    /// closure *before* the first statement. The caller can then check whether
    /// the write lock is already held — which is exactly the difference between
    /// `BEGIN IMMEDIATE` and a mutated plain `BEGIN` (defect F).
    #[cfg(test)]
    pub(crate) fn with_write_lock_probe(
        &self,
        entered: std::sync::mpsc::Sender<()>,
        release: std::sync::mpsc::Receiver<()>,
    ) -> Result<(), StateError> {
        self.with_write(|conn| {
            let _ = entered.send(());
            let _ = release.recv();
            conn.execute(
                "INSERT INTO events(kind,payload,ts_ms) VALUES('probe','lock',0)",
                [],
            )?;
            Ok(())
        })
    }

    /// Upsert a task row. Idempotent on `id`.
    pub fn upsert_task(&self, id: &str, kind: &str, status: &str) -> Result<(), StateError> {
        let ts = self.now_ms();
        self.with_write(|conn| {
            conn.execute(
                "INSERT INTO tasks(id,kind,status,updated_ms) VALUES(?,?,?,?)
                 ON CONFLICT(id) DO UPDATE SET kind=excluded.kind, status=excluded.status, updated_ms=excluded.updated_ms",
                rusqlite::params![id, kind, status, ts],
            )?;
            Ok(())
        })
    }

    pub fn append_event(&self, kind: &str, payload: &str) -> Result<(), StateError> {
        let ts = self.now_ms();
        self.with_write(|conn| {
            conn.execute(
                "INSERT INTO events(kind,payload,ts_ms) VALUES(?,?,?)",
                rusqlite::params![kind, payload, ts],
            )?;
            Ok(())
        })
    }

    pub fn append_receipt(
        &self,
        id: &str,
        task_id: &str,
        kind: &str,
        evidence: &str,
    ) -> Result<(), StateError> {
        self.with_write(|conn| {
            conn.execute(
                "INSERT INTO receipts(id,task_id,kind,evidence) VALUES(?,?,?,?)
                 ON CONFLICT(id) DO UPDATE SET kind=excluded.kind, evidence=excluded.evidence",
                rusqlite::params![id, task_id, kind, evidence],
            )?;
            Ok(())
        })
    }

    /// Append-only token ledger. Cache hits are tracked, never folded into the
    /// billed total.
    pub fn record_tokens_billed(
        &self,
        session: &str,
        amount: i64,
        cache_hit: bool,
        billed_kind: &str,
    ) -> Result<(), StateError> {
        let ts = self.now_ms();
        self.with_write(|conn| {
            conn.execute(
                "INSERT INTO token_ledger(session,amount,cache_hit,billed_kind,ts_ms) VALUES(?,?,?,?,?)",
                rusqlite::params![session, amount, cache_hit as i32, billed_kind, ts],
            )?;
            Ok(())
        })
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
        let conn = self.lock()?;
        let v: Option<i64> = conn.query_row(
            "SELECT SUM(amount) FROM token_ledger WHERE cache_hit=0",
            [],
            |r| r.get(0),
        )?;
        Ok(v.unwrap_or(0))
    }

    /// Record the intent to create a worktree *before* touching git, so a crash
    /// mid-create leaves a recoverable `creating` row rather than an invisible
    /// directory. The path is canonicalized to an absolute, lexical form so the
    /// row can never disagree with git's absolute view of the same worktree.
    pub fn begin_worktree(&self, path: &str, repo: &str, slug: &str) -> Result<(), StateError> {
        let ts = self.now_ms();
        let canon = canonical_path(path);
        self.with_write(|conn| {
            conn.execute(
                "INSERT INTO worktrees(path,repo,slug,state,created_ms) VALUES(?,?,?,'creating',?)
                 ON CONFLICT(path) DO UPDATE SET repo=excluded.repo, slug=excluded.slug",
                rusqlite::params![canon, repo, slug, ts],
            )?;
            Ok(())
        })
    }

    pub fn mark_worktree_ready(&self, path: &str) -> Result<(), StateError> {
        let canon = canonical_path(path);
        self.with_write(|conn| {
            conn.execute(
                "UPDATE worktrees SET state='ready' WHERE path=?",
                rusqlite::params![canon],
            )?;
            Ok(())
        })
    }

    /// Delete by canonical path *or* by the given raw string, so both rows
    /// written by the canonicalizing writer and any legacy relative row are
    /// removable.
    pub fn delete_worktree(&self, path: &str) -> Result<(), StateError> {
        let canon = canonical_path(path);
        self.with_write(|conn| {
            conn.execute(
                "DELETE FROM worktrees WHERE path=?1 OR path=?2",
                rusqlite::params![canon, path],
            )?;
            Ok(())
        })
    }

    /// All worktree paths currently tracked, canonicalized for comparison.
    pub fn worktree_paths(&self) -> Result<Vec<String>, StateError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare("SELECT path FROM worktrees")?;
        let raw = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let mut set = std::collections::BTreeSet::new();
        for p in raw {
            set.insert(canonical_path(&p));
        }
        Ok(set.into_iter().collect())
    }

    /// Raw `(path, state)` rows exactly as stored (no canonicalization); used by
    /// recovery when it must delete a row by its stored key.
    pub fn worktree_rows(&self) -> Result<Vec<(String, String)>, StateError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare("SELECT path, state FROM worktrees")?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Full worktree rows, including the recorded `repo`, for `verify`'s
    /// containment / repo-match / kind checks (defect D).
    pub fn worktree_records(&self) -> Result<Vec<WorktreeRecord>, StateError> {
        let conn = self.lock()?;
        let mut stmt =
            conn.prepare("SELECT path, repo, slug, state FROM worktrees ORDER BY path")?;
        let rows = stmt
            .query_map([], |r| {
                Ok(WorktreeRecord {
                    path: r.get(0)?,
                    repo: r.get(1)?,
                    slug: r.get(2)?,
                    state: r.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Count of rows whose `state` is not one of the known-good values
    /// (`creating`, `ready`). Any other value is corruption and must make the
    /// integrity projection unclean.
    pub fn invalid_state_rows(&self) -> Result<usize, StateError> {
        let conn = self.lock()?;
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM worktrees WHERE state NOT IN ('creating','ready')",
            [],
            |r| r.get(0),
        )?;
        Ok(n as usize)
    }

    /// Paths recorded but never confirmed (`state='creating'`).
    pub fn worktrees_in_state(&self, state: &str) -> Result<Vec<String>, StateError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare("SELECT path FROM worktrees WHERE state=? ORDER BY path")?;
        let rows = stmt
            .query_map([state], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Force a WAL checkpoint, retrying PASSIVE → RESTART → TRUNCATE with
    /// backoff. Never discards the `(busy, log, checkpointed)` tuple: the
    /// returned [`CheckpointOutcome`] tells the caller whether truncation
    /// actually happened. Under a held reader every mode can return `busy`, so
    /// the caller must apply backpressure (see [`StateStore::enforce_wal_bound`]).
    pub fn checkpoint(&self) -> Result<CheckpointOutcome, StateError> {
        self.checkpoint_with_retry(4, Duration::from_millis(50))
    }

    /// Retry the three checkpoint modes `attempts` times with `backoff` between
    /// rounds. Returns the last outcome (never panics, never discards).
    pub fn checkpoint_with_retry(
        &self,
        attempts: u32,
        backoff: Duration,
    ) -> Result<CheckpointOutcome, StateError> {
        const MODES: [&str; 3] = ["PASSIVE", "RESTART", "TRUNCATE"];
        let rounds = attempts.max(1);
        let mut last = CheckpointOutcome::default();
        for round in 0..rounds {
            for mode in MODES {
                let out = self.checkpoint_once(mode)?;
                last = out;
                if out.truncated {
                    return Ok(out);
                }
            }
            if round + 1 < rounds {
                std::thread::sleep(backoff);
            }
        }
        Ok(last)
    }

    fn checkpoint_once(&self, mode: &str) -> Result<CheckpointOutcome, StateError> {
        let conn = self.lock()?;
        // Checkpoint attempts must fail *fast*: `RESTART`/`TRUNCATE` otherwise
        // block for the connection's full busy_timeout (5s) on every attempt,
        // the exact stall the receipt used to hide. Bound each attempt tightly;
        // backpressure (not a long block) handles a starving reader.
        conn.busy_timeout(Duration::from_millis(50))?;
        let result = conn.query_row(&format!("PRAGMA wal_checkpoint({mode})"), [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        });
        conn.busy_timeout(Duration::from_millis(5_000))?;
        let (busy, log, checkpointed): (i64, i64, i64) = result?;
        Ok(CheckpointOutcome {
            busy,
            log,
            checkpointed,
            truncated: busy == 0 && mode == "TRUNCATE",
        })
    }

    /// Current on-disk WAL size (0 for in-memory stores or a missing WAL file).
    pub fn wal_size_bytes(&self) -> Result<u64, StateError> {
        let Some(db) = &self.db_path else {
            return Ok(0);
        };
        let wal = PathBuf::from(format!("{}-wal", db.to_string_lossy()));
        Ok(std::fs::metadata(wal).map(|m| m.len()).unwrap_or(0))
    }

    /// Bound WAL growth by applying backpressure: whenever the WAL exceeds
    /// `max_bytes`, retry checkpoints until it shrinks or `timeout` elapses. If
    /// a writer cannot make progress (a long-lived reader pinning the WAL) this
    /// returns a typed [`StateError::WalBackpressure`] instead of letting the
    /// WAL grow without bound — fail closed, never silent.
    pub fn enforce_wal_bound(
        &self,
        max_bytes: u64,
        timeout: Duration,
    ) -> Result<CheckpointOutcome, StateError> {
        let start = std::time::Instant::now();
        let mut last = CheckpointOutcome::default();
        loop {
            if self.wal_size_bytes()? <= max_bytes {
                return Ok(last);
            }
            last = self.checkpoint_with_retry(2, Duration::from_millis(25))?;
            if self.wal_size_bytes()? <= max_bytes {
                return Ok(last);
            }
            if start.elapsed() >= timeout {
                return Err(StateError::WalBackpressure {
                    size: self.wal_size_bytes()?,
                    bound: max_bytes,
                });
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    /// `PRAGMA integrity_check`; returns the raw report (`ok` when clean).
    pub fn integrity_check(&self) -> Result<String, StateError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare("PRAGMA integrity_check")?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows.join("; "))
    }

    /// `PRAGMA foreign_key_check`; empty means clean.
    pub fn foreign_key_check(&self) -> Result<Vec<String>, StateError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare("PRAGMA foreign_key_check")?;
        let rows = stmt
            .query_map([], |r| {
                let tbl: String = r.get(0)?;
                let rowid: i64 = r.get(1)?;
                Ok(format!("{tbl}#{rowid}"))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Read-only projection snapshot. Pure function of the DB at call time —
    /// the UI never owns canonical state.
    pub fn snapshot(&self) -> Result<crate::projection::Snapshot, StateError> {
        use crate::projection::{lane_for, BurnRow, EventRow, ReceiptRow, Snapshot, TaskRow};
        let t0 = std::time::Instant::now();
        let conn = self.lock()?;

        let mut stmt = conn.prepare("SELECT id, kind, status FROM tasks ORDER BY id")?;
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

        let mut stmt =
            conn.prepare("SELECT seq, kind, payload FROM events ORDER BY seq DESC LIMIT 100")?;
        let stream: Vec<EventRow> = stmt
            .query_map([], |r| {
                Ok(EventRow {
                    seq: r.get(0)?,
                    kind: r.get(1)?,
                    payload: r.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let mut stmt =
            conn.prepare("SELECT id, task_id, kind, evidence FROM receipts ORDER BY id")?;
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

        let mut stmt = conn.prepare(
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

        let change_seq: Option<i64> = conn
            .query_row("SELECT MAX(seq) FROM change_log", [], |r| r.get(0))
            .unwrap_or(None);

        Ok(Snapshot {
            db_age_ms: t0.elapsed().as_millis() as u64,
            // G: the provenance label reflects the *actual* DB file, not a
            // hard-coded "studio.db" (a `--db other-name.db` run must not claim
            // to be studio.db).
            source: self
                .db_path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "studio.db".into()),
            fleet,
            stream,
            receipts,
            burn,
            change_seq: change_seq.unwrap_or(0),
            config: crate::projection::config_projection(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn file_store() -> (tempfile::TempDir, StateStore) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("studio.db");
        let store = StateStore::open(path.to_str().unwrap()).unwrap();
        (dir, store)
    }

    #[test]
    fn wal_mode_and_v2_schema_version() {
        let (dir, store) = file_store();
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
        let mode: String = store
            .lock()
            .unwrap()
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
        assert!(dir.path().join("studio.db-wal").exists());
    }

    #[test]
    fn ledger_is_append_only_and_cache_hits_excluded() {
        let (_dir, s) = file_store();
        s.record_tokens("a", 100, false).unwrap();
        s.record_tokens("a", 50, true).unwrap();
        assert_eq!(s.lifetime_spend().unwrap(), 100);
        s.record_tokens("a", 25, false).unwrap();
        assert_eq!(s.lifetime_spend().unwrap(), 125);
    }

    #[test]
    fn second_connection_in_same_process_gets_busy() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("studio.db");
        let p = path.to_str().unwrap();
        let a = StateStore::open(p).unwrap();
        let b = StateStore::open(p).unwrap();

        let conn_a = a.lock().unwrap();
        conn_a.execute_batch("BEGIN IMMEDIATE;").unwrap();
        conn_a
            .execute(
                "INSERT INTO events(kind,payload,ts_ms) VALUES('x','y',0)",
                [],
            )
            .unwrap();

        let conn_b = b.lock().unwrap();
        conn_b.busy_timeout(Duration::from_millis(20)).unwrap();
        let err = conn_b.execute_batch("BEGIN IMMEDIATE;").unwrap_err();
        match err {
            rusqlite::Error::SqliteFailure(e, _) => {
                assert!(
                    matches!(
                        e.code,
                        rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
                    ),
                    "unexpected sqlite code: {:?}",
                    e.code
                );
            }
            other => panic!("expected busy/locked, got {other:?}"),
        }
        drop(conn_b);
        conn_a.execute_batch("ROLLBACK;").unwrap();
        drop(conn_a);

        // Once the writer releases, a second writer proceeds (single-writer, not deadlock).
        b.upsert_task("t1", "coder", "building").unwrap();
        assert_eq!(a.snapshot().unwrap().fleet.len(), 1);
    }

    // --- genuine cross-process single-writer proof -------------------------
    // The test above uses two connections in *this* process. To prove the
    // discipline holds *across processes* we re-exec this test binary as a
    // child, have the child take `BEGIN IMMEDIATE`, and assert the parent's
    // connection is refused while the child holds it.

    const HELPER_DB_ENV: &str = "STC_STATE_HELPER_DB";
    const HELPER_READY_ENV: &str = "STC_STATE_HELPER_READY";

    /// Child entry point. A no-op unless spawned with `HELPER_DB_ENV` set.
    #[test]
    fn hold_write_transaction_child_helper() {
        let Ok(db) = std::env::var(HELPER_DB_ENV) else {
            return;
        };
        let ready = std::env::var(HELPER_READY_ENV).expect("ready marker path");
        let store = StateStore::open(&db).unwrap();
        let conn = store.lock().unwrap();
        conn.execute_batch("BEGIN IMMEDIATE;").unwrap();
        conn.execute(
            "INSERT INTO events(kind,payload,ts_ms) VALUES('helper','hold',0)",
            [],
        )
        .unwrap();
        std::fs::write(&ready, b"ready").unwrap();
        std::thread::sleep(Duration::from_millis(1500));
        conn.execute_batch("ROLLBACK;").unwrap();
    }

    #[test]
    fn cross_process_writer_is_excluded_while_child_holds_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("studio.db");
        let db = path.to_str().unwrap();
        // Create the v2 schema *before* the child exists, so the parent's
        // contention probe below never blocks on DDL.
        drop(StateStore::open(db).unwrap());
        let ready = dir.path().join("ready");
        let exe = std::env::current_exe().expect("test binary path");
        let mut child = std::process::Command::new(exe)
            .args([
                "--exact",
                "state::tests::hold_write_transaction_child_helper",
                "--nocapture",
            ])
            .env(HELPER_DB_ENV, db)
            .env(HELPER_READY_ENV, ready.to_str().unwrap())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("helper child spawns");

        let start = std::time::Instant::now();
        while !ready.exists() {
            if start.elapsed() > Duration::from_secs(10) {
                child.kill().ok();
                child.wait().ok();
                panic!("helper child never acquired the write transaction");
            }
            std::thread::sleep(Duration::from_millis(20));
        }

        // A bare connection (no DDL, short busy timeout) must be refused.
        let conn = rusqlite::Connection::open(db).unwrap();
        conn.busy_timeout(Duration::from_millis(50)).unwrap();
        let err = conn
            .execute_batch("BEGIN IMMEDIATE;")
            .expect_err("parent must be refused while the child holds the write lock");
        match err {
            rusqlite::Error::SqliteFailure(e, _) => assert!(
                matches!(
                    e.code,
                    rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
                ),
                "unexpected sqlite code: {:?}",
                e.code
            ),
            other => panic!("expected busy/locked, got {other:?}"),
        }
        drop(conn);

        child.kill().ok();
        child.wait().ok();
    }

    /// F: the app-level single-writer discipline depends on `with_write` using
    /// `BEGIN IMMEDIATE`. If that is mutated to a plain `BEGIN`, the transaction
    /// takes no lock until its first statement, so a concurrent
    /// `BEGIN IMMEDIATE` would *succeed* here — this test then fails.
    #[test]
    fn with_write_holds_immediate_lock_before_first_statement() {
        let (dir, store) = file_store();
        let db = dir.path().join("studio.db");
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();

        let writer =
            std::thread::spawn(move || store.with_write_lock_probe(entered_tx, release_rx));
        entered_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("probe entered its write transaction");

        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.busy_timeout(Duration::from_millis(50)).unwrap();
        let res = conn.execute_batch("BEGIN IMMEDIATE;");
        let refused = matches!(
            res,
            Err(rusqlite::Error::SqliteFailure(ref e, _))
                if matches!(
                    e.code,
                    rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
                )
        );
        // Release the probe before asserting so a failed test cannot deadlock.
        let _ = release_tx.send(());
        let _ = writer.join();
        assert!(
            refused,
            "with_write must hold the IMMEDIATE (write) lock *before* its closure runs; \
             a concurrent BEGIN IMMEDIATE must be refused, got {res:?}"
        );
    }

    /// G: the projection's `source` must name the actual database file.
    #[test]
    fn snapshot_source_reflects_actual_db_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("other-name.db");
        let store = StateStore::open(path.to_str().unwrap()).unwrap();
        let snap = store.snapshot().unwrap();
        assert_eq!(snap.source, "other-name.db");
    }

    fn open_reader(db: &std::path::Path) -> rusqlite::Connection {
        let r = rusqlite::Connection::open(db).unwrap();
        r.execute_batch("BEGIN; SELECT count(*) FROM events;")
            .unwrap();
        r
    }

    #[test]
    fn checkpoint_surfaces_busy_under_held_reader_then_recovers() {
        let (dir, s) = file_store();
        let db = dir.path().join("studio.db");
        // Establish WAL content first, then pin a snapshot with the reader.
        for i in 0..100 {
            s.append_event("pre", &i.to_string()).unwrap();
        }
        let reader = open_reader(&db);
        for i in 100..300 {
            s.append_event("tick", &i.to_string()).unwrap();
        }

        let held = s
            .checkpoint_with_retry(1, Duration::from_millis(1))
            .unwrap();
        assert!(
            !held.truncated,
            "checkpoint must not claim success while a reader is held: {held:?}"
        );
        assert!(
            held.busy != 0 || held.checkpointed < held.log,
            "the (busy,log,checkpointed) tuple must be surfaced, not discarded: {held:?}"
        );

        drop(reader);
        let ok = s
            .checkpoint_with_retry(4, Duration::from_millis(20))
            .unwrap();
        assert!(
            ok.truncated,
            "checkpoint must truncate once the reader leaves: {ok:?}"
        );
        assert_eq!(
            s.wal_size_bytes().unwrap(),
            0,
            "WAL must be truncated to zero"
        );
    }

    #[test]
    fn wal_is_bounded_with_a_concurrent_held_reader() {
        use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
        use std::sync::Arc;

        let (dir, s) = file_store();
        let db = dir.path().join("studio.db");
        let wal = dir.path().join("studio.db-wal");
        let bound: u64 = 2 * 1024 * 1024;
        let max = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let reader_ready = Arc::new(AtomicBool::new(false));

        // Establish WAL content before the reader pins a snapshot.
        for i in 0..100 {
            s.append_event("pre", &i.to_string()).unwrap();
        }

        // Sampler: record the highest WAL size seen.
        let (wal_s, max_s, stop_s) = (wal.clone(), max.clone(), stop.clone());
        let sampler = std::thread::spawn(move || {
            while !stop_s.load(Ordering::Relaxed) {
                let sz = std::fs::metadata(&wal_s).map(|m| m.len()).unwrap_or(0);
                max_s.fetch_max(sz, Ordering::Relaxed);
                std::thread::sleep(Duration::from_millis(2));
            }
        });

        // A concurrent reader holds a snapshot for a while, starving checkpoints.
        let (db_r, ready_r) = (db.clone(), reader_ready.clone());
        let reader = std::thread::spawn(move || {
            let r = open_reader(&db_r);
            ready_r.store(true, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(400));
            drop(r);
        });
        while !reader_ready.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(2));
        }

        // The writer must keep the WAL bounded via retry + backpressure.
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        let mut i: u64 = 100;
        while i < 4000 && std::time::Instant::now() < deadline {
            s.append_event("tick", &i.to_string()).unwrap();
            s.enforce_wal_bound(bound, Duration::from_secs(5)).unwrap();
            i += 1;
        }

        reader.join().unwrap();
        let final_out = s
            .checkpoint_with_retry(6, Duration::from_millis(25))
            .unwrap();
        assert!(
            final_out.truncated,
            "final checkpoint must truncate: {final_out:?}"
        );
        stop.store(true, Ordering::Relaxed);
        sampler.join().unwrap();

        let peak = max.load(Ordering::Relaxed);
        assert!(
            peak <= bound + 1024 * 1024,
            "WAL grew to {peak} bytes, beyond the {bound}-byte bound + 1MiB slack"
        );
        assert_eq!(s.wal_size_bytes().unwrap(), 0, "WAL must end truncated");
    }

    #[test]
    fn checkpoint_truncates_the_wal() {
        let (dir, s) = file_store();
        for i in 0..200 {
            s.append_event("tick", &i.to_string()).unwrap();
        }
        let wal = dir.path().join("studio.db-wal");
        let before = std::fs::metadata(&wal).map(|m| m.len()).unwrap_or(0);
        assert!(before > 0, "expected a non-empty WAL before checkpoint");
        let out = s.checkpoint().unwrap();
        assert!(
            out.truncated,
            "expected a completed TRUNCATE checkpoint: {out:?}"
        );
        let after = std::fs::metadata(&wal).map(|m| m.len()).unwrap_or(0);
        assert_eq!(after, 0, "checkpoint(TRUNCATE) must zero the WAL");
    }

    #[test]
    fn newer_schema_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("studio.db");
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE schema_meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
                 INSERT INTO schema_meta(key,value) VALUES('schema_version','999');",
            )
            .unwrap();
        }
        let err = StateStore::open(path.to_str().unwrap()).unwrap_err();
        assert!(matches!(err, StateError::NewerSchema { found: 999, .. }));
    }

    #[test]
    fn read_only_missing_database_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let err =
            StateStore::open_readonly(dir.path().join("nope.db").to_str().unwrap()).unwrap_err();
        assert!(matches!(err, StateError::NotFound(_)));
    }

    #[test]
    fn integrity_is_ok_after_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("studio.db");
        {
            let s = StateStore::open(path.to_str().unwrap()).unwrap();
            s.upsert_task("t1", "coder", "building").unwrap();
            s.append_event("dispatch", "t1").unwrap();
        }
        let s = StateStore::open(path.to_str().unwrap()).unwrap();
        assert_eq!(s.integrity_check().unwrap(), "ok");
        assert!(s.foreign_key_check().unwrap().is_empty());
        // Recovery is idempotent: re-opening again is a no-op.
        let s2 = StateStore::open(path.to_str().unwrap()).unwrap();
        assert_eq!(s2.integrity_check().unwrap(), "ok");
    }
}
