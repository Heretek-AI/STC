//! Process-exclusive FS/git lock for the worktree engine (D4, reworked for B).
//!
//! SQLite serializes the *database* half of the daemon with `BEGIN IMMEDIATE`,
//! but the filesystem + git half (creating/trashing worktrees, `git worktree
//! add/prune`) has no such protection.
//!
//! The lock must be keyed to the **shared resource**, not the database path.
//! The guarded resource is the repo (its `.git` admin dir) plus the worktree
//! root; both are shared across databases. Keying on `<db>.studio-lock` allowed
//! three failures (defect B):
//! - **B(i)** `rm -f <db>.studio-lock` unlinked the file and the next daemon
//!   recreated a *new inode*, so its `flock` succeeded while the first ran.
//! - **B(ii)** two DBs on one repo/root ran concurrently; the second's boot
//!   recovery treated the first's live, git-locked `ready` worktrees as orphans
//!   and deleted them.
//! - **B(iii)** `gc --db dbA` (exit 0) reaped a live daemon-B's worktrees.
//!
//! [`StudioLock`] instead takes a non-blocking `flock(2)` on two **directories
//! that are never replaced during normal operation**: `<repo>/.git` and the
//! worktree root. There is no lock *file* to unlink-and-recreate, so B(i)'s
//! bypass no longer exists. The handles are held open for the whole process
//! lifetime (per the SQLite corruption notes, VERIFIED `988dd43e...`, `close()`
//! cancels POSIX advisory locks), and `flock` is per open-file-description, so a
//! second `File::open` — even in the same process — is refused.
//!
//! `studio gc` takes the same lock, which makes GC daemon-exclusive.
//!
//! ## Residual: directory-inode replacement (LOCK-B-iii-01)
//!
//! `flock` is keyed to an inode. A same-user process can `mv shared shared.old;
//! mkdir shared` and hand a *new* inode to a second engine. Replacing just one
//! inode does **not** admit a second engine: the lock is taken on **two**
//! independent resources, `<repo>/.git` and the worktree root, so the other
//! holds (pinned by
//! `replacing_the_root_directory_inode_does_not_grant_a_second_engine`).
//!
//! Replacing **both** inodes — or copying `.git` into a fresh repo — *does*
//! admit a second engine; that is inherent to any inode-based lock. QA-B (r4)
//! demonstrated this is destructive: a second engine with an empty ledger swept
//! the first engine's live, git-`locked` `ready` worktrees
//! (`/tmp/opencode/qa-b4/B3.sh`, `B4.sh`; the first daemon then crashed on its
//! vanished worktree). The destruction is closed *independently of the lock*:
//! recovery now refuses to reap any worktree git reports as `locked`
//! (LOCK-B-iii, see `recovery`), and the sweep is fd-anchored
//! (SEC-A-TOCTOU-01), so no concurrent process can make `gc` follow a symlink
//! out of the root. We still do not attempt to over-lock ancestors (that would
//! refuse unrelated projects sharing a parent directory).

use std::fs::File;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum LockError {
    #[error("another studio engine already owns this repo/worktree root (lock held: {0})")]
    AlreadyHeld(String),
    #[error("io: {0}")]
    Io(String),
    #[error("not a git repository (no .git): {0}")]
    NotARepo(String),
    #[error("refusing to lock non-directory resource: {0}")]
    NotADirectory(String),
}

/// An exclusive lock guarding daemon FS/git mutations and `studio gc`, keyed to
/// the shared repo + worktree root.
pub struct StudioLock {
    /// Held for the process lifetime; dropping releases the locks.
    _repo: File,
    _root: File,
    resource: String,
}

impl std::fmt::Debug for StudioLock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StudioLock")
            .field("resource", &self.resource)
            .finish()
    }
}

impl StudioLock {
    /// Acquire the engine lock for the shared `repo` + `worktree_root`. Fails
    /// closed with [`LockError::AlreadyHeld`] if any other engine holds it.
    pub fn acquire(repo: &Path, worktree_root: &Path) -> Result<Self, LockError> {
        let repo = repo
            .canonicalize()
            .map_err(|e| LockError::Io(format!("{}: {e}", repo.display())))?;
        let git = repo.join(".git");
        match std::fs::symlink_metadata(&git) {
            Ok(_) => {}
            Err(_) => return Err(LockError::NotARepo(repo.display().to_string())),
        }
        let root = crate::worktree::canonicalize_or_create(worktree_root)
            .map_err(|e| LockError::Io(e.to_string()))?;
        if std::fs::symlink_metadata(&root)
            .map(|m| !m.file_type().is_dir())
            .unwrap_or(false)
        {
            return Err(LockError::NotADirectory(root.display().to_string()));
        }

        // Deterministic order (repo, then root); LOCK_NB means no deadlock.
        let repo_handle =
            File::open(&git).map_err(|e| LockError::Io(format!("{}: {e}", git.display())))?;
        Self::lock_exclusive(&repo_handle, &git)?;
        let root_handle =
            File::open(&root).map_err(|e| LockError::Io(format!("{}: {e}", root.display())))?;
        Self::lock_exclusive(&root_handle, &root)?;

        Ok(Self {
            _repo: repo_handle,
            _root: root_handle,
            resource: format!("repo {} + root {}", git.display(), root.display()),
        })
    }

    /// Human-readable description of the locked resource.
    pub fn resource(&self) -> &str {
        &self.resource
    }

    /// Path-like helper kept for diagnostics.
    pub fn repo_git_dir(repo: &Path) -> PathBuf {
        repo.join(".git")
    }

    #[cfg(unix)]
    fn lock_exclusive(file: &File, path: &Path) -> Result<(), LockError> {
        use std::os::unix::io::AsRawFd;
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if rc == 0 {
            return Ok(());
        }
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::EWOULDBLOCK) {
            return Err(LockError::AlreadyHeld(path.display().to_string()));
        }
        Err(LockError::Io(format!("flock {}: {err}", path.display())))
    }

    /// Non-Unix fallback: create-new marker semantics (best effort).
    #[cfg(not(unix))]
    fn lock_exclusive(_file: &File, path: &Path) -> Result<(), LockError> {
        let marker = PathBuf::from(format!("{}.studio-lock", path.display()));
        if marker.exists() {
            return Err(LockError::AlreadyHeld(path.display().to_string()));
        }
        std::fs::write(&marker, b"held").map_err(|e| LockError::Io(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_repo(dir: &Path) -> PathBuf {
        let repo = dir.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let out = std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&repo)
            .output()
            .expect("git init");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        repo
    }

    #[test]
    fn second_engine_on_same_repo_and_root_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let repo = scratch_repo(dir.path());
        let root = dir.path().join("wt");
        let held = StudioLock::acquire(&repo, &root).unwrap();

        let err = StudioLock::acquire(&repo, &root).unwrap_err();
        assert!(matches!(err, LockError::AlreadyHeld(_)), "{err:?}");

        // B(i): the identity is the resource, so there is no db lockfile to
        // unlink-and-recreate. Releasing the held lock allows a new acquire.
        let fake_db_lock = dir.path().join("studio.db.studio-lock");
        std::fs::write(&fake_db_lock, b"").unwrap();
        std::fs::remove_file(&fake_db_lock).unwrap();
        assert!(matches!(
            StudioLock::acquire(&repo, &root),
            Err(LockError::AlreadyHeld(_))
        ));

        drop(held);
        // A concurrent test that forks while `held` is open can briefly pin the
        // open-file-description; poll until the lock is released.
        let mut released = false;
        for _ in 0..100 {
            if StudioLock::acquire(&repo, &root).is_ok() {
                released = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(
            released,
            "engine lock must be released when the holder drops"
        );
    }

    #[test]
    fn different_root_same_repo_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let repo = scratch_repo(dir.path());
        let _held = StudioLock::acquire(&repo, &dir.path().join("wt-a")).unwrap();
        let err = StudioLock::acquire(&repo, &dir.path().join("wt-b")).unwrap_err();
        assert!(matches!(err, LockError::AlreadyHeld(_)), "{err:?}");
    }

    #[test]
    fn different_repo_same_root_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let repo_a = scratch_repo(&dir.path().join("a"));
        let repo_b = scratch_repo(&dir.path().join("b"));
        let root = dir.path().join("wt");
        let _held = StudioLock::acquire(&repo_a, &root).unwrap();
        let err = StudioLock::acquire(&repo_b, &root).unwrap_err();
        assert!(matches!(err, LockError::AlreadyHeld(_)), "{err:?}");
    }

    #[test]
    fn non_repo_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let not_repo = dir.path().join("plain");
        std::fs::create_dir_all(&not_repo).unwrap();
        let err = StudioLock::acquire(&not_repo, &dir.path().join("wt")).unwrap_err();
        assert!(matches!(err, LockError::NotARepo(_)), "{err:?}");
    }

    /// LOCK-B-iii-01: `mv shared shared.old; mkdir shared` swaps the worktree
    /// root inode while the engine holds its flock. This pins the current,
    /// documented behaviour: the second engine is still refused because the
    /// repo `.git` lock (a separate, stable resource) is also held.
    #[test]
    fn replacing_the_root_directory_inode_does_not_grant_a_second_engine() {
        let dir = tempfile::tempdir().unwrap();
        let repo = scratch_repo(dir.path());
        let root = dir.path().join("shared");
        let held = StudioLock::acquire(&repo, &root).unwrap();

        // Swap the root directory for a fresh inode.
        std::fs::rename(&root, dir.path().join("shared.old")).unwrap();
        std::fs::create_dir(&root).unwrap();

        let err = StudioLock::acquire(&repo, &root).unwrap_err();
        assert!(
            matches!(err, LockError::AlreadyHeld(_)),
            "replacing only the root inode must not admit a second engine: {err:?}"
        );

        drop(held);
    }
}
