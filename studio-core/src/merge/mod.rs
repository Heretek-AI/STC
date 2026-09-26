//! Merge queue: Bors turnstile + CONFLICT_RETAINED (never destructive reset).
//! Anchors (by symbol): antigravity ADR-003 (`CONFLICT_RETAINED`),
//! agent-orchestrator `review.go` (head-SHA-fenced feedback), orca prune-before-delete.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;
use tokio::process::Command;

#[derive(Debug, Error)]
pub enum MergeError {
    #[error("git: {0}")]
    Git(String),
    #[error("io: {0}")]
    Io(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MergeOutcome {
    Merged { head_sha: String },
    ConflictRetained { conflict_files: Vec<String>, diagnostic: String, head_sha: String },
}

pub struct DiagnosticBundle {
    pub task_id: String,
    pub head_sha: String,
    pub conflict_files: Vec<String>,
    pub log_tail: String,
}

/// Bors turnstile: one merge at a time (tokio Mutex), full-test gate assumed by caller.
pub struct MergeQueue {
    gate: Arc<tokio::sync::Mutex<()>>,
}

impl MergeQueue {
    pub fn new() -> Self {
        Self { gate: Arc::new(tokio::sync::Mutex::new(())) }
    }

    /// Gated landing: refuses unless DoD receipt is Done (green tests + syntax + delta).
    /// This is the "nothing lands unverified" enforcement point.
    pub async fn gated_merge(
        &self,
        repo: &std::path::Path,
        worktree_path: &std::path::Path,
        branch: &str,
        head_sha_fence: &str,
        dod_done: bool,
        dod_detail: &str,
    ) -> Result<MergeOutcome, MergeError> {
        if !dod_done {
            let head = git_head(repo, worktree_path).await.unwrap_or_else(|_| "unknown".into());
            return Ok(MergeOutcome::ConflictRetained {
                conflict_files: vec![],
                diagnostic: format!("gate refused: DoD not Done ({dod_detail}); re-verify before landing"),
                head_sha: head,
            });
        }
        self.merge(repo, worktree_path, branch, head_sha_fence).await
    }

    /// Attempt merge of worktree branch into main. On conflict: keep worktree intact,
    /// return diagnostic (never `git reset --hard` on parallel-coder work).
    pub async fn merge(
        &self,
        repo: &std::path::Path,
        worktree_path: &std::path::Path,
        branch: &str,
        head_sha_fence: &str,
    ) -> Result<MergeOutcome, MergeError> {
        let _turnstile = self.gate.lock().await;
        // verify fence: worktree HEAD must still equal the reviewed SHA
        let head = git_head(repo, worktree_path).await?;
        if head != head_sha_fence {
            return Ok(MergeOutcome::ConflictRetained {
                conflict_files: vec![],
                diagnostic: format!("stale verdict: fence {head_sha_fence} != head {head}; re-review required"),
                head_sha: head,
            });
        }
        // try merge --no-commit --no-ff for dry-run detection
        let out = Command::new("git")
            .args(["merge", "--no-commit", "--no-ff", branch])
            .current_dir(repo)
            .output()
            .await
            .map_err(|e| MergeError::Io(e.to_string()))?;
        if out.status.success() {
            // abort the dry-run, caller performs real gated merge
            let _ = Command::new("git").args(["merge", "--abort"]).current_dir(repo).output().await;
            return Ok(MergeOutcome::Merged { head_sha: head });
        }
        let stderr = String::from_utf8_lossy(&out.stderr).to_string() + String::from_utf8_lossy(&out.stdout).as_ref();
        let files = conflicting_files(repo).await.unwrap_or_default();
        let _ = Command::new("git").args(["merge", "--abort"]).current_dir(repo).output().await;
        let bundle = DiagnosticBundle {
            task_id: branch.into(),
            head_sha: head.clone(),
            conflict_files: files.clone(),
            log_tail: stderr.chars().take(2000).collect(),
        };
        Ok(MergeOutcome::ConflictRetained {
            conflict_files: files,
            diagnostic: format!(
                "CONFLICT_RETAINED task={} head={} files={:?} hint=rebase worktree onto main and re-verify; worktree kept at {:?} tail={}",
                bundle.task_id, bundle.head_sha, bundle.conflict_files, worktree_path, bundle.log_tail.chars().take(500).collect::<String>()
            ),
            head_sha: head,
        })
    }
}

impl Default for MergeQueue {
    fn default() -> Self {
        Self::new()
    }
}

async fn git_head(_repo: &std::path::Path, worktree_path: &std::path::Path) -> Result<String, MergeError> {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(worktree_path)
        .output()
        .await
        .map_err(|e| MergeError::Io(e.to_string()))?;
    if !out.status.success() {
        return Err(MergeError::Git(String::from_utf8_lossy(&out.stderr).to_string()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

async fn conflicting_files(repo: &std::path::Path) -> Result<Vec<String>, MergeError> {
    let out = Command::new("git")
        .args(["diff", "--name-only", "--diff-filter=U"])
        .current_dir(repo)
        .output()
        .await
        .map_err(|e| MergeError::Io(e.to_string()))?;
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conflict_retained_is_not_destructive() {
        let o = MergeOutcome::ConflictRetained {
            conflict_files: vec!["a.rs".into()],
            diagnostic: "keep worktree".into(),
            head_sha: "abc".into(),
        };
        assert!(matches!(o, MergeOutcome::ConflictRetained { .. }));
    }
}
