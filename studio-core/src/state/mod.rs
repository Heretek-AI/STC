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
use std::collections::HashSet;
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

-- B-EXEMPT-FORGE-01: persistent per-database engine identity. Generated once
-- (see `StateStore::engine_id`), never changed afterwards. The git lock reason
-- `studio:<engine_id>` written at worktree-create time is the ownership proof
-- that lets recovery tell our own abandoned half-creates apart from a forged
-- `creating` row naming another engine's live locked worktree.
CREATE TABLE IF NOT EXISTS engine_meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
-- A-DB-COPY-01: the file identity (`engine_dev`/`engine_ino`, see
-- `StateStore::engine_id`) the id above was generated for. A `cp` clones the
-- id but NOT the inode, so the copy diverges on first open.
-- C-UNLOCK-TRAP-01: persisted refusals. Once a path is refused (locked,
-- forged, control-char, or registered-worktree refusal) it is NEVER auto-reaped
-- by `gc` or daemon boot recovery, regardless of later lock state — unlocking a
-- refused path does NOT re-arm auto-reap. The ONLY way out is the explicit
-- operator command `studio release <path>` (see `StateStore::clear_refused`).
-- First refusal wins (`INSERT OR IGNORE` keeps the original reason/time).
CREATE TABLE IF NOT EXISTS refused_paths(path TEXT PRIMARY KEY, reason TEXT NOT NULL, at_ms INTEGER NOT NULL);

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

/// Phase-02 contract-runtime tables (additive; schema_version stays 2).
/// All `IF NOT EXISTS`, executed on every writable open alongside
/// `SCHEMA_SQL`. Pre-existing DBs gain them on next open; no migration step,
/// no version bump — these tables are new authority records, not a change to
/// the phase-01 tables. Backs: frozen candidates, approval requests, scoped
/// capability leases, burned ack tokens. Delivery receipts reuse `receipts`.
const CONTRACT_SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS frozen_candidates(
    freeze_hash   TEXT PRIMARY KEY,
    lineage_hash  TEXT NOT NULL,
    revision      INTEGER NOT NULL,
    target_ref    TEXT NOT NULL,
    targets_json  TEXT NOT NULL,
    contents_json TEXT NOT NULL,
    tier          TEXT NOT NULL,
    created_ms    INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS contract_approvals(
    id         TEXT PRIMARY KEY,
    task_id    TEXT NOT NULL,
    tool       TEXT NOT NULL,
    status     TEXT NOT NULL DEFAULT 'pending',
    reason     TEXT,
    created_ms INTEGER NOT NULL,
    decided_ms INTEGER
);
CREATE INDEX IF NOT EXISTS idx_contract_approvals_task ON contract_approvals(task_id);
CREATE TABLE IF NOT EXISTS capability_leases(
    id          TEXT PRIMARY KEY,
    approval_id TEXT NOT NULL,
    scope       TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    issued_ms   INTEGER NOT NULL,
    consumed    INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_capability_leases_approval ON capability_leases(approval_id);
CREATE TABLE IF NOT EXISTS ack_tokens(
    token       TEXT PRIMARY KEY,
    freeze_hash TEXT NOT NULL,
    task_id     TEXT NOT NULL DEFAULT '',
    issued_ms   INTEGER NOT NULL,
    burned      INTEGER NOT NULL DEFAULT 0
);
-- QA-R1: correction attempts are runtime state, not caller memory. A blocked
-- gate run consumes the single per-freeze attempt HERE, so a fresh (or
-- absent) CorrectionBudget object cannot buy more corrections for the same
-- frozen candidate. New hashes legitimately start at zero (re-freezing a new
-- candidate is the designed escape hatch, with its own receipt trail).
CREATE TABLE IF NOT EXISTS correction_attempts(
    freeze_hash TEXT PRIMARY KEY,
    attempts    INTEGER NOT NULL DEFAULT 0
);
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

/// One CDC row from `change_log` (P04 event-hub history cursor). Pure read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeEntry {
    pub seq: i64,
    pub tbl: String,
    pub row: String,
    pub op: String,
    pub old: Option<String>,
    pub new: Option<String>,
}

/// Canonicalize a worktree path string to an absolute, lexical form for storage
/// and comparison.
fn canonical_path(p: &str) -> String {
    crate::worktree::normalize_path(Path::new(p))
        .to_string_lossy()
        .into_owned()
}

/// Fresh random engine identity: 16 bytes as lowercase hex. No new
/// dependencies: `/dev/urandom` on unix, otherwise a (pid, nanos, counter)
/// tuple hashed with the std hasher. The value is only ever generated once per
/// database (guarded by `INSERT OR IGNORE`), so even the weaker fallback is a
/// stable unique-enough nonce — and it never leaves the local machine.
fn generate_engine_id() -> String {
    #[cfg(unix)]
    {
        if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
            use std::io::Read as _;
            let mut bytes = [0u8; 16];
            if f.read_exact(&mut bytes).is_ok() {
                return hex::encode(bytes);
            }
        }
    }
    use std::hash::{Hash as _, Hasher as _};
    use std::sync::atomic::{AtomicU64, Ordering};
    static FALLBACK_CTR: AtomicU64 = AtomicU64::new(0);
    let mut h = std::collections::hash_map::DefaultHasher::new();
    std::time::SystemTime::now().hash(&mut h);
    std::process::id().hash(&mut h);
    FALLBACK_CTR.fetch_add(1, Ordering::Relaxed).hash(&mut h);
    format!("{:016x}{:016x}", h.finish(), {
        let mut h2 = std::collections::hash_map::DefaultHasher::new();
        FALLBACK_CTR.load(Ordering::Relaxed).hash(&mut h2);
        std::time::SystemTime::now().hash(&mut h2);
        h2.finish()
    })
}

/// File identity of the live database file: `(st_dev, st_ino)` (A-DB-COPY-01).
/// `None` when the file cannot be stated (in-memory stores, non-unix
/// platforms): callers then keep the stored id — an unstatable file is not
/// proof of a copy, so we fail open on identity but stay fail-closed on
/// destruction (the lock-reason check still applies).
#[cfg(unix)]
fn live_file_identity(db_path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(db_path).ok().map(|m| (m.dev(), m.ino()))
}

/// Non-unix fallback: no inode identity available, so no copy divergence is
/// enforced here (documented residual; the lock-reason check still refuses
/// forged rows from a *different* engine id).
#[cfg(not(unix))]
fn live_file_identity(_db_path: &Path) -> Option<(u64, u64)> {
    None
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

/// RO-STATUS-01: a read-only *media* failure (as opposed to a locked/corrupt DB)
/// is what the `immutable=1` fallback is allowed to answer.
fn is_readonly_media_error(e: &StateError) -> bool {
    matches!(
        e,
        StateError::Sqlite(rusqlite::Error::SqliteFailure(sf, _))
            if matches!(
                sf.code,
                rusqlite::ErrorCode::ReadOnly | rusqlite::ErrorCode::CannotOpen
            )
    )
}

/// Build a `file:...?immutable=1` URI for a path, percent-encoding everything
/// outside the URI-unreserved set plus `/` so `?`, `#` and `%` in a path cannot
/// be mistaken for URI syntax.
fn sqlite_immutable_uri(path: &str) -> String {
    let mut s = String::with_capacity(path.len() + 24);
    s.push_str("file:");
    for b in path.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                s.push(b as char);
            }
            _ => s.push_str(&format!("%{b:02X}")),
        }
    }
    s.push_str("?immutable=1");
    s
}

/// Durable state store. Cheap to clone by `Arc`; all writers serialize on one
/// connection.
#[derive(Debug)]
pub struct StateStore {
    conn: Mutex<Connection>,
    /// Path of the on-disk database (`None` for in-memory stores).
    db_path: Option<PathBuf>,
    /// RO-STATUS-01: set when a read-only open had to degrade (e.g. immutable
    /// read on read-only media). Honest provenance for the projection.
    degradation: Option<String>,
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
            degradation: None,
        };
        store.init_schema()?;
        store.check_version()?;
        // A-DB-COPY-01: bind (or re-bind) the engine id to this file on EVERY
        // writable open. A `cp` clone diverges to its own id here, before any
        // recovery pass can honour a forged row with the cloned id.
        store.engine_id()?;
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
            degradation: None,
        };
        store.init_schema()?;
        store.check_version()?;
        Ok(store)
    }

    /// Read-only open for the CLI projection path. Never creates the DB and
    /// never runs DDL, so `studio status` works with no daemon running.
    ///
    /// RO-STATUS-01: on read-only media the WAL read path would need to write
    /// `-shm`/`-wal` sidecars and fail with `attempt to write a readonly
    /// database`. When the plain read-only open fails for that reason we retry
    /// with SQLite's `immutable=1` URI, which reads the main database file
    /// without creating or touching any sidecar. The degradation is recorded
    /// (and reported by `studio status`) because `immutable=1` also ignores any
    /// uncheckpointed `-wal` frames.
    pub fn open_readonly(path: &str) -> Result<Self, StateError> {
        if !Path::new(path).exists() {
            return Err(StateError::NotFound(path.to_string()));
        }
        match Self::open_readonly_impl(path, false) {
            Ok(store) => Ok(store),
            Err(primary) => {
                if !is_readonly_media_error(&primary) {
                    return Err(primary);
                }
                match Self::open_readonly_impl(path, true) {
                    Ok(mut store) => {
                        let wal = format!("{path}-wal");
                        let wal_len = std::fs::metadata(&wal).map(|m| m.len()).unwrap_or(0);
                        store.degradation = Some(format!(
                            "immutable read-only open (no sidecar writes); {wal_len}-byte -wal \
                             deliberately ignored — projection reflects the last checkpoint"
                        ));
                        Ok(store)
                    }
                    Err(_) => Err(primary),
                }
            }
        }
    }

    fn open_readonly_impl(path: &str, immutable: bool) -> Result<Self, StateError> {
        let (target, flags) = if immutable {
            (
                sqlite_immutable_uri(path),
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
            )
        } else {
            (path.to_string(), OpenFlags::SQLITE_OPEN_READ_ONLY)
        };
        let conn = Connection::open_with_flags(target, flags)?;
        conn.busy_timeout(std::time::Duration::from_millis(250))?;
        let store = Self {
            conn: Mutex::new(conn),
            db_path: Some(PathBuf::from(path)),
            degradation: None,
        };
        store.check_version()?;
        Ok(store)
    }

    /// RO-STATUS-01: why a read-only open degraded, if it did.
    pub fn read_degradation(&self) -> Option<&str> {
        self.degradation.as_deref()
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
        // Phase 02 (contract runtime): additive authority tables. `IF NOT
        // EXISTS`, so phase-01 DBs upgrade silently with schema_version pinned.
        conn.execute_batch(CONTRACT_SCHEMA_SQL)?;
        // QA-R4: task binding on ack tokens. DBs created before the column
        // existed gain it here (`ADD COLUMN` has no `IF NOT EXISTS`, so the
        // alter is pragma-gated instead of blind).
        let has_task: i64 = conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('ack_tokens') WHERE name='task_id'",
            [],
            |r| r.get(0),
        )?;
        if has_task == 0 {
            conn.execute_batch(
                "ALTER TABLE ack_tokens ADD COLUMN task_id TEXT NOT NULL DEFAULT ''",
            )?;
        }
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

    /// Persistent per-database engine identity (B-EXEMPT-FORGE-01), bound to the
    /// database FILE identity (A-DB-COPY-01). Generated once on first call
    /// (`INSERT OR IGNORE`, so concurrent openers agree), read back on every
    /// later call — including after a process restart, since it lives in the
    /// database file. The daemon stamps worktrees it locks with
    /// `studio:<engine_id>` (see `worktree::lock_reason_for`); recovery only
    /// unlocks+reaps an abandoned `creating` path whose live git lock reason
    /// matches this id, so a forged ledger row alone can never authorize
    /// destroying another engine's live locked worktree.
    ///
    /// A-DB-COPY-01: `cp studio.db backup.db` clones `engine_meta`, so without
    /// binding two engines would share one id and the copy plus a forged
    /// `creating` row would reap the victim. Every call therefore compares the
    /// live file's `(st_dev, st_ino)` against the stored pair:
    /// - match (normal restart, same file) → keep the id;
    /// - differ (copy, restore-to-new-path, replace) → REGENERATE the id and
    ///   store the new pair, so the copy diverges on first open and its forged
    ///   row mismatches the victim's `studio:<original-id>` lock reason;
    /// - no stored pair yet (pre-fix databases, or an attacker `DELETE` of the
    ///   pair — D-MIGRATION-01) → REGENERATE the id and store the new pair,
    ///   exactly like a copy. Keeping the id here would let a pre-fix-shaped
    ///   DB copied before its first post-fix open share one id in both files,
    ///   replaying the forged-row kill chain; and since pre-fix binaries never
    ///   issued `studio:<id>` lock reasons, nothing live can reference the
    ///   discarded id, so regenerating breaks nothing attributable;
    /// - unstatable file (in-memory, non-unix) → keep the id, bind nothing.
    ///
    /// Edge cases (explicit): hardlinks share one inode, so they share one id —
    /// CORRECT, it is the same file. `init` creating a fresh DB mints a new id
    /// plus the fresh identity. An in-place overwrite of the same path
    /// (plain `cp src.db dst.db` truncating onto dst's inode) does NOT keep
    /// the id: the stored pair travels with the source content and mismatches
    /// the destination's live identity, so the destination mints a fresh
    /// (third) id on next open — the safe direction (it diverges, and a
    /// forged row from either file refuses against the other). Only a
    /// byte-identical self-rewrite of a file's own content keeps the id.
    ///
    /// Never call this on a read-only store: generating/regenerating the id is
    /// a write and fails closed on read-only media.
    pub fn engine_id(&self) -> Result<String, StateError> {
        // Stat outside the write transaction; the stored pair is re-read inside
        // it, so two openers racing on the same copy converge (last writer
        // wins, both re-read the winner).
        let live = self.db_path.as_deref().and_then(live_file_identity);
        self.with_write(|conn| {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS engine_meta(key TEXT PRIMARY KEY, value TEXT NOT NULL)",
            )?;
            let get = |key: &str| -> Result<Option<String>, StateError> {
                conn.query_row(
                    "SELECT value FROM engine_meta WHERE key=?",
                    rusqlite::params![key],
                    |r| r.get(0),
                )
                .map(Some)
                .or_else(|e| match e {
                    rusqlite::Error::QueryReturnedNoRows => Ok(None),
                    other => Err(StateError::from(other)),
                })
            };
            let put = |key: &str, value: &str| -> Result<(), StateError> {
                conn.execute(
                    "INSERT OR REPLACE INTO engine_meta(key,value) VALUES(?,?)",
                    rusqlite::params![key, value],
                )?;
                Ok(())
            };
            let existing: Option<String> = get("engine_id")?;
            let stored_dev = get("engine_dev")?.and_then(|v| v.parse::<u64>().ok());
            let stored_ino = get("engine_ino")?.and_then(|v| v.parse::<u64>().ok());
            match (existing, stored_dev, stored_ino, live) {
                (Some(id), Some(d), Some(i), Some((ld, li))) if d == ld && i == li => Ok(id),
                (Some(_), _, _, Some((ld, li))) => {
                    // Stored pair differs from live (copy/restore/replace) or
                    // is absent/incomplete (pre-fix DB, or an attacker `DELETE`
                    // of the pair — D-MIGRATION-01): REGENERATE like a copy.
                    // Keep-and-bind here would leave a pre-fix-shaped DB and
                    // its `cp` clone sharing one id, replaying the forged-row
                    // kill chain. Safe: pre-fix binaries never issued
                    // `studio:<id>` lock reasons, so nothing live references
                    // the discarded id. Stored atomically with the new pair
                    // (same `with_write` transaction as every arm here).
                    let id = generate_engine_id();
                    put("engine_id", &id)?;
                    put("engine_dev", &ld.to_string())?;
                    put("engine_ino", &li.to_string())?;
                    let stored: String = conn.query_row(
                        "SELECT value FROM engine_meta WHERE key='engine_id'",
                        [],
                        |r| r.get(0),
                    )?;
                    Ok(stored)
                }
                (Some(id), _, _, _) => {
                    // Unstatable file (in-memory, non-unix): no identity to
                    // bind or compare, so keep the id and bind nothing.
                    Ok(id)
                }
                (None, _, _, _) => {
                    let id = generate_engine_id();
                    conn.execute(
                        "INSERT OR IGNORE INTO engine_meta(key,value) VALUES('engine_id',?)",
                        rusqlite::params![id],
                    )?;
                    if let Some((ld, li)) = live {
                        conn.execute(
                            "INSERT OR IGNORE INTO engine_meta(key,value) VALUES('engine_dev',?)",
                            rusqlite::params![ld.to_string()],
                        )?;
                        conn.execute(
                            "INSERT OR IGNORE INTO engine_meta(key,value) VALUES('engine_ino',?)",
                            rusqlite::params![li.to_string()],
                        )?;
                    }
                    let stored: String = conn.query_row(
                        "SELECT value FROM engine_meta WHERE key='engine_id'",
                        [],
                        |r| r.get(0),
                    )?;
                    Ok(stored)
                }
            }
        })
    }

    /// C-UNLOCK-TRAP-01: record a refused path. First refusal wins
    /// (`INSERT OR IGNORE` keeps the original reason and time). `reason` is one
    /// of `locked` (git-`locked` refusal, incl. forged rows), `control`
    /// (control-character path), `worktree` (registered but untracked), or
    /// `persisted` (re-refused on a stored refusal).
    pub fn record_refused(&self, path: &str, reason: &str) -> Result<(), StateError> {
        let canon = canonical_path(path);
        let ts = self.now_ms();
        self.with_write(|conn| {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS refused_paths(path TEXT PRIMARY KEY, reason TEXT NOT NULL, at_ms INTEGER NOT NULL)",
            )?;
            conn.execute(
                "INSERT OR IGNORE INTO refused_paths(path,reason,at_ms) VALUES(?,?,?)",
                rusqlite::params![canon, reason, ts],
            )?;
            Ok(())
        })
    }

    /// All persisted refused paths (canonical form), consulted by recovery
    /// before ANY reap: a refused path is never auto-reaped, regardless of
    /// later lock state.
    pub fn refused_set(&self) -> Result<HashSet<String>, StateError> {
        let conn = self.lock()?;
        let has: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='refused_paths'",
            [],
            |r| r.get(0),
        )?;
        if has == 0 {
            return Ok(HashSet::new());
        }
        let mut stmt = conn.prepare("SELECT path FROM refused_paths")?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows.into_iter().map(|p| canonical_path(&p)).collect())
    }

    /// The stored refusal reason for `path`, if it was ever refused.
    pub fn refused_reason(&self, path: &str) -> Result<Option<String>, StateError> {
        let canon = canonical_path(path);
        let conn = self.lock()?;
        let has: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='refused_paths'",
            [],
            |r| r.get(0),
        )?;
        if has == 0 {
            return Ok(None);
        }
        conn.query_row(
            "SELECT reason FROM refused_paths WHERE path=?",
            rusqlite::params![canon],
            |r| r.get(0),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(StateError::from(other)),
        })
    }

    /// Explicit operator recourse (the ONLY path from refused to reaped):
    /// forget the persisted refusal for `path`. Returns true when a refusal
    /// was actually removed. This does NOT unlock git, delete data, or drop
    /// ledger rows — the next `gc` treats the path as an ordinary orphan, so
    /// running this on a foreign LIVE worktree and then unlocking it lets `gc`
    /// reap it. That is the operator's responsibility (see `studio release`).
    pub fn clear_refused(&self, path: &str) -> Result<bool, StateError> {
        let canon = canonical_path(path);
        let removed = std::cell::Cell::new(false);
        self.with_write(|conn| {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS refused_paths(path TEXT PRIMARY KEY, reason TEXT NOT NULL, at_ms INTEGER NOT NULL)",
            )?;
            let n = conn.execute(
                "DELETE FROM refused_paths WHERE path=?",
                rusqlite::params![canon],
            )?;
            removed.set(n > 0);
            Ok(())
        })?;
        Ok(removed.get())
    }

    /// Run `f` inside exactly one `BEGIN IMMEDIATE` transaction on the single
    /// writer connection. Any error rolls the transaction back and propagates.
    /// Phase-02 contract modules (`verify`, `approvals`, `landing`) persist
    /// their authority records through this single-writer transaction helper.
    /// Same `BEGIN IMMEDIATE` discipline as the engine paths.
    pub(crate) fn with_write<T>(
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

    /// Phase-02 read helper for the contract modules (no DDL, no writes).
    pub(crate) fn with_read<T>(
        &self,
        f: impl FnOnce(&Connection) -> Result<T, StateError>,
    ) -> Result<T, StateError> {
        let conn = self.lock()?;
        f(&conn)
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

    /// History cursor: rows with `seq > since`, oldest-first, bounded by
    /// `limit` (clamped 1..=1024). The caller owns gap semantics: if `since`
    /// is older than the hub's retained floor the hub reports `Lost` — this
    /// helper never fabricates rows.
    pub fn changes_since(&self, since: i64, limit: usize) -> Result<Vec<ChangeEntry>, StateError> {
        let limit = (limit.clamp(1, 1024)) as i64;
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT seq,tbl,row,op,old,new FROM change_log WHERE seq>? ORDER BY seq ASC LIMIT ?",
        )?;
        let rows = stmt
            .query_map(rusqlite::params![since, limit], |r| {
                Ok(ChangeEntry {
                    seq: r.get(0)?,
                    tbl: r.get(1)?,
                    row: r.get(2)?,
                    op: r.get(3)?,
                    old: r.get(4)?,
                    new: r.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Highest `change_log` seq (0 on an empty log). The hub's retained floor
    /// and the client's cursor are both compared against this.
    pub fn max_change_seq(&self) -> Result<i64, StateError> {
        let conn = self.lock()?;
        let v: Option<i64> = conn.query_row("SELECT MAX(seq) FROM change_log", [], |r| r.get(0))?;
        Ok(v.unwrap_or(0))
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
    fn change_cursor_pages_oldest_first_bounded() {
        let (_dir, s) = file_store();
        s.upsert_task("t1", "coder", "building").unwrap();
        s.upsert_task("t1", "coder", "done").unwrap();
        s.upsert_task("t2", "reviewer", "in-review").unwrap();
        let max = s.max_change_seq().unwrap();
        assert!(max >= 3);
        let page = s.changes_since(0, 2).unwrap();
        assert_eq!(page.len(), 2);
        assert!(page[0].seq < page[1].seq);
        let rest = s.changes_since(page[1].seq, 1024).unwrap();
        assert!(rest.iter().all(|e| e.seq > page[1].seq));
        assert_eq!(rest.len() as i64, max - page[1].seq);
        // Empty log edge: fresh store reports floor 0, no rows.
        let (_d2, s2) = file_store();
        assert_eq!(s2.max_change_seq().unwrap(), 0);
        assert!(s2.changes_since(0, 10).unwrap().is_empty());
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

    /// RO-STATUS-01: a read-only *directory* (no sidecar writes possible) must
    /// not make `status` fail. The immutable fallback reads the DB and records
    /// the honest degradation.
    #[cfg(unix)]
    #[test]
    fn read_only_directory_open_succeeds_without_sidecars() {
        use std::os::unix::fs::PermissionsExt;
        // Root ignores directory permissions, so the failure cannot be provoked.
        // SAFETY: geteuid takes no arguments and cannot fail.
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("studio.db");
        {
            let s = StateStore::open(path.to_str().unwrap()).unwrap();
            s.upsert_task("t1", "coder", "building").unwrap();
            s.checkpoint().unwrap();
        }
        let _ = std::fs::remove_file(dir.path().join("studio.db-wal"));
        let _ = std::fs::remove_file(dir.path().join("studio.db-shm"));
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
        let opened = StateStore::open_readonly(path.to_str().unwrap());
        // Restore before asserting so the tempdir can always be cleaned up.
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();

        let store = opened.expect("status must succeed on read-only media");
        assert!(
            store.read_degradation().is_some(),
            "the read-only fallback must be reported honestly"
        );
        let snap = store.snapshot().unwrap();
        assert!(snap.fleet.iter().any(|t| t.id == "t1"), "{snap:?}");
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

    // --- A-DB-COPY-01: engine id is bound to the DB file identity ---

    /// `cp` clones `engine_meta` but not the inode: the copy MUST diverge to
    /// its own id on first open, while the original keeps its id.
    #[cfg(unix)]
    #[test]
    fn db_copy_diverges_engine_id_while_original_keeps_its_id() {
        let dir = tempfile::tempdir().unwrap();
        let orig = dir.path().join("studio.db");
        let copy = dir.path().join("backup.db");
        let id_orig = {
            let s = StateStore::open(orig.to_str().unwrap()).unwrap();
            let id = s.engine_id().unwrap();
            s.upsert_task("t1", "coder", "building").unwrap();
            s.checkpoint().unwrap();
            id
        };
        // The store is dropped (connection closed) before the copy, so the
        // copy is self-contained; the WAL is truncated by the checkpoint.
        std::fs::copy(&orig, &copy).unwrap();
        // Sanity: the clone really does carry the same id bytes.
        let cloned: String = rusqlite::Connection::open(&copy)
            .unwrap()
            .query_row(
                "SELECT value FROM engine_meta WHERE key='engine_id'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cloned, id_orig, "precondition: cp clones engine_meta");
        // First open of the copy diverges …
        let id_copy = StateStore::open(copy.to_str().unwrap())
            .unwrap()
            .engine_id()
            .unwrap();
        assert_ne!(
            id_copy, id_orig,
            "a copied database must not keep the victim's engine id"
        );
        // … and the divergence is stable, while the original is unaffected.
        let id_copy2 = StateStore::open(copy.to_str().unwrap())
            .unwrap()
            .engine_id()
            .unwrap();
        assert_eq!(id_copy, id_copy2, "the copy keeps its new id");
        let id_orig2 = StateStore::open(orig.to_str().unwrap())
            .unwrap()
            .engine_id()
            .unwrap();
        assert_eq!(id_orig, id_orig2, "the original keeps its id");
        // The original's data survived the whole exercise.
        let s = StateStore::open(orig.to_str().unwrap()).unwrap();
        assert_eq!(s.snapshot().unwrap().fleet.len(), 1);
    }

    /// A hardlink is the SAME file (same inode): sharing the id is correct.
    #[cfg(unix)]
    #[test]
    fn hardlink_shares_the_engine_id() {
        let dir = tempfile::tempdir().unwrap();
        let orig = dir.path().join("studio.db");
        let link = dir.path().join("link.db");
        let id_orig = StateStore::open(orig.to_str().unwrap())
            .unwrap()
            .engine_id()
            .unwrap();
        std::fs::hard_link(&orig, &link).unwrap();
        let id_link = StateStore::open(link.to_str().unwrap())
            .unwrap()
            .engine_id()
            .unwrap();
        assert_eq!(id_orig, id_link, "a hardlink is the same file");
    }

    /// D-MIGRATION-01: pre-fix databases have an id but no stored file
    /// identity. The first open after upgrade must REGENERATE (not
    /// keep-and-bind): a pre-fix-shaped DB copied before its first post-fix
    /// open would otherwise share one id in both files, replaying the
    /// forged-row kill chain — and the shape is attacker-reachable with one
    /// `sqlite DELETE` of the pair. Safe: pre-fix binaries never issued
    /// `studio:<id>` lock reasons, so nothing live references the discarded
    /// id. The new id is stable afterwards, with the live pair bound.
    #[cfg(unix)]
    #[test]
    fn missing_file_identity_regenerates_a_fresh_id_and_binds_the_pair() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("studio.db");
        let id = StateStore::open(path.to_str().unwrap())
            .unwrap()
            .engine_id()
            .unwrap();
        // Simulate a pre-fix row: drop the file-identity keys by hand.
        rusqlite::Connection::open(&path)
            .unwrap()
            .execute_batch("DELETE FROM engine_meta WHERE key IN ('engine_dev','engine_ino')")
            .unwrap();
        let id2 = StateStore::open(path.to_str().unwrap())
            .unwrap()
            .engine_id()
            .unwrap();
        assert_ne!(id, id2, "a pre-fix-shaped DB must regenerate, not keep");
        // And a second open is a stable no-op.
        let id3 = StateStore::open(path.to_str().unwrap())
            .unwrap()
            .engine_id()
            .unwrap();
        assert_eq!(id2, id3);
    }

    /// D-MIGRATION-01: a pre-fix-shaped DB (id, no pair) copied BEFORE the
    /// first post-fix open: opening both must DIVERGE — neither file may
    /// keep serving the legacy id.
    #[cfg(unix)]
    #[test]
    fn prefix_shaped_copy_diverges_on_first_open_of_both_files() {
        let dir = tempfile::tempdir().unwrap();
        let orig = dir.path().join("studio.db");
        let copy = dir.path().join("backup.db");
        let id_legacy = {
            let s = StateStore::open(orig.to_str().unwrap()).unwrap();
            let id = s.engine_id().unwrap();
            s.upsert_task("t1", "coder", "building").unwrap();
            s.checkpoint().unwrap();
            id
        };
        // Simulate pre-fix shape, then copy before any post-fix open.
        rusqlite::Connection::open(&orig)
            .unwrap()
            .execute_batch("DELETE FROM engine_meta WHERE key IN ('engine_dev','engine_ino')")
            .unwrap();
        std::fs::copy(&orig, &copy).unwrap();
        let id_orig = StateStore::open(orig.to_str().unwrap())
            .unwrap()
            .engine_id()
            .unwrap();
        let id_copy = StateStore::open(copy.to_str().unwrap())
            .unwrap()
            .engine_id()
            .unwrap();
        assert_ne!(
            id_orig, id_legacy,
            "the original must abandon the legacy id"
        );
        assert_ne!(id_copy, id_legacy, "the copy must abandon the legacy id");
        assert_ne!(
            id_orig, id_copy,
            "pre-fix-shaped original and copy must not share an id"
        );
        // Both are stable afterwards.
        assert_eq!(
            id_orig,
            StateStore::open(orig.to_str().unwrap())
                .unwrap()
                .engine_id()
                .unwrap()
        );
        assert_eq!(
            id_copy,
            StateStore::open(copy.to_str().unwrap())
                .unwrap()
                .engine_id()
                .unwrap()
        );
    }

    // --- C-UNLOCK-TRAP-01: refused paths persist per DB ---

    #[test]
    fn refused_paths_roundtrip_persist_and_release() {
        let (_dir, s) = file_store();
        assert!(s.refused_set().unwrap().is_empty());
        assert_eq!(s.refused_reason("/x/a").unwrap(), None);
        s.record_refused("/x/a", "locked").unwrap();
        s.record_refused("/x/b", "control").unwrap();
        // First refusal wins.
        s.record_refused("/x/a", "worktree").unwrap();
        let set = s.refused_set().unwrap();
        assert!(set.iter().any(|p| p.ends_with("x/a")));
        assert!(set.iter().any(|p| p.ends_with("x/b")));
        assert_eq!(s.refused_reason("/x/a").unwrap().as_deref(), Some("locked"));
        // Release removes exactly one path.
        assert!(s.clear_refused("/x/a").unwrap());
        assert!(!s.clear_refused("/x/a").unwrap());
        assert_eq!(s.refused_reason("/x/a").unwrap(), None);
        assert_eq!(
            s.refused_reason("/x/b").unwrap().as_deref(),
            Some("control")
        );
    }
}
