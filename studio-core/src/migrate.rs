//! Forward-only schema migrations (Phase 6 WS4).
//! `PRAGMA user_version` tracks the applied version. Before the first pending
//! step, a `VACUUM INTO <db>.bak.<ts>` backup is taken (never inside a
//! transaction — SQLite forbids it). Opening a database newer than the engine
//! understands fails closed (refuse to boot against an unknown layout).

use rusqlite::Connection;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MigrateError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("database schema v{found} is newer than supported v{supported}; refusing to boot")]
    TooNew { found: i64, supported: i64 },
    #[error("backup failed: {0}")]
    Backup(String),
}

pub struct Migration {
    pub version: i64,
    pub name: &'static str,
    pub sql: &'static str,
}

pub fn current_version(conn: &Connection) -> Result<i64, MigrateError> {
    Ok(conn.query_row("PRAGMA user_version", [], |r| r.get(0))?)
}

/// Apply pending steps in order. Returns applied versions. `backup_path`, when
/// given, receives exactly one `VACUUM INTO` snapshot before the first step.
pub fn run_migrations(
    conn: &Connection,
    target: i64,
    steps: &[Migration],
    backup_path: Option<&std::path::Path>,
) -> Result<Vec<i64>, MigrateError> {
    let found = current_version(conn)?;
    if found > target {
        return Err(MigrateError::TooNew {
            found,
            supported: target,
        });
    }
    let pending: Vec<&Migration> = steps
        .iter()
        .filter(|s| s.version > found && s.version <= target)
        .collect();
    if pending.is_empty() {
        return Ok(vec![]);
    }
    if let Some(dest) = backup_path {
        if let Some(p) = dest.parent() {
            std::fs::create_dir_all(p).map_err(|e| MigrateError::Backup(e.to_string()))?;
        }
        let sql = format!(
            "VACUUM INTO '{}'",
            dest.to_string_lossy().replace('\'', "''")
        );
        conn.execute_batch(&sql)
            .map_err(|e| MigrateError::Backup(e.to_string()))?;
    }
    let mut applied = vec![];
    for step in pending {
        conn.execute_batch(step.sql)?;
        conn.execute_batch(&format!("PRAGMA user_version = {}", step.version))?;
        applied.push(step.version);
    }
    Ok(applied)
}

#[cfg(test)]
mod tests {
    use super::*;

    const V1: &str = "CREATE TABLE IF NOT EXISTS t(id TEXT PRIMARY KEY);";

    #[test]
    fn applies_pending_and_stamps_version() {
        let conn = Connection::open_in_memory().unwrap();
        let applied = run_migrations(
            &conn,
            1,
            &[Migration {
                version: 1,
                name: "base",
                sql: V1,
            }],
            None,
        )
        .unwrap();
        assert_eq!(applied, vec![1]);
        assert_eq!(current_version(&conn).unwrap(), 1);
        // Idempotent re-run.
        let again = run_migrations(
            &conn,
            1,
            &[Migration {
                version: 1,
                name: "base",
                sql: V1,
            }],
            None,
        )
        .unwrap();
        assert!(again.is_empty());
    }

    #[test]
    fn refuses_newer_database() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA user_version = 99").unwrap();
        let err = run_migrations(
            &conn,
            1,
            &[Migration {
                version: 1,
                name: "base",
                sql: V1,
            }],
            None,
        )
        .unwrap_err();
        assert!(matches!(err, MigrateError::TooNew { .. }));
    }

    #[test]
    fn backup_precedes_first_step() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("s.db");
        let bak = dir.path().join("s.db.bak.1");
        {
            let conn = Connection::open(&db).unwrap();
            let applied = run_migrations(
                &conn,
                1,
                &[Migration {
                    version: 1,
                    name: "base",
                    sql: V1,
                }],
                Some(&bak),
            )
            .unwrap();
            assert_eq!(applied, vec![1]);
        }
        assert!(bak.exists());
    }
}
