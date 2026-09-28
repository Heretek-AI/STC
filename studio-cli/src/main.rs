//! `studio` — STC v2 command surface for the engine skeleton.
//!
//! Read verbs (`status`, `verify`) are pure projections of the v2 database and
//! work with **no daemon running**. `gc` is the boot-time recovery pass; the
//! daemon (`daemon`) is the only writer.

use clap::{Parser, Subcommand};
use serde_json::{json, Value};
use std::path::Path;
use std::process::ExitCode;
use studio_core::lock::StudioLock;
use studio_core::recovery;
use studio_core::state::{StateError, StateStore};
use studio_core::worktree::WorktreeManager;

/// Default soft bound on the WAL size before the daemon applies write
/// backpressure (bytes). Overridable per run with `--wal-bound-bytes` or the
/// `STUDIO_WAL_BOUND_BYTES` env var (C2).
const WAL_BOUND_BYTES_DEFAULT: u64 = 4 * 1024 * 1024;

/// Hard WAL ceiling: only reachable by *genuinely unbounded* growth (a writer
/// that keeps writing without any pause). Plain reader contention pauses writes
/// and therefore never approaches it (C1). Defaults to 16× the soft bound;
/// overridable with `STUDIO_WAL_HARD_CEILING_BYTES` / `--wal-hard-ceiling-bytes`.
const WAL_HARD_CEILING_FACTOR: u64 = 16;

/// Resolve the soft WAL bound: CLI flag > env > default.
fn resolve_wal_bound(cli: Option<u64>) -> u64 {
    if let Some(v) = cli {
        return v.max(1);
    }
    std::env::var("STUDIO_WAL_BOUND_BYTES")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(WAL_BOUND_BYTES_DEFAULT)
}

/// Resolve the hard WAL ceiling: CLI flag > env > `factor * bound`.
fn resolve_wal_hard_ceiling(cli: Option<u64>, bound: u64) -> u64 {
    if let Some(v) = cli {
        return v.max(1);
    }
    std::env::var("STUDIO_WAL_HARD_CEILING_BYTES")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|n| *n > 0)
        .unwrap_or_else(|| bound.saturating_mul(WAL_HARD_CEILING_FACTOR))
}

#[derive(Parser)]
#[command(name = "studio", version, about = "STC v2 engine skeleton")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create/upgrade the v2 database (idempotent) and exit.
    Init(Paths),
    /// Print the read-only DB projection (works with no daemon).
    Status(Paths),
    /// Run the single-writer daemon (writes rows + worktrees until killed).
    /// Engine-exclusive: refuses to start if another daemon or GC holds the lock.
    Daemon(DaemonArgs),
    /// Boot-time recovery: reconcile ledger with disk + git, idempotent.
    ///
    /// GC is daemon-exclusive: it takes the same engine lock as `daemon` and
    /// refuses to run while a daemon holds it.
    ///
    /// REFUSED-PATH operator story (C-UNLOCK-TRAP-01): an abandoned git-locked
    /// worktree with no ledger row of ours (or a forged row we refused) is
    /// refused, and the refusal is PERSISTED in this DB — `git worktree unlock
    /// <path>` followed by `gc` still refuses (unlocking never re-arms
    /// auto-reap; the old unlock-then-gc remedy was a trap). The ONLY path
    /// from refused to reaped is `studio release <path>` (explicit operator
    /// intent), then `gc`. Running `release` on a foreign LIVE worktree and
    /// then unlocking it lets `gc` reap it — the operator's responsibility.
    /// Exit code is 1 whenever the report is not clean (reaped, refused, or
    /// otherwise unclean), matching `verify`'s contract.
    Gc(Paths),
    /// Forget a persisted gc refusal for one path (C-UNLOCK-TRAP-01 recourse).
    ///
    /// This is the ONLY path from refused to reaped: it drops the stored
    /// refusal (and a lingering `creating` ledger row for the path, if any —
    /// the wedge previously clearable only by hand-`sqlite3 DELETE`). The next
    /// `gc` then treats the path as an ordinary orphan (a still-locked foreign
    /// worktree is refused again by its live lock; an unlocked stale path is
    /// reaped). It does NOT unlock git or delete data. Releasing a foreign
    /// LIVE worktree and then unlocking it lets `gc` destroy it — the
    /// operator's responsibility.
    Release(ReleaseArgs),
    /// Read-only integrity check; exit 1 if anything is unclean.
    Verify(Paths),
}

#[derive(clap::Args, Clone)]
struct Paths {
    /// Path to the v2 database.
    #[arg(long, default_value = "studio.db")]
    db: String,
    /// Git repository the daemon creates worktrees from.
    #[arg(long, default_value = ".")]
    repo: String,
    /// Root directory for worktrees.
    #[arg(long, default_value = ".studio/worktrees")]
    worktree_root: String,
}

#[derive(clap::Args)]
struct DaemonArgs {
    #[command(flatten)]
    paths: Paths,
    /// Stop after this many ticks (default: run until killed).
    #[arg(long)]
    ticks: Option<u64>,
    /// Soft WAL bound in bytes (default 4 MiB or `STUDIO_WAL_BOUND_BYTES`).
    #[arg(long)]
    wal_bound_bytes: Option<u64>,
    /// Hard WAL ceiling in bytes; only unbounded growth reaches it.
    #[arg(long)]
    wal_hard_ceiling_bytes: Option<u64>,
}

#[derive(clap::Args)]
struct ReleaseArgs {
    /// Path to the v2 database holding the refusal.
    #[arg(long, default_value = "studio.db")]
    db: String,
    /// Worktree path whose persisted refusal should be forgotten.
    path: String,
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli).await {
        Ok(code) => code,
        Err(e) => {
            eprintln!("studio: error: {e}");
            ExitCode::from(2)
        }
    }
}

async fn run(cli: Cli) -> Result<ExitCode, Box<dyn std::error::Error>> {
    match cli.cmd {
        Cmd::Init(p) => {
            let store = StateStore::open(&p.db)?;
            store.checkpoint()?;
            println!(
                "{}",
                json!({"ok": true, "db": p.db, "schema_version": store.schema_version()?, "engine_id": store.engine_id()?})
            );
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Status(p) => {
            let store = StateStore::open_readonly(&p.db)?;
            let snap = store.snapshot()?;
            let mut v = serde_json::to_value(&snap)?;
            if let Value::Object(ref mut m) = v {
                m.insert("schema_version".into(), json!(store.schema_version()?));
                m.insert("worktrees".into(), json!(store.worktree_paths()?.len()));
                m.insert("read_only".into(), json!(true));
                // RO-STATUS-01: honest provenance when a read-only media open
                // had to degrade (immutable read; uncheckpointed WAL ignored).
                if let Some(d) = store.read_degradation() {
                    m.insert("degraded".into(), json!(true));
                    m.insert("degradation".into(), json!(d));
                }
            }
            println!("{}", serde_json::to_string_pretty(&v)?);
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Gc(p) => {
            // GC mutates FS + git; it must not race a live daemon (on ANY db)
            // that shares this repo/worktree root (B).
            let repo = studio_core::worktree::canonicalize_existing(Path::new(&p.repo))?;
            let root = studio_core::worktree::canonicalize_or_create(Path::new(&p.worktree_root))?;
            let _lock = StudioLock::acquire(&repo, &root)?;
            let store = StateStore::open(&p.db)?;
            let report = recovery::recover(&store, &repo, &root).await?;
            println!(
                "{}",
                json!({
                    "clean": report.clean(),
                    "engine_id": store.engine_id()?,
                    "integrity_ok": report.integrity_ok,
                    "integrity_detail": report.integrity_detail,
                    "reaped_creating": report.reaped_creating,
                    "reaped_git_orphans": report.reaped_git_orphans,
                    "dropped_missing_rows": report.dropped_missing_rows,
                    "swept_dirs": report.swept_dirs,
                    "refused_symlinks": report.refused_symlinks,
                    "refused_worktrees": report.refused_worktrees,
                    "refused_locked": report.refused_locked,
                    "refused_control_paths": report.refused_control_paths,
                    "refused_persisted": report.refused_persisted,
                })
            );
            // Like `verify`: a report that is not clean exits non-zero, so a
            // refused (never reaped — unlocking does NOT re-arm auto-reap; use
            // `studio release <path>`) or otherwise active recovery is visible
            // to callers and scripts.
            Ok(if report.clean() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            })
        }
        Cmd::Release(a) => {
            let store = StateStore::open(&a.db)?;
            let canon = studio_core::worktree::normalize_path(Path::new(&a.path))
                .to_string_lossy()
                .into_owned();
            let prior_reason = store.refused_reason(&canon)?;
            // The lingering wedge: a refused forged `creating` row is kept as
            // evidence by gc; releasing the path drops it too, so the refusal
            // is clearable with no hand-sqlite. `ready` rows are never touched.
            let creating_row = store.worktree_rows()?.into_iter().any(|(p, s)| {
                s == "creating"
                    && studio_core::worktree::normalize_path(Path::new(&p))
                        .to_string_lossy()
                        .into_owned()
                        == canon
            });
            let mut ledger_row_removed = false;
            let released = store.clear_refused(&canon)?;
            if released && creating_row {
                store.delete_worktree(&canon)?;
                ledger_row_removed = true;
            }
            println!(
                "{}",
                json!({
                    "released": released,
                    "path": canon,
                    "prior_reason": prior_reason,
                    "ledger_creating_row_removed": ledger_row_removed,
                    "warning": "release does not unlock git or delete data; the next gc treats the path as an ordinary orphan — releasing a foreign LIVE worktree and then unlocking it lets gc reap it (operator's responsibility)",
                })
            );
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Verify(p) => {
            let store = StateStore::open_readonly(&p.db)?;
            let report =
                recovery::verify(&store, Path::new(&p.repo), Path::new(&p.worktree_root)).await?;
            println!(
                "{}",
                json!({
                    "clean": report.clean(),
                    "integrity_ok": report.integrity_ok,
                    "integrity_detail": report.integrity_detail,
                    "creating_rows": report.creating_rows,
                    "missing_rows": report.missing_rows,
                    "invalid_state_rows": report.invalid_state_rows,
                    "root_missing": report.root_missing,
                    "outside_root_rows": report.outside_root_rows,
                    "repo_mismatch_rows": report.repo_mismatch_rows,
                    "non_directory_rows": report.non_directory_rows,
                    "empty_rows": report.empty_rows,
                    "not_a_worktree_rows": report.not_a_worktree_rows,
                    "refused_symlinks": report.refused_symlinks,
                    "git_orphans": report.git_orphans,
                    "leftover_dirs": report.leftover_dirs,
                })
            );
            Ok(if report.clean() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            })
        }
        Cmd::Daemon(a) => {
            run_daemon(
                a.paths,
                a.ticks,
                a.wal_bound_bytes,
                a.wal_hard_ceiling_bytes,
            )
            .await?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// The only writer: one tick writes rows (inside `BEGIN IMMEDIATE`) and, every
/// few ticks, creates a worktree recorded *before* git runs. The process is
/// expected to be `kill -9`-ed at an arbitrary instant; recovery on the next
/// boot reconciles whatever was interrupted.
///
/// The engine lock is keyed to the shared repo + worktree root and taken for the
/// whole process, so a second daemon on the same repo/root is refused up front —
/// even if it uses a different database (B).
async fn run_daemon(
    p: Paths,
    ticks: Option<u64>,
    wal_bound_arg: Option<u64>,
    wal_hard_arg: Option<u64>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Canonicalize repo + root so every worktree path we record, every git path
    // and every recovery comparison share one absolute form (D1), and so git is
    // invoked with absolute paths regardless of cwd (D3).
    let repo = studio_core::worktree::canonicalize_existing(Path::new(&p.repo))?;
    let root = studio_core::worktree::canonicalize_or_create(Path::new(&p.worktree_root))?;
    // FS + git mutations are engine-exclusive, keyed to the shared resource (D4/B).
    let _lock = StudioLock::acquire(&repo, &root)?;

    let store = StateStore::open(&p.db)?;
    let boot = recovery::recover(&store, &repo, &root).await?;
    eprintln!("[daemon] boot recovery: clean={}", boot.clean());
    store.checkpoint()?;

    // B-EXEMPT-FORGE-01: our persistent ownership proof, stamped as the git
    // lock reason on every worktree we create (stable across restarts).
    let engine_id = store.engine_id()?;
    let lock_reason = studio_core::worktree::lock_reason_for(&engine_id);

    let wal_bound = resolve_wal_bound(wal_bound_arg);
    let wal_hard_ceiling = resolve_wal_hard_ceiling(wal_hard_arg, wal_bound);
    eprintln!("[daemon] wal soft bound={wal_bound} bytes hard ceiling={wal_hard_ceiling} bytes");

    let wm = WorktreeManager::new();
    let mut i: u64 = 0;
    let mut peak_wal: u64 = 0;
    loop {
        let status = if i.is_multiple_of(3) {
            "validating"
        } else {
            "building"
        };
        store.upsert_task(&format!("t{}", i % 32), "coder", status)?;
        store.append_event("tick", &i.to_string())?;
        store.record_tokens("daemon", 1, false)?;

        if i.is_multiple_of(4) {
            let slug = format!("wt-{i}");
            let path = WorktreeManager::path_for(&root, "scratch", &slug);
            let ps = path.to_string_lossy().to_string();
            store.begin_worktree(&ps, &repo.to_string_lossy(), &slug)?;
            wm.create(&repo, &path, "HEAD", &lock_reason).await?;
            store.mark_worktree_ready(&ps)?;
        }

        // HON-C3-01: sample the WAL peak *before* any checkpoint in this
        // iteration. Sampling after `enforce_wal_bound`/the periodic checkpoint
        // reported 0 while an external sampler observed 53–61 KB.
        peak_wal = peak_wal.max(store.wal_size_bytes()?);

        if i.is_multiple_of(64) {
            store.checkpoint()?;
        }

        // Backpressure: never let a starved checkpoint grow the WAL unbounded,
        // but do NOT let a plain local O_RDONLY reader kill the engine (C1).
        // Any process that can read the 0600 db can hold a snapshot; that is
        // reader contention, so we pause writes until a checkpoint can proceed.
        match store.enforce_wal_bound(wal_bound, std::time::Duration::from_millis(250)) {
            Ok(_) => {}
            Err(StateError::WalBackpressure { size, .. }) => {
                if size > wal_hard_ceiling {
                    // Genuinely unbounded growth even while not writing: fail closed.
                    return Err(StateError::WalBackpressure {
                        size,
                        bound: wal_hard_ceiling,
                    }
                    .into());
                }
                eprintln!(
                    "[daemon] WAL pressure: {size} bytes > soft bound {wal_bound} \
                     (reader contention); pausing writes, engine stays up"
                );
                relieve_wal_pressure(&store, wal_bound, wal_hard_ceiling).await?;
            }
            Err(e) => return Err(e.into()),
        }

        peak_wal = peak_wal.max(store.wal_size_bytes()?);
        i += 1;
        if let Some(t) = ticks {
            if i >= t {
                break;
            }
        }
    }
    store.checkpoint()?;
    eprintln!(
        "[daemon] peak WAL {peak_wal} bytes (soft bound {wal_bound}, hard ceiling {wal_hard_ceiling})"
    );
    Ok(())
}

/// Graceful degradation under reader contention: stop writing and retry
/// checkpoints until the WAL is back under the soft bound. A reader that never
/// leaves simply pauses the writer — the daemon stays alive and the WAL stays
/// bounded (no writes happen here). Only growth past the hard ceiling is fatal.
async fn relieve_wal_pressure(
    store: &StateStore,
    bound: u64,
    hard_ceiling: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        // Best-effort checkpoint; a held reader makes this a no-op.
        let _ = store.checkpoint();
        let size = store.wal_size_bytes()?;
        if size <= bound {
            eprintln!("[daemon] WAL pressure relieved at {size} bytes; resuming writes");
            return Ok(());
        }
        if size > hard_ceiling {
            return Err(StateError::WalBackpressure {
                size,
                bound: hard_ceiling,
            }
            .into());
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}
