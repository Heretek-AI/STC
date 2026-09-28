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
    /// Unlocks + prunes orphans (idempotent). Returns reaped paths.
    ///
    /// Every path is normalized to an absolute, lexical form before comparison:
    /// the ledger may hold a path built from a relative `--worktree-root` while
    /// git reports an absolute path, and comparing the two raw would classify a
    /// live, locked worktree as an orphan.
    pub async fn recover(
        repo: &Path,
        known: &std::collections::HashSet<String>,
    ) -> Result<Vec<String>, WorktreeError> {
        let repo_norm = normalize_path(repo);
        let known_norm: std::collections::HashSet<String> = known
            .iter()
            .map(|k| normalize_path(Path::new(k)).to_string_lossy().into_owned())
            .collect();
        let listed = Self::list_worktrees(repo).await?;
        let mut reaped = vec![];
        for wt in listed {
            let wt_norm = normalize_path(Path::new(&wt));
            // main checkout (first entry) is never an orphan
            if wt_norm == repo_norm || known_norm.contains(&wt_norm.to_string_lossy().into_owned())
            {
                continue;
            }
            // orphan: path may be missing (stale lock) or present (leaked dir)
            Self::unlock(repo, &wt_norm).await?;
            if wt_norm.exists() {
                // move leaked dir to trash instead of rm -rf (deferred bounded
                // sweep). A symlinked trash dir is refused, not followed (A4).
                Self::trash_rename(&wt_norm)?;
            }
            reaped.push(wt_norm.to_string_lossy().into_owned());
        }
        Self::prune(repo).await?;
        Ok(reaped)
    }

    pub async fn list_worktrees(repo: &Path) -> Result<Vec<String>, WorktreeError> {
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
        let mut out_v = vec![];
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            if let Some(rest) = line.strip_prefix("worktree ") {
                out_v.push(rest.to_string());
            }
        }
        Ok(out_v)
    }
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
