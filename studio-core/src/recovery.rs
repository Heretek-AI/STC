//! Boot-time crash recovery: reconcile the SQLite worktree ledger with the
//! filesystem and git's worktree admin state. Idempotent — a clean database and
//! filesystem yield an all-zero report.
//!
//! Crash model: a worktree is recorded as `creating` *before* git runs, then
//! marked `ready` after `git worktree add` succeeds. A `SIGKILL` can therefore
//! leave (a) a `creating` row with a partial directory, (b) a directory git
//! knows about but the ledger has not confirmed, or (c) a directory git does
//! not know about at all. Recovery resolves all three; it never deletes work
//! merely because a reader overlapped (WAL readers are unaffected).

use crate::state::StateStore;
use crate::worktree::WorktreeManager;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RecoveryError {
    #[error("state: {0}")]
    State(#[from] crate::state::StateError),
    #[error("worktree: {0}")]
    Worktree(#[from] crate::worktree::WorktreeError),
    #[error("io: {0}")]
    Io(String),
    #[error("path has no usable file name: {0}")]
    BadPath(String),
    /// Refused a symlink: recovery never traverses or deletes through one.
    #[error("refusing to traverse symlink (never leaves the worktree root): {0}")]
    SymlinkRefused(String),
}

/// What a recovery pass changed. All-empty on a clean boot.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RecoveryReport {
    /// `creating` rows reaped (partial creates).
    pub reaped_creating: Vec<String>,
    /// Git-registered worktrees the ledger did not know about.
    pub reaped_git_orphans: Vec<String>,
    /// Ledger rows whose directory had already vanished.
    pub dropped_missing_rows: Vec<String>,
    /// Leftover directories under the worktree root not tracked by the ledger.
    pub swept_dirs: Vec<String>,
    /// Symlinks encountered under the root and refused (never traversed/deleted).
    pub refused_symlinks: Vec<String>,
    pub integrity_ok: bool,
    pub integrity_detail: String,
}

impl RecoveryReport {
    pub fn clean(&self) -> bool {
        self.integrity_ok
            && self.reaped_creating.is_empty()
            && self.reaped_git_orphans.is_empty()
            && self.dropped_missing_rows.is_empty()
            && self.swept_dirs.is_empty()
            && self.refused_symlinks.is_empty()
    }
}

/// Run GC. Safe to call repeatedly; the second call on a clean tree is a no-op.
///
/// Order matters for one-pass convergence. Ledger rows whose directory has
/// vanished are dropped **before** git orphans are classified, so the stale git
/// admin entry left behind by a vanished directory is seen as an orphan in the
/// same pass (one `gc` converges; `verify` then exits 0).
pub async fn recover(
    store: &StateStore,
    repo: &Path,
    worktree_root: &Path,
) -> Result<RecoveryReport, RecoveryError> {
    let repo = canonical(repo)?;
    let root = crate::worktree::canonicalize_or_create(worktree_root)
        .map_err(|e| RecoveryError::Io(e.to_string()))?;
    let mut report = RecoveryReport::default();

    report.integrity_detail = store.integrity_check()?;
    report.integrity_ok = report.integrity_detail == "ok";

    // (a) Reap half-created worktrees: drop the ledger row; the directory, if
    // any, is removed by the sweep below.
    for path in store.worktrees_in_state("creating")? {
        store.delete_worktree(&path)?;
        report.reaped_creating.push(norm_str(&path));
    }

    // (b) Ledger row whose directory already vanished. This must run before the
    // git pass so the now-untracked git entry is classified as an orphan.
    for (raw, _state) in store.worktree_rows()? {
        let norm = norm_str(&raw);
        if !Path::new(&norm).exists() {
            store.delete_worktree(&raw)?;
            report.dropped_missing_rows.push(norm);
        }
    }

    // (c) Git knows about it, the ledger does not: unlock + prune + trash.
    let known: HashSet<String> = store.worktree_paths()?.into_iter().collect();
    report.reaped_git_orphans = WorktreeManager::recover(&repo, &known).await?;

    // (d) Sweep every *real* directory under the root that the ledger does not
    // track (leaked dirs, emptied buckets, nested trash). Bounded: depth 2.
    //
    // SECURITY (A3): never follow a symlink. Rust's `remove_dir_all` refuses a
    // symlink only as the *final* component; an intermediate symlink is followed
    // and the subtree outside the root is destroyed. Here every entry is
    // classified with `symlink_metadata` first: a symlink bucket or child is
    // refused and reported, and only real directories are ever removed.
    for (bucket, kind) in read_children(&root)? {
        match kind {
            ChildKind::Symlink => {
                report.refused_symlinks.push(path_str(&bucket));
                continue;
            }
            // Only directories are buckets; loose files directly under the root
            // were never swept and still are not.
            ChildKind::RealFile => continue,
            ChildKind::RealDir => {}
        }
        for (entry, kind) in read_children(&bucket)? {
            if matches!(kind, ChildKind::Symlink) {
                report.refused_symlinks.push(path_str(&entry));
                continue;
            }
            let norm = path_str(&entry);
            if known.contains(&norm) {
                continue;
            }
            if subtree_has_symlink(&entry)? {
                report.refused_symlinks.push(norm);
            } else {
                remove_tree_no_follow(&entry)?;
                report.swept_dirs.push(norm);
            }
        }
        // Remove the bucket if the sweep emptied it.
        if read_children(&bucket)?.is_empty() {
            let _ = std::fs::remove_dir(&bucket);
        }
    }

    Ok(report)
}

/// Read-only integrity report. Never mutates; used by `studio verify`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct IntegrityReport {
    pub integrity_ok: bool,
    pub integrity_detail: String,
    pub creating_rows: usize,
    pub missing_rows: usize,
    /// Rows whose `state` is not `creating`/`ready` — corruption.
    pub invalid_state_rows: usize,
    /// Rows whose path is not contained in the worktree root (escape).
    pub outside_root_rows: Vec<String>,
    /// Rows whose recorded `repo` is not the repo being verified (wrong repo/DB).
    pub repo_mismatch_rows: Vec<String>,
    /// Rows whose path exists but is not a real directory (file or symlink).
    pub non_directory_rows: Vec<String>,
    /// Rows whose directory exists but is empty (not a usable worktree).
    pub empty_rows: Vec<String>,
    /// Rows whose directory is not registered with git as a worktree.
    pub not_a_worktree_rows: Vec<String>,
    /// Symlinks found under the root; refused, never traversed.
    pub refused_symlinks: Vec<String>,
    pub git_orphans: Vec<String>,
    pub leftover_dirs: Vec<String>,
}

impl IntegrityReport {
    pub fn clean(&self) -> bool {
        self.integrity_ok
            && self.creating_rows == 0
            && self.missing_rows == 0
            && self.invalid_state_rows == 0
            && self.outside_root_rows.is_empty()
            && self.repo_mismatch_rows.is_empty()
            && self.non_directory_rows.is_empty()
            && self.empty_rows.is_empty()
            && self.not_a_worktree_rows.is_empty()
            && self.refused_symlinks.is_empty()
            && self.git_orphans.is_empty()
            && self.leftover_dirs.is_empty()
    }
}

pub async fn verify(
    store: &StateStore,
    repo: &Path,
    worktree_root: &Path,
) -> Result<IntegrityReport, RecoveryError> {
    let repo = canonical(repo)?;
    let root = crate::worktree::canonicalize_or_create(worktree_root)
        .map_err(|e| RecoveryError::Io(e.to_string()))?;
    let mut report = IntegrityReport::default();

    report.integrity_detail = store.integrity_check()?;
    report.integrity_ok = report.integrity_detail == "ok";
    report.creating_rows = store.worktrees_in_state("creating")?.len();
    // D6: any state other than the known-good set is unclean.
    report.invalid_state_rows = store.invalid_state_rows()?;

    let known: HashSet<String> = store.worktree_paths()?.into_iter().collect();
    let repo_norm = path_str(&repo);
    let main = repo_norm.clone();
    let listed = WorktreeManager::list_worktrees(&repo).await?;
    let listed_norm: HashSet<String> = listed.iter().map(|p| norm_str(p)).collect();

    // D: every ledger row must be inside the root, belong to this repo, be a
    // real directory, be a registered git worktree, and be non-empty. Each
    // failure is reported under its own typed class — a row that is none of
    // those must never read as `clean`.
    for rec in store.worktree_records()? {
        let norm = norm_str(&rec.path);
        if !Path::new(&norm).starts_with(&root) {
            report.outside_root_rows.push(norm.clone());
        }
        if norm_str(&rec.repo) != repo_norm {
            report.repo_mismatch_rows.push(norm.clone());
        }
        let md = match std::fs::symlink_metadata(&norm) {
            Ok(m) => m,
            Err(_) => {
                report.missing_rows += 1;
                continue;
            }
        };
        let ft = md.file_type();
        if ft.is_symlink() {
            report.refused_symlinks.push(norm.clone());
        }
        if !ft.is_dir() {
            report.non_directory_rows.push(norm.clone());
            continue;
        }
        if !listed_norm.contains(&norm) {
            report.not_a_worktree_rows.push(norm.clone());
        }
        if read_children(Path::new(&norm))?.is_empty() {
            report.empty_rows.push(norm);
        }
    }

    for wt in &listed {
        let norm = norm_str(wt);
        if norm == main || known.contains(&norm) {
            continue;
        }
        report.git_orphans.push(norm);
    }

    for (bucket, kind) in read_children(&root)? {
        if matches!(kind, ChildKind::Symlink) {
            report.refused_symlinks.push(path_str(&bucket));
            continue;
        }
        if !matches!(kind, ChildKind::RealDir) {
            continue;
        }
        for (p, kind) in read_children(&bucket)? {
            if matches!(kind, ChildKind::Symlink) {
                report.refused_symlinks.push(path_str(&p));
                continue;
            }
            let norm = path_str(&p);
            if !known.contains(&norm) {
                report.leftover_dirs.push(norm);
            }
        }
    }
    Ok(report)
}

fn canonical(p: &Path) -> Result<PathBuf, RecoveryError> {
    p.canonicalize()
        .map_err(|e| RecoveryError::Io(format!("{}: {e}", p.display())))
}

/// Absolute, lexically-normalized form of a path string.
fn norm_str(s: &str) -> String {
    crate::worktree::normalize_path(Path::new(s))
        .to_string_lossy()
        .into_owned()
}

fn path_str(p: &Path) -> String {
    crate::worktree::normalize_path(p)
        .to_string_lossy()
        .into_owned()
}

/// A directory entry classified by `symlink_metadata` — never following a link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChildKind {
    RealDir,
    RealFile,
    Symlink,
}

fn classify(p: &Path) -> Result<ChildKind, RecoveryError> {
    let md = std::fs::symlink_metadata(p).map_err(|e| RecoveryError::Io(e.to_string()))?;
    let ft = md.file_type();
    Ok(if ft.is_symlink() {
        ChildKind::Symlink
    } else if ft.is_dir() {
        ChildKind::RealDir
    } else {
        ChildKind::RealFile
    })
}

/// Immediate children of `dir`, each classified with `lstat` (no symlink follow).
fn read_children(dir: &Path) -> Result<Vec<(PathBuf, ChildKind)>, RecoveryError> {
    let mut out = vec![];
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(RecoveryError::Io(e.to_string())),
    };
    for entry in entries {
        let entry = entry.map_err(|e| RecoveryError::Io(e.to_string()))?;
        let path = entry.path();
        let kind = classify(&path)?;
        out.push((path, kind));
    }
    Ok(out)
}

/// True when `p` or any descendant is a symlink. Used to refuse before deleting
/// so a symlinked subtree is never partially removed.
fn subtree_has_symlink(p: &Path) -> Result<bool, RecoveryError> {
    match classify(p)? {
        ChildKind::Symlink => Ok(true),
        ChildKind::RealFile => Ok(false),
        ChildKind::RealDir => {
            for (child, _) in read_children(p)? {
                if subtree_has_symlink(&child)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
    }
}

/// Recursively remove `p`, refusing (typed) at the first symlink encountered.
/// `remove_dir_all` cannot be used: it follows intermediate symlinks.
fn remove_tree_no_follow(p: &Path) -> Result<(), RecoveryError> {
    match classify(p) {
        Err(_) => Ok(()), // already gone
        Ok(ChildKind::Symlink) => Err(RecoveryError::SymlinkRefused(path_str(p))),
        Ok(ChildKind::RealFile) => {
            std::fs::remove_file(p).map_err(|e| RecoveryError::Io(e.to_string()))
        }
        Ok(ChildKind::RealDir) => {
            for (child, kind) in read_children(p)? {
                if matches!(kind, ChildKind::Symlink) {
                    return Err(RecoveryError::SymlinkRefused(path_str(&child)));
                }
                remove_tree_no_follow(&child)?;
            }
            std::fs::remove_dir(p).map_err(|e| RecoveryError::Io(e.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(repo: &Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn scratch_repo(dir: &Path) -> PathBuf {
        let repo = dir.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q"]);
        git(&repo, &["config", "user.email", "t@example.com"]);
        git(&repo, &["config", "user.name", "test"]);
        std::fs::write(repo.join("README.md"), "x\n").unwrap();
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-q", "-m", "init"]);
        repo
    }

    #[tokio::test]
    async fn recovery_is_idempotent_on_clean_tree() {
        let dir = tempfile::tempdir().unwrap();
        let repo = scratch_repo(dir.path());
        let root = dir.path().join("wt");
        let store = StateStore::open(dir.path().join("studio.db").to_str().unwrap()).unwrap();

        let first = recover(&store, &repo, &root).await.unwrap();
        assert!(first.clean(), "expected clean first pass: {first:?}");
        let second = recover(&store, &repo, &root).await.unwrap();
        assert_eq!(first, second, "second pass must be a no-op");
    }

    #[tokio::test]
    async fn recovery_reaps_a_half_created_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let repo = scratch_repo(dir.path());
        let root = dir.path().join("wt");
        let store = StateStore::open(dir.path().join("studio.db").to_str().unwrap()).unwrap();

        // Simulate a crash mid-create: the ledger recorded intent, git never ran.
        let wt_path = WorktreeManager::path_for(&root, "scratch", "half");
        std::fs::create_dir_all(&wt_path).unwrap();
        store
            .begin_worktree(&wt_path.to_string_lossy(), "scratch", "half")
            .unwrap();

        let report = recover(&store, &repo, &root).await.unwrap();
        assert!(report.reaped_creating.iter().any(|p| p.ends_with("half")));
        assert!(report.swept_dirs.iter().any(|p| p.ends_with("half")));
        assert!(store.worktree_paths().unwrap().is_empty());
        assert!(!wt_path.exists());

        // Idempotent on the repaired tree.
        let again = recover(&store, &repo, &root).await.unwrap();
        assert!(again.clean(), "post-repair pass must be clean: {again:?}");
    }

    #[tokio::test]
    async fn verify_flags_and_then_clears_an_orphan() {
        let dir = tempfile::tempdir().unwrap();
        let repo = scratch_repo(dir.path());
        let root = dir.path().join("wt");
        let store = StateStore::open(dir.path().join("studio.db").to_str().unwrap()).unwrap();

        let orphan = WorktreeManager::path_for(&root, "scratch", "orphan");
        std::fs::create_dir_all(&orphan).unwrap();

        let bad = verify(&store, &repo, &root).await.unwrap();
        assert!(!bad.clean());
        assert_eq!(bad.leftover_dirs.len(), 1);

        recover(&store, &repo, &root).await.unwrap();
        let good = verify(&store, &repo, &root).await.unwrap();
        assert!(good.clean(), "expected clean after recovery: {good:?}");
    }

    #[tokio::test]
    async fn one_gc_pass_converges_after_directory_vanishes() {
        let dir = tempfile::tempdir().unwrap();
        let repo = scratch_repo(dir.path());
        let root = dir.path().join("wt");
        let store = StateStore::open(dir.path().join("studio.db").to_str().unwrap()).unwrap();

        // A `ready` ledger row whose directory vanished while git still holds
        // the admin entry: exactly the D2 repro.
        let wt = WorktreeManager::path_for(&root, "scratch", "gone");
        std::fs::create_dir_all(&wt).unwrap();
        store
            .begin_worktree(&wt.to_string_lossy(), "scratch", "gone")
            .unwrap();
        store.mark_worktree_ready(&wt.to_string_lossy()).unwrap();
        git(
            &repo,
            &[
                "worktree",
                "add",
                "--detach",
                "--no-checkout",
                wt.to_str().unwrap(),
                "HEAD",
            ],
        );
        std::fs::remove_dir_all(&wt).unwrap();

        // ONE pass must drop the row AND reap the now-stale git entry.
        let report = recover(&store, &repo, &root).await.unwrap();
        assert!(
            report
                .dropped_missing_rows
                .iter()
                .any(|p| p.ends_with("gone")),
            "vanished row must be dropped: {report:?}"
        );
        assert!(
            report
                .reaped_git_orphans
                .iter()
                .any(|p| p.ends_with("gone")),
            "stale git entry must be reaped in the SAME pass: {report:?}"
        );

        let v = verify(&store, &repo, &root).await.unwrap();
        assert!(v.clean(), "verify must be clean after ONE gc pass: {v:?}");

        let again = recover(&store, &repo, &root).await.unwrap();
        assert!(again.clean(), "second pass must be a no-op: {again:?}");
    }

    #[tokio::test]
    async fn verify_reports_invalid_worktree_state_as_unclean() {
        let dir = tempfile::tempdir().unwrap();
        let repo = scratch_repo(dir.path());
        let root = dir.path().join("wt");
        let store = StateStore::open(dir.path().join("studio.db").to_str().unwrap()).unwrap();

        let wt = WorktreeManager::path_for(&root, "scratch", "ok");
        std::fs::create_dir_all(&wt).unwrap();
        store
            .begin_worktree(&wt.to_string_lossy(), "scratch", "ok")
            .unwrap();
        store.mark_worktree_ready(&wt.to_string_lossy()).unwrap();
        // Corrupt the state directly (separate connection, same WAL db).
        let raw = rusqlite::Connection::open(dir.path().join("studio.db")).unwrap();
        raw.execute_batch("UPDATE worktrees SET state='bogus_state'")
            .unwrap();
        drop(raw);

        let report = verify(&store, &repo, &root).await.unwrap();
        assert_eq!(report.invalid_state_rows, 1, "{report:?}");
        assert!(
            !report.clean(),
            "an unknown state must be unclean: {report:?}"
        );
    }

    // --- A3/A4: symlink refusal (never traverse or delete through a link) ---

    #[cfg(unix)]
    #[tokio::test]
    async fn sweep_refuses_a_symlink_bucket_and_preserves_its_target() {
        let dir = tempfile::tempdir().unwrap();
        let repo = scratch_repo(dir.path());
        let root = dir.path().join("wt");
        let store = StateStore::open(dir.path().join("studio.db").to_str().unwrap()).unwrap();

        // A victim tree outside the root, with a live symlink bucket pointing at it.
        let victim = dir.path().join("victim");
        std::fs::create_dir_all(victim.join("keep")).unwrap();
        std::fs::create_dir_all(victim.join("data")).unwrap();
        std::fs::write(victim.join("marker.txt"), "VICTIM").unwrap();
        std::fs::write(victim.join("keep/k.txt"), "KEEP").unwrap();
        std::fs::write(victim.join("data/nested.txt"), "NESTED").unwrap();
        std::fs::create_dir_all(&root).unwrap();
        std::os::unix::fs::symlink(&victim, root.join("evilbucket")).unwrap();

        let report = recover(&store, &repo, &root).await.unwrap();

        assert!(
            report
                .refused_symlinks
                .iter()
                .any(|p| p.ends_with("evilbucket")),
            "symlink bucket must be refused: {report:?}"
        );
        assert!(
            report.swept_dirs.is_empty(),
            "nothing may be swept through the symlink: {report:?}"
        );
        assert!(!report.clean(), "a refused symlink is not clean");
        // The victim is untouched.
        assert_eq!(
            std::fs::read_to_string(victim.join("marker.txt")).unwrap(),
            "VICTIM"
        );
        assert!(victim.join("keep/k.txt").exists());
        assert!(victim.join("data/nested.txt").exists());
        assert!(root.join("evilbucket").is_symlink());

        // `verify` reports the symlink as unclean too.
        let v = verify(&store, &repo, &root).await.unwrap();
        assert!(
            v.refused_symlinks.iter().any(|p| p.ends_with("evilbucket")),
            "verify must refuse the symlink: {v:?}"
        );
        assert!(!v.clean());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn sweep_refuses_a_symlink_nested_in_a_bucket() {
        let dir = tempfile::tempdir().unwrap();
        let repo = scratch_repo(dir.path());
        let root = dir.path().join("wt");
        let store = StateStore::open(dir.path().join("studio.db").to_str().unwrap()).unwrap();

        let victim = dir.path().join("victim");
        std::fs::create_dir_all(victim.join("keep")).unwrap();
        std::fs::write(victim.join("keep/k.txt"), "KEEP").unwrap();
        // A real untracked bucket containing a symlink to the victim.
        let bucket = root.join("aaaaaaaa");
        std::fs::create_dir_all(&bucket).unwrap();
        std::fs::write(bucket.join("real.txt"), "real").unwrap();
        std::os::unix::fs::symlink(&victim, bucket.join("escape")).unwrap();

        let report = recover(&store, &repo, &root).await.unwrap();

        assert!(
            report
                .refused_symlinks
                .iter()
                .any(|p| p.ends_with("escape")),
            "nested symlink must be refused: {report:?}"
        );
        assert!(victim.join("keep/k.txt").exists(), "victim must survive");
        // The whole offending entry is left in place, not partially deleted.
        assert!(bucket.join("escape").is_symlink());
    }

    // --- D: verify must not report a false clean ---

    #[tokio::test]
    async fn verify_flags_each_containment_repo_and_kind_defect() {
        let dir = tempfile::tempdir().unwrap();
        let repo = scratch_repo(dir.path());
        let repo_canon = repo.canonicalize().unwrap();
        let repo_str = repo_canon.to_string_lossy().to_string();
        let root = dir.path().join("wt");
        let store = StateStore::open(dir.path().join("studio.db").to_str().unwrap()).unwrap();
        std::fs::create_dir_all(&root).unwrap();

        let bucket = WorktreeManager::path_for(&root, "scratch", "x")
            .parent()
            .unwrap()
            .to_path_buf();
        std::fs::create_dir_all(&bucket).unwrap();

        // (1) row path OUTSIDE the worktree root.
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("f"), "x").unwrap();
        store
            .begin_worktree(&outside.to_string_lossy(), &repo_str, "outside")
            .unwrap();
        store
            .mark_worktree_ready(&outside.to_string_lossy())
            .unwrap();

        // (2) row whose `repo` is a different repo/DB.
        let mismatch = bucket.join("repo-mismatch");
        std::fs::create_dir_all(&mismatch).unwrap();
        std::fs::write(mismatch.join("f"), "x").unwrap();
        store
            .begin_worktree(&mismatch.to_string_lossy(), "/some/other/repo", "rm")
            .unwrap();
        store
            .mark_worktree_ready(&mismatch.to_string_lossy())
            .unwrap();

        // (3) `ready` row whose dir exists but is EMPTY.
        let empty = bucket.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        store
            .begin_worktree(&empty.to_string_lossy(), &repo_str, "empty")
            .unwrap();
        store.mark_worktree_ready(&empty.to_string_lossy()).unwrap();

        // (4) row whose path is a regular FILE.
        let file = bucket.join("regular-file");
        std::fs::write(&file, "hello").unwrap();
        store
            .begin_worktree(&file.to_string_lossy(), &repo_str, "file")
            .unwrap();
        store.mark_worktree_ready(&file.to_string_lossy()).unwrap();

        // (5) real non-empty dir that is NOT a git worktree.
        let not_wt = bucket.join("not-a-worktree");
        std::fs::create_dir_all(&not_wt).unwrap();
        std::fs::write(not_wt.join("f"), "x").unwrap();
        store
            .begin_worktree(&not_wt.to_string_lossy(), &repo_str, "naw")
            .unwrap();
        store
            .mark_worktree_ready(&not_wt.to_string_lossy())
            .unwrap();

        let report = verify(&store, &repo, &root).await.unwrap();
        assert!(!report.clean(), "must not be clean: {report:?}");
        assert_eq!(report.outside_root_rows.len(), 1, "{report:?}");
        assert_eq!(report.repo_mismatch_rows.len(), 1, "{report:?}");
        assert_eq!(report.empty_rows.len(), 1, "{report:?}");
        assert_eq!(report.non_directory_rows.len(), 1, "{report:?}");
        // The other bogus rows are not git worktrees either, so this class
        // legitimately flags them all; assert the specific row is present.
        assert!(
            report
                .not_a_worktree_rows
                .iter()
                .any(|p| p.ends_with("not-a-worktree")),
            "{report:?}"
        );
        assert!(
            report
                .outside_root_rows
                .iter()
                .any(|p| p.ends_with("outside")),
            "{report:?}"
        );
    }
}
