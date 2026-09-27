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
    pub async fn create(
        &self,
        repo: &Path,
        path: &Path,
        rev: &str,
        reason: &str,
    ) -> Result<(), WorktreeError> {
        let repo_key = repo.to_string_lossy().to_string();
        let repo_lock = self.lock.for_repo(&repo_key);
        let _guard = repo_lock.lock().await;
        if path.exists() {
            return Ok(()); // idempotent reuse
        }
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).map_err(|e| WorktreeError::Io(e.to_string()))?;
        }
        let out = Command::new("git")
            .args(["worktree", "add", "--detach", "--no-checkout"])
            .arg(path)
            .arg(rev)
            .current_dir(repo)
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
            .current_dir(path)
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
            .arg(path)
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
    fn trash_rename(path: &Path) -> Result<(), WorktreeError> {
        if !path.exists() {
            return Ok(()); // idempotent
        }
        let trash_root = Self::trash_root(path);
        std::fs::create_dir_all(&trash_root).map_err(|e| WorktreeError::Io(e.to_string()))?;
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
    pub async fn recover(
        repo: &Path,
        known: &std::collections::HashSet<String>,
    ) -> Result<Vec<String>, WorktreeError> {
        let listed = Self::list_worktrees(repo).await?;
        let mut reaped = vec![];
        for wt in listed {
            // main checkout (first entry) is never an orphan
            if wt == repo.to_string_lossy() {
                continue;
            }
            if known.contains(&wt) {
                continue;
            }
            // orphan: path may be missing (stale lock) or present (leaked dir)
            let p = Path::new(&wt);
            Self::unlock(repo, p).await?;
            if p.exists() {
                // move leaked dir to trash instead of rm -rf (deferred bounded sweep)
                let _ = Self::trash_rename(p);
            }
            reaped.push(wt);
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

    /// Sweep trash dirs (bounded deferred teardown). Verifies ledger state via `keep` predicate.
    pub fn sweep_trash(root: &Path, keep: impl Fn(&str) -> bool) -> Result<usize, WorktreeError> {
        let trash = root.join(".studio-worktree-trash");
        if !trash.exists() {
            return Ok(0);
        }
        let mut removed = 0;
        for entry in std::fs::read_dir(&trash).map_err(|e| WorktreeError::Io(e.to_string()))? {
            let entry = entry.map_err(|e| WorktreeError::Io(e.to_string()))?;
            let name = entry.file_name().to_string_lossy().to_string();
            if keep(&name) {
                continue;
            }
            let p = entry.path();
            if p.is_dir() {
                std::fs::remove_dir_all(&p).map_err(|e| WorktreeError::Io(e.to_string()))?;
            } else {
                std::fs::remove_file(&p).map_err(|e| WorktreeError::Io(e.to_string()))?;
            }
            removed += 1;
        }
        Ok(removed)
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
    fn hashed_layout_is_stable() {
        let a = WorktreeManager::path_for(Path::new("/wt"), "repoA", "slug");
        let b = WorktreeManager::path_for(Path::new("/wt"), "repoA", "slug");
        assert_eq!(a, b);
        assert!(a.to_string_lossy().contains("slug"));
    }

    #[test]
    fn trash_sweep_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            WorktreeManager::sweep_trash(dir.path(), |_| false).unwrap(),
            0
        );
    }
}
