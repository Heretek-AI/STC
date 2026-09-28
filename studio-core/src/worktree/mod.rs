//! Worktree lifecycle: fail-closed, idempotent, crash-safe.
//! Anchors (by symbol):
//! - agent-orchestrator `ports/outbound.go`: Workspace `Create/Destroy/StashUncommitted/ApplyPreserved/ForceDestroy`,
//!   `ErrWorkspaceDirty`, `ErrPreservedConflict`
//! - agent-of-empires `git/worktree`: `lock_worktree/unlock_worktree/prune_worktrees`, mutation timeouts
//! - orca `worktree-trash.ts`: `moveWorktreeDirectoryToTrash`, `WORKTREE_TRASH_DIR_NAME`,
//!   create `--detach --no-checkout` -> `reset --hard` -> `lock --reason`

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tokio::process::Command;

#[derive(Debug, Error)]
pub enum WorktreeError {
    #[error("workspace dirty, refusing destroy without stash (fail-closed)")]
    WorkspaceDirty,
    #[error("preserved conflict on apply: {0}")]
    PreservedConflict(String),
    #[error("git failure: {0}")]
    Git(String),
    #[error("io: {0}")]
    Io(String),
    /// Refused a symlink: the engine never traverses/relocates through one.
    #[error("refusing to traverse symlink (never leaves the worktree root): {0}")]
    SymlinkRefused(String),
    #[error("refusing: expected a directory, found something else: {0}")]
    NotADirectory(String),
    /// C-NEWLINE-01: the path (derived from a task slug) or the lock reason
    /// contains control characters (newline etc.). Such paths cannot be
    /// represented safely in git's LF-delimited porcelain and are refused at
    /// creation, fail closed.
    #[error("refusing path with control characters (newline etc.): {0}")]
    ControlCharsRefused(String),
}

/// Outcome of a merge attempt. Never destructive on conflict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeOutcome {
    Merged,
    ConflictRetained {
        conflict_files: Vec<String>,
        diagnostic: String,
    },
}

/// Per-repo operation lock: serializes worktree mutations per repo root.
/// Guards against `.git/index.lock` collisions from concurrent creates.
#[derive(Debug, Default)]
pub struct WorktreeOperationLock {
    locks: std::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

impl WorktreeOperationLock {
    pub fn for_repo(&self, repo: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.locks
            .lock()
            .unwrap()
            .entry(repo.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }
}

/// Absolute, lexically-normalized path. Relative paths resolve against the
/// current directory; `.` is dropped and `..` folds. Unlike `Path::canonicalize`
/// this needs no existing file and does not resolve symlinks, so it is safe for
/// a path recorded *before* `git worktree add` runs. Every comparison of a
/// ledger path against a git path goes through this, so a relative
/// `--worktree-root` can never make a live worktree look like an orphan.
pub fn normalize_path(p: &Path) -> PathBuf {
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(p))
            .unwrap_or_else(|_| p.to_path_buf())
    };
    lexical_normalize(&abs)
}

fn lexical_normalize(p: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Canonicalize a path that must already exist (resolves symlinks).
pub fn canonicalize_existing(p: &Path) -> Result<PathBuf, WorktreeError> {
    p.canonicalize()
        .map_err(|e| WorktreeError::Io(format!("{}: {e}", p.display())))
}

/// True when `s` contains control characters (newline, CR, tab, NUL, DEL…).
/// C-NEWLINE-01: slugs derive from task names, so this is reachable input, not
/// theoretical. Git's LF-delimited porcelain cannot represent such paths (the
/// `\nlocked` tail of `evil\nlocked` masquerades as a `locked` line), so paths
/// with control characters are refused at creation and never reaped by GC.
pub fn has_control_chars(s: &str) -> bool {
    s.chars().any(|c| c.is_control())
}

/// Byte-level variant for `OsStr` names encountered during the sweep (a raw
/// `0x0A` byte in a file name is a newline regardless of UTF-8 validity;
/// bytes `>= 0x80` may be UTF-8 continuation and must NOT be flagged).
#[cfg(unix)]
pub fn name_has_control_bytes(name: &std::ffi::OsStr) -> bool {
    use std::os::unix::ffi::OsStrExt as _;
    name.as_bytes().iter().any(|b| *b < 0x20 || *b == 0x7f)
}

/// Ownership proof stamped as the git lock reason at create time
/// (B-EXEMPT-FORGE-01): `studio:<engine_id>`. Recovery's `own_abandoned`
/// exemption unlocks+reaps a `creating` path only when the live git lock
/// reason on that worktree equals this string for OUR engine id. A forged
/// ledger row alone (reason belongs to another engine, or an old bare
/// `daemon` reason from before this change) is refused — never unlocked,
/// never reaped.
///
/// Honest boundary: a deliberate same-user attacker who re-locks with a stolen
/// engine id is out of scope (they can already `rm -rf` the victim); the
/// mechanism defeats the realistic cases — a second engine, a stale root, a
/// crash-restarted engine (same DB keeps its id), and forged rows.
pub fn lock_reason_for(engine_id: &str) -> String {
    format!("studio:{engine_id}")
}

/// Create the directory if needed, then canonicalize it (resolves symlinks).
pub fn canonicalize_or_create(p: &Path) -> Result<PathBuf, WorktreeError> {
    std::fs::create_dir_all(p).map_err(|e| WorktreeError::Io(e.to_string()))?;
    canonicalize_existing(p)
}

/// Ensure `dir` is a real directory, creating it if absent. Refuses (typed) if
/// it is a symlink or any other non-directory, and re-checks after creation so
/// a race cannot smuggle a symlink in. This is the guard against a symlinked
/// trash directory being used as a rename target outside the worktree root.
fn prepare_real_dir(dir: &Path) -> Result<(), WorktreeError> {
    // A symlinked *parent* would also carry a rename outside the root: refuse.
    if let Some(parent) = dir.parent() {
        if !parent.as_os_str().is_empty() {
            if let Ok(md) = std::fs::symlink_metadata(parent) {
                if md.file_type().is_symlink() {
                    return Err(WorktreeError::SymlinkRefused(parent.display().to_string()));
                }
            }
        }
    }
    match std::fs::symlink_metadata(dir) {
        Ok(md) if md.file_type().is_symlink() => {
            Err(WorktreeError::SymlinkRefused(dir.display().to_string()))
        }
        Ok(md) if md.file_type().is_dir() => Ok(()),
        Ok(_) => Err(WorktreeError::NotADirectory(dir.display().to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if let Some(parent) = dir.parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| WorktreeError::Io(e.to_string()))?;
                }
            }
            // `create_dir` fails (AlreadyExists) rather than silently accepting
            // an existing symlink the way `create_dir_all` would.
            std::fs::create_dir(dir).map_err(|e| WorktreeError::Io(e.to_string()))?;
            match std::fs::symlink_metadata(dir) {
                Ok(md) if md.file_type().is_dir() && !md.file_type().is_symlink() => Ok(()),
                _ => Err(WorktreeError::NotADirectory(dir.display().to_string())),
            }
        }
        Err(e) => Err(WorktreeError::Io(e.to_string())),
    }
}

pub struct WorktreeManager {
    lock: Arc<WorktreeOperationLock>,
}

impl WorktreeManager {
    pub fn new() -> Self {
        Self {
            lock: Arc::new(WorktreeOperationLock::default()),
        }
    }

    /// Hashed path layout `<root>/<hash8>/<slug>`. Idempotent: same inputs -> same path.
    pub fn path_for(root: &Path, repo_id: &str, slug: &str) -> PathBuf {
        let mut h = Sha256::new();
        h.update(repo_id.as_bytes());
        let hash = hex::encode(h.finalize());
        root.join(&hash[..8]).join(slug)
    }

    /// Trash root: hidden sibling so rename stays on one volume (orca pattern).
    pub fn trash_root(worktree_path: &Path) -> PathBuf {
        worktree_path
            .parent()
            .unwrap_or(Path::new("."))
            .join(".studio-worktree-trash")
    }

    /// Disciplined create: `git worktree add --detach --no-checkout` -> `reset --hard` -> `lock --reason`.
    /// Holds per-repo lock. Idempotent: existing path is reused after verification.
    ///
    /// Both `repo` and `path` are made absolute up front: `git worktree add`
    /// runs with `current_dir(repo)` while `git reset --hard` runs with
    /// `current_dir(path)`, so a cwd-relative `path` would resolve against two
    /// different bases and the reset would fail whenever `repo != cwd`.
    pub async fn create(
        &self,
        repo: &Path,
        path: &Path,
        rev: &str,
        reason: &str,
    ) -> Result<(), WorktreeError> {
        // C-NEWLINE-01, fail closed at creation: a control character in the
        // path (reachable via task-name slugs) or the reason cannot be
        // represented in git's porcelain and must never become a worktree.
        if has_control_chars(&path.to_string_lossy()) {
            return Err(WorktreeError::ControlCharsRefused(
                path.display().to_string(),
            ));
        }
        if has_control_chars(reason) {
            return Err(WorktreeError::ControlCharsRefused(
                "lock reason contains control characters".to_string(),
            ));
        }
        let repo = canonicalize_existing(repo)?;
        let path = normalize_path(path);
        let repo_key = repo.to_string_lossy().to_string();
        let repo_lock = self.lock.for_repo(&repo_key);
        let _guard = repo_lock.lock().await;
        match std::fs::symlink_metadata(&path) {
            Ok(md) if md.file_type().is_symlink() => {
                return Err(WorktreeError::SymlinkRefused(path.display().to_string()));
            }
            Ok(_) => return Ok(()), // idempotent reuse of a real existing path
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(WorktreeError::Io(e.to_string())),
        }
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).map_err(|e| WorktreeError::Io(e.to_string()))?;
        }
        let out = Command::new("git")
            .args(["worktree", "add", "--detach", "--no-checkout"])
            .arg(&path)
            .arg(rev)
            .current_dir(&repo)
            .output()
            .await
            .map_err(|e| WorktreeError::Io(e.to_string()))?;
        if !out.status.success() {
            return Err(WorktreeError::Git(
                String::from_utf8_lossy(&out.stderr).to_string(),
            ));
        }
        let out = Command::new("git")
            .args(["reset", "--hard", rev])
            .current_dir(&path)
            .output()
            .await
            .map_err(|e| WorktreeError::Io(e.to_string()))?;
        if !out.status.success() {
            return Err(WorktreeError::Git(
                String::from_utf8_lossy(&out.stderr).to_string(),
            ));
        }
        let out = Command::new("git")
            .args(["worktree", "lock", "--reason", reason])
            .arg(&path)
            .current_dir(&repo)
            .output()
            .await
            .map_err(|e| WorktreeError::Io(e.to_string()))?;
        if !out.status.success() {
            return Err(WorktreeError::Git(
                String::from_utf8_lossy(&out.stderr).to_string(),
            ));
        }
        Ok(())
    }

    /// Fail-closed destroy: refuses dirty state. Caller must stash to `refs/preserved/<sess>` first.
    pub async fn destroy(&self, repo: &Path, path: &Path) -> Result<(), WorktreeError> {
        let repo_key = repo.to_string_lossy().to_string();
        let repo_lock = self.lock.for_repo(&repo_key);
        let _guard = repo_lock.lock().await;
        if Self::is_dirty(path).await? {
            return Err(WorktreeError::WorkspaceDirty);
        }
        Self::unlock(repo, path).await?;
        Self::trash_rename(path)?;
        Self::prune(repo).await
    }

    /// Force destroy only after stash (caller enforces ordering).
    pub async fn force_destroy_after_stash(
        &self,
        repo: &Path,
        path: &Path,
    ) -> Result<(), WorktreeError> {
        let repo_key = repo.to_string_lossy().to_string();
        let repo_lock = self.lock.for_repo(&repo_key);
        let _guard = repo_lock.lock().await;
        Self::unlock(repo, path).await?;
        Self::trash_rename(path)?;
        Self::prune(repo).await
    }

    /// Stash uncommitted work to `refs/preserved/<sess>`.
    pub async fn stash_uncommitted(
        &self,
        path: &Path,
        sess: &str,
    ) -> Result<String, WorktreeError> {
        let target_ref = format!("refs/preserved/{sess}");
        let out = Command::new("git")
            .args(["stash", "create"])
            .current_dir(path)
            .output()
            .await
            .map_err(|e| WorktreeError::Io(e.to_string()))?;
        if !out.status.success() {
            return Err(WorktreeError::Git(
                String::from_utf8_lossy(&out.stderr).to_string(),
            ));
        }
        let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if sha.is_empty() {
            return Ok(String::new()); // nothing to stash
        }
        let out = Command::new("git")
            .args(["update-ref", &target_ref, &sha])
            .current_dir(path)
            .output()
            .await
            .map_err(|e| WorktreeError::Io(e.to_string()))?;
        if !out.status.success() {
            return Err(WorktreeError::Git(
                String::from_utf8_lossy(&out.stderr).to_string(),
            ));
        }
        Ok(target_ref)
    }

    async fn unlock(repo: &Path, path: &Path) -> Result<(), WorktreeError> {
        // Best-effort unlock so prune can reap the admin entry (prune skips locked).
        let _ = Command::new("git")
            .args(["worktree", "unlock"])
            .arg(path)
            .current_dir(repo)
            .output()
            .await;
        Ok(())
    }

    async fn is_dirty(path: &Path) -> Result<bool, WorktreeError> {
        let out = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(path)
            .output()
            .await
            .map_err(|e| WorktreeError::Io(e.to_string()))?;
        Ok(!out.stdout.is_empty())
    }

    /// Metadata-only rename-to-trash (fast, never blocks 8-35s like `remove`).
    ///
    /// SECURITY (A4): a symlinked trash directory would make `rename` relocate
    /// the worktree *into* the link target, outside the worktree root. The trash
    /// dir is therefore required to be a real directory (never a symlink), and
    /// the source must be a real directory too.
    fn trash_rename(path: &Path) -> Result<(), WorktreeError> {
        match std::fs::symlink_metadata(path) {
            Ok(md) if md.file_type().is_symlink() => {
                return Err(WorktreeError::SymlinkRefused(path.display().to_string()));
            }
            Ok(md) if md.file_type().is_dir() => {}
            Ok(_) => {
                return Err(WorktreeError::NotADirectory(path.display().to_string()));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()), // idempotent
            Err(e) => return Err(WorktreeError::Io(e.to_string())),
        }
        let trash_root = Self::trash_root(path);
        prepare_real_dir(&trash_root)?;
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let dest = trash_root.join(format!("wt-{ts}"));
        std::fs::rename(path, &dest).map_err(|e| WorktreeError::Io(e.to_string()))?;
        Ok(())
    }

    async fn prune(repo: &Path) -> Result<(), WorktreeError> {
        let out = Command::new("git")
            .args(["worktree", "prune"])
            .current_dir(repo)
            .output()
            .await
            .map_err(|e| WorktreeError::Io(e.to_string()))?;
        if !out.status.success() {
            return Err(WorktreeError::Git(
                String::from_utf8_lossy(&out.stderr).to_string(),
            ));
        }
        Ok(())
    }

    /// Crash recovery: `git worktree list --porcelain` minus known ledger paths = orphans.
    /// Unlocks + prunes orphans (idempotent). Returns what it reaped plus the
    /// orphans it **refused** because git reports them as `locked` (LOCK-B-iii).
    ///
    /// Every path is normalized to an absolute, lexical form before comparison:
    /// the ledger may hold a path built from a relative `--worktree-root` while
    /// git reports an absolute path, and comparing the two raw would classify a
    /// live, locked worktree as an orphan.
    ///
    /// `own_abandoned` is the set of paths of half-created `creating` rows from
    /// *this* engine's own ledger. B-EXEMPT-FORGE-01: ledger content is not a
    /// trust anchor, so those are unlocked+reaped ONLY when the LIVE git lock
    /// reason on the worktree equals `studio:<own_engine_id>` — proving this
    /// engine locked it. A forged row naming another engine's live worktree
    /// (foreign reason, old bare `daemon` reason, or no reason) is refused
    /// (`refused_locked`): never unlocked, never moved to trash, never pruned,
    /// and the caller must keep the row. Same DB after a restart keeps its
    /// engine id, so our own previous-run half-creates still reap.
    ///
    /// C-NEWLINE-01: an entry whose path contains control characters is refused
    /// (`refused_control`) before any other check — never unlocked, never
    /// reaped — even if a ledger row names it.
    ///
    /// C-UNLOCK-TRAP-01: `persisted_refused` is this DB's stored refusal set
    /// (canonical paths). A persisted refusal is honoured BEFORE the
    /// own-abandoned exemption and before the unlocked-orphan reap: unlocking
    /// a refused path (or the worktree vanishing from git) does NOT re-arm
    /// auto-reap. Only `studio release <path>` clears it.
    pub async fn recover(
        repo: &Path,
        known: &std::collections::HashSet<String>,
        own_abandoned: &std::collections::HashSet<String>,
        own_engine_id: &str,
        persisted_refused: &std::collections::HashSet<String>,
    ) -> Result<WorktreeRecovery, WorktreeError> {
        let expected_reason = lock_reason_for(own_engine_id);
        let repo_norm = normalize_path(repo);
        let normalize_set = |set: &std::collections::HashSet<String>| {
            set.iter()
                .map(|k| normalize_path(Path::new(k)).to_string_lossy().into_owned())
                .collect::<std::collections::HashSet<String>>()
        };
        let known_norm = normalize_set(known);
        let own_norm = normalize_set(own_abandoned);
        let persisted_norm = normalize_set(persisted_refused);
        let entries = Self::list_worktree_entries(repo).await?;
        let mut result = WorktreeRecovery::default();
        for e in entries {
            let wt_norm = normalize_path(Path::new(&e.path));
            let key = wt_norm.to_string_lossy().into_owned();
            // main checkout (first entry) is never an orphan; a ledger-tracked
            // live path is skipped silently by exact match — even when it
            // contains control characters (the sweep does the same).
            if wt_norm == repo_norm || known_norm.contains(&key) {
                continue;
            }
            // C-NEWLINE-01: an UNTRACKED entry whose path contains control
            // characters is refused before any other check — never unlocked,
            // never reaped — even when a (possibly forged) ledger row names it
            // (such rows are `creating`, hence not in `known` above).
            if has_control_chars(&e.path) {
                result.refused_control.push(e.path.clone());
                continue;
            }
            // C-UNLOCK-TRAP-01: a persisted refusal wins over EVERYTHING
            // below — including our own exemption (a path we refused as
            // foreign stays refused even if our id would now match) and the
            // unlocked-orphan reap (unlocking never re-arms auto-reap).
            if persisted_norm.contains(&key) {
                result.refused_persisted.push(key);
                continue;
            }
            // LOCK-B-iii + B-EXEMPT-FORGE-01: never unlock or reap a worktree
            // git reports as `locked` — unless it is one of our own abandoned
            // half-creates AND the live lock reason proves we locked it.
            if e.locked {
                let ours = own_norm.contains(&key)
                    && e.lock_reason.as_deref() == Some(expected_reason.as_str());
                if !ours {
                    result.refused_locked.push(key);
                    continue;
                }
            }
            // orphan: path may be missing (stale lock) or present (leaked dir)
            Self::unlock(repo, &wt_norm).await?;
            if wt_norm.exists() {
                // move leaked dir to trash instead of rm -rf (deferred bounded
                // sweep). A symlinked trash dir is refused, not followed (A4).
                Self::trash_rename(&wt_norm)?;
            }
            result.reaped.push(key);
        }
        Self::prune(repo).await?;
        Ok(result)
    }

    /// `git worktree list --porcelain` with git's `locked` state per entry.
    ///
    /// C-NEWLINE-01: prefers NUL-delimited output (`--porcelain -z`, supported
    /// by the toolchain in use — verified `git 2.55.0`, exit 0), where each
    /// field is NUL-terminated so a path containing `\n` stays inside its own
    /// `worktree ` field instead of its tail masquerading as a `locked` line.
    /// Falls back to plain LF porcelain on toolchains without `-z` (stateful
    /// parse; control-char paths are still refused downstream, never reaped).
    pub async fn list_worktree_entries(repo: &Path) -> Result<Vec<WorktreeEntry>, WorktreeError> {
        let out = Command::new("git")
            .args(["worktree", "list", "--porcelain", "-z"])
            .current_dir(repo)
            .output()
            .await
            .map_err(|e| WorktreeError::Io(e.to_string()))?;
        if out.status.success() {
            return Ok(parse_worktree_porcelain_z(&out.stdout));
        }
        let out = Command::new("git")
            .args(["worktree", "list", "--porcelain"])
            .current_dir(repo)
            .output()
            .await
            .map_err(|e| WorktreeError::Io(e.to_string()))?;
        if !out.status.success() {
            return Err(WorktreeError::Git(
                String::from_utf8_lossy(&out.stderr).to_string(),
            ));
        }
        Ok(parse_worktree_porcelain(&String::from_utf8_lossy(
            &out.stdout,
        )))
    }

    pub async fn list_worktrees(repo: &Path) -> Result<Vec<String>, WorktreeError> {
        Ok(Self::list_worktree_entries(repo)
            .await?
            .into_iter()
            .map(|e| e.path)
            .collect())
    }
}

/// One entry from `git worktree list --porcelain`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeEntry {
    /// Worktree path exactly as git reported it.
    pub path: String,
    /// git reports this worktree as `locked` (porcelain `locked` line).
    pub locked: bool,
    /// The lock reason, when git recorded one.
    pub lock_reason: Option<String>,
}

/// Outcome of a git-orphan recovery pass.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct WorktreeRecovery {
    /// Orphans unlocked (when needed) and moved to trash / pruned.
    pub reaped: Vec<String>,
    /// Orphans git reports as `locked` that are not ours: refused, never
    /// unlocked, never moved, never pruned (LOCK-B-iii, B-EXEMPT-FORGE-01).
    pub refused_locked: Vec<String>,
    /// Entries whose path contains control characters: refused before any
    /// other check, never unlocked, never reaped (C-NEWLINE-01).
    pub refused_control: Vec<String>,
    /// Entries refused SOLELY because a previous gc persisted a refusal for
    /// them (C-UNLOCK-TRAP-01): currently unlocked (or unregistered) but never
    /// auto-reaped — unlocking never re-arms auto-reap. Only an explicit
    /// `studio release <path>` clears this.
    pub refused_persisted: Vec<String>,
}

/// Parse `git worktree list --porcelain`. Blocks are separated by a blank line;
/// a `locked [<reason>]` line marks the worktree as locked.
///
/// Stateful by record: attribute lines only apply to the current `worktree `
/// record — a bare `locked` line with no open record is ignored, never turned
/// into a phantom entry. (A path containing `\n` still defeats LF porcelain —
/// its tail *is* parsed as a record line — which is why `-z` is preferred and
/// control-char paths are refused downstream. The fallback therefore never
/// invents an entry out of nothing: only `worktree ` lines create records.)
fn parse_worktree_porcelain(stdout: &str) -> Vec<WorktreeEntry> {
    parse_porcelain_fields(stdout.split('\n').map(|l| l.to_string()))
}

/// Parse NUL-delimited `git worktree list --porcelain -z` output. Fields are
/// split on NUL (which cannot appear in a path), so a path containing `\n`
/// stays verbatim inside its own `worktree ` field; empty fields are the
/// record separators (git emits `\0\0` between records). A `locked [...]`
/// field marks the current record locked — including git's C-quoted form
/// `locked "evil\nreason"` (one field, quotes preserved in the reason): any
/// remainder after `locked` still counts as locked, and only the exact reason
/// `studio:<own engine id>` ever authorizes the exemption.
fn parse_worktree_porcelain_z(stdout: &[u8]) -> Vec<WorktreeEntry> {
    parse_porcelain_fields(
        stdout
            .split(|b| *b == 0)
            .map(|f| String::from_utf8_lossy(f).into_owned()),
    )
}

/// Shared state machine over already-delimited fields (LF lines or NUL fields).
fn parse_porcelain_fields(fields: impl Iterator<Item = String>) -> Vec<WorktreeEntry> {
    let mut out: Vec<WorktreeEntry> = Vec::new();
    let mut cur: Option<WorktreeEntry> = None;
    for raw in fields {
        // Defensive: never let a stray CR turn `locked` into a miss.
        let field = raw.strip_suffix('\r').unwrap_or(&raw);
        // `worktree ` always opens a new record — even when the path remainder
        // contains control characters (verbatim under `-z`); such records are
        // refused downstream, never silently truncated into a phantom.
        if let Some(rest) = field.strip_prefix("worktree ") {
            if let Some(e) = cur.take() {
                out.push(e);
            }
            cur = Some(WorktreeEntry {
                path: rest.to_string(),
                locked: false,
                lock_reason: None,
            });
        } else if field == "locked" {
            if let Some(e) = cur.as_mut() {
                e.locked = true;
            }
        } else if let Some(reason) = field.strip_prefix("locked ") {
            if let Some(e) = cur.as_mut() {
                e.locked = true;
                e.lock_reason = Some(reason.to_string());
            }
        } else if field.is_empty() {
            if let Some(e) = cur.take() {
                out.push(e);
            }
        }
        // Every other line (HEAD/branch/detached/bare/prunable, or a stray
        // path-continuation fragment under plain LF porcelain) is ignored:
        // it never creates or marks a record.
    }
    if let Some(e) = cur.take() {
        out.push(e);
    }
    out
}

/// LOCK-B-iii: true when git reports the worktree **at `path`** as `locked`.
///
/// `git worktree list --porcelain` is run with `path` as the working directory,
/// so a locked worktree belonging to a *different* repo/engine is detected too:
/// the recovering engine's own repo listing would not mention a foreign
/// worktree at all. A path that is not a git worktree (git exits non-zero) is
/// not locked.
pub fn locked_worktree_at(path: &Path) -> bool {
    worktree_entry_at(path).map(|e| e.locked).unwrap_or(false)
}

/// The git-registered worktree whose path is exactly `path`, if any.
///
/// `git worktree list --porcelain` is run with `path` as the working directory,
/// so a worktree registered to a *different* repo is found as well. `None` when
/// `path` is not a git worktree (git fails) or the exact path is not listed (a
/// subdirectory of a worktree is not itself one).
pub fn worktree_entry_at(path: &Path) -> Option<WorktreeEntry> {
    // C-NEWLINE-01: NUL-delimited first (see `list_worktree_entries`), plain
    // LF porcelain as fallback. `None` when `path` is not a git worktree (git
    // fails both ways) or the exact path is not listed.
    let z = std::process::Command::new("git")
        .args(["worktree", "list", "--porcelain", "-z"])
        .current_dir(path)
        .output()
        .ok()?;
    let entries = if z.status.success() {
        parse_worktree_porcelain_z(&z.stdout)
    } else {
        let out = std::process::Command::new("git")
            .args(["worktree", "list", "--porcelain"])
            .current_dir(path)
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        parse_worktree_porcelain(&String::from_utf8_lossy(&out.stdout))
    };
    let target = normalize_path(path);
    entries
        .into_iter()
        .find(|e| normalize_path(Path::new(&e.path)) == target)
}

impl Default for WorktreeManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashed_layout_is_stable_and_distinct() {
        // sha256("repoA")[..8] == f4f3cc83: pinned so a change to the hash or the
        // layout actually fails this test (the previous version compared
        // `path_for` to itself and could not fail).
        let a = WorktreeManager::path_for(Path::new("/wt"), "repoA", "slug");
        assert_eq!(a, Path::new("/wt/f4f3cc83/slug"));
        let b = WorktreeManager::path_for(Path::new("/wt"), "repoB", "slug");
        assert_ne!(a, b, "different repo ids must not collide");
        let c = WorktreeManager::path_for(Path::new("/wt"), "repoA", "other");
        assert_ne!(a, c, "different slugs must not collide");
    }

    #[test]
    fn normalize_path_absolutizes_and_folds_dotdot() {
        let rel = normalize_path(Path::new("a/./b/../c"));
        assert!(rel.is_absolute(), "{rel:?} must be absolute");
        assert!(rel.ends_with("a/c"), "{rel:?}");
    }

    // --- C-NEWLINE-01: porcelain hardening ---

    #[test]
    fn plain_porcelain_ignores_a_bare_locked_line_with_no_record() {
        // A stray `locked` line with no open `worktree ` record must never
        // invent a phantom entry.
        let entries = parse_worktree_porcelain("locked\n\nworktree /r/a\nHEAD abc\n\n");
        assert_eq!(entries.len(), 1, "{entries:?}");
        assert_eq!(entries[0].path, "/r/a");
        assert!(!entries[0].locked);
    }

    #[test]
    fn plain_porcelain_parses_a_c_quoted_lock_reason_as_locked() {
        // QA-B C2: git C-quotes a lock reason containing a newline onto one
        // line (`locked "evil\nreason"`). It must still parse as locked.
        let out = "worktree /r\nHEAD abc\nbranch refs/heads/m\n\nworktree /r/w\nHEAD abc\ndetached\nlocked \"evil\\nreason\"\n\n";
        let entries = parse_worktree_porcelain(out);
        assert_eq!(entries.len(), 2, "{entries:?}");
        assert!(!entries[0].locked);
        assert!(entries[1].locked, "{entries:?}");
        assert_eq!(
            entries[1].lock_reason.as_deref(),
            Some("\"evil\\nreason\""),
            "{entries:?}"
        );
    }

    #[test]
    fn z_porcelain_keeps_a_newline_path_whole_with_no_phantom() {
        // `evil\nlocked` under `-z`: the path stays verbatim inside its own
        // NUL-terminated `worktree ` field — no truncated `.../evil` phantom,
        // no phantom `locked` mark.
        let mut bytes = Vec::new();
        for f in [
            "worktree /r",
            "HEAD abc",
            "branch refs/heads/m",
            "",
            "worktree /r/wt/evil\nlocked",
            "HEAD abc",
            "detached",
            "",
        ] {
            bytes.extend_from_slice(f.as_bytes());
            bytes.push(0);
        }
        bytes.push(0);
        let entries = parse_worktree_porcelain_z(&bytes);
        assert_eq!(entries.len(), 2, "{entries:?}");
        assert_eq!(entries[1].path, "/r/wt/evil\nlocked", "{entries:?}");
        assert!(
            !entries[1].locked,
            "the path tail must not masquerade as a lock: {entries:?}"
        );
        assert!(
            !entries.iter().any(|e| e.path == "/r/wt/evil"),
            "no truncated phantom may exist: {entries:?}"
        );
    }

    #[test]
    fn z_porcelain_keeps_a_verbatim_newline_lock_reason() {
        // Under `-z` git emits the reason verbatim (`locked evil\nreason` as
        // ONE NUL-terminated field). Still locked; the reason never equals a
        // `studio:<id>` proof, so it can never authorize the exemption.
        let mut bytes = Vec::new();
        for f in [
            "worktree /r/w",
            "HEAD abc",
            "detached",
            "locked evil\nreason",
            "",
        ] {
            bytes.extend_from_slice(f.as_bytes());
            bytes.push(0);
        }
        let entries = parse_worktree_porcelain_z(&bytes);
        assert_eq!(entries.len(), 1, "{entries:?}");
        assert!(entries[0].locked);
        assert_eq!(
            entries[0].lock_reason.as_deref(),
            Some("evil\nreason"),
            "{entries:?}"
        );
        assert_ne!(
            entries[0].lock_reason.as_deref(),
            Some(lock_reason_for("anything").as_str())
        );
    }

    #[tokio::test]
    async fn create_refuses_a_path_with_control_characters() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let out = std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(out.status.success());
        let wm = WorktreeManager::new();
        // Slug derived from a task name containing a newline (reachable input).
        let evil = dir.path().join("wt").join("evil\nlocked");
        let err = wm
            .create(&repo, &evil, "HEAD", "studio:x")
            .await
            .unwrap_err();
        assert!(
            matches!(err, WorktreeError::ControlCharsRefused(_)),
            "{err:?}"
        );
        assert!(!evil.exists(), "nothing may be created for a refused path");
        // A control-char lock reason is refused too.
        let ok_path = dir.path().join("wt").join("fine");
        let err = wm
            .create(&repo, &ok_path, "HEAD", "bad\nreason")
            .await
            .unwrap_err();
        assert!(
            matches!(err, WorktreeError::ControlCharsRefused(_)),
            "{err:?}"
        );
    }

    #[test]
    fn trash_rename_moves_worktree_to_sibling_trash_dir() {
        let dir = tempfile::tempdir().unwrap();
        let wt = dir.path().join("bucket").join("wt-0");
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(wt.join("f"), "x").unwrap();

        WorktreeManager::trash_rename(&wt).unwrap();
        assert!(!wt.exists(), "worktree must be moved out of the way");
        let trash = WorktreeManager::trash_root(&wt);
        assert_eq!(
            std::fs::read_dir(&trash).unwrap().count(),
            1,
            "one trashed entry expected under {}",
            trash.display()
        );

        // Idempotent on an already-missing path.
        WorktreeManager::trash_rename(&wt).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn trash_rename_refuses_a_symlinked_trash_dir() {
        let dir = tempfile::tempdir().unwrap();
        let victim = dir.path().join("victim");
        std::fs::create_dir_all(&victim).unwrap();
        let wt = dir.path().join("bucket").join("wt-0");
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(wt.join("f"), "x").unwrap();

        // A4: the trash dir is a symlink to a victim outside the root. Renaming
        // through it would relocate the live worktree INTO the victim.
        std::os::unix::fs::symlink(&victim, WorktreeManager::trash_root(&wt)).unwrap();

        let err = WorktreeManager::trash_rename(&wt).unwrap_err();
        assert!(matches!(err, WorktreeError::SymlinkRefused(_)), "{err:?}");
        assert!(
            wt.exists(),
            "worktree must not be relocated through the symlink"
        );
        assert_eq!(
            std::fs::read_dir(&victim).unwrap().count(),
            0,
            "victim must remain empty"
        );
    }
}
