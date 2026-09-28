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
//!
//! ## SEC-A-TOCTOU-01: fd-anchored sweep
//!
//! The untracked-directory sweep is **directory-fd anchored**, not path
//! anchored. The root is pinned once with `open(O_DIRECTORY|O_NOFOLLOW)`
//! (on Linux, `openat2` with `RESOLVE_BENEATH|RESOLVE_NO_SYMLINKS`), every
//! child directory is opened with `openat(dirfd, name, O_DIRECTORY|O_NOFOLLOW)`
//! and deleted with `unlinkat(dirfd, name[, AT_REMOVEDIR])`. No path string is
//! ever re-resolved inside the sweep, so a bucket swapped for a symlink *after*
//! it was classified is refused (`ELOOP`) instead of being followed; an entry
//! that vanishes mid-walk is skipped instead of aborting the pass. This closes
//! QA-B's post-classification swap (`a5`) and its fast-flip abort.
//!
//! ## LOCK-B-iii: never reap a git-`locked` worktree
//!
//! Recovery is the only destructive path. The engine `git worktree lock`s every
//! worktree it creates, so a *live* worktree is always reported `locked` by
//! `git worktree list --porcelain`. A second engine can nevertheless be admitted
//! when a same-user replaces **both** locked inodes (`mv shared shared.old;
//! mkdir shared` plus a fresh repo): the `flock` no longer matches, the new
//! engine's ledger is empty, and every worktree under the shared path looks like
//! an orphan. Both destructive passes therefore honour git's own lock flag:
//! the git-orphan pass ([`WorktreeManager::recover`]) refuses any worktree git
//! reports as `locked` (reported as `refused_locked`) unless it is a
//! half-created `creating` row from this engine's own ledger AND the live git
//! lock reason equals `studio:<our engine id>` (B-EXEMPT-FORGE-01: a forged row
//! alone never authorizes destruction; old bare `daemon` reasons are refused),
//! and the scratchpad sweep reaps only *unregistered* directories —
//! a registered git worktree (locked, or still mid-create) is refused
//! (`refused_worktrees`) and left to the git pass. The refusal is independent of
//! lock identity, so it also covers the accidental two-engine / stale-root case.
//!
//! STALE-LOCK / REFUSED-PATH operator story (C-UNLOCK-TRAP-01): an abandoned
//! git-locked worktree with no ledger row (or with a refused forged row) is
//! refused, and the refusal is PERSISTED in this DB (`refused_paths`) — so
//! `git worktree unlock <path>` followed by `gc` still refuses (unlocking
//! never re-arms auto-reap; the old unlock-then-gc remedy was a trap that let
//! `gc` reap the foreign worktree). The ONLY path from refused to reaped is
//! the explicit operator command `studio release <path>`, which forgets the
//! refusal (and drops a lingering `creating` row for the path); the next `gc`
//! then treats the path as an ordinary orphan. Running `release` on a foreign
//! LIVE worktree and then unlocking it lets `gc` reap it — the operator's
//! responsibility. `gc` exits non-zero whenever its report is not clean, so
//! every refusal is visible to operators.

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
    /// Registered git worktrees found under the root that recovery refused to
    /// sweep: a registered worktree is never scratch junk (LOCK-B-iii). This
    /// includes both locked and not-yet-locked (mid-create) worktrees.
    pub refused_worktrees: Vec<String>,
    /// The subset of [`Self::refused_worktrees`] that git reports as `locked`.
    pub refused_locked: Vec<String>,
    /// Directories under the root whose name contains control characters
    /// (newline etc.): refused, never traversed, never reaped (C-NEWLINE-01).
    /// A ledger-tracked live path is still skipped silently via exact match —
    /// only untracked control-char paths land here.
    pub refused_control_paths: Vec<String>,
    /// Paths refused SOLELY because a previous gc persisted a refusal for them
    /// (C-UNLOCK-TRAP-01): currently unlocked (or no longer git-registered)
    /// but never auto-reaped. Unlocking never re-arms auto-reap; only
    /// `studio release <path>` clears this. Merged here from both the git
    /// pass (unlocked entries with a stored refusal) and the sweep
    /// (unregistered directories with a stored refusal).
    pub refused_persisted: Vec<String>,
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
            && self.refused_worktrees.is_empty()
            && self.refused_locked.is_empty()
            && self.refused_control_paths.is_empty()
            && self.refused_persisted.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Directory-fd helpers (POSIX). All traversal is relative to a pinned dirfd.
// ---------------------------------------------------------------------------
#[cfg(unix)]
mod fd {
    use super::{path_str, RecoveryError};
    use std::ffi::{CStr, CString, OsStr, OsString};
    use std::os::raw::c_int;
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    use std::path::{Path, PathBuf};

    /// An owned directory fd, closed on drop. Traversal never dereferences a
    /// path string again once this exists.
    pub struct DirFd {
        fd: c_int,
    }

    impl DirFd {
        pub fn raw(&self) -> c_int {
            self.fd
        }
    }

    impl Drop for DirFd {
        fn drop(&mut self) {
            // SAFETY: `self.fd` is an open fd owned by this struct; closing once.
            unsafe {
                libc::close(self.fd);
            }
        }
    }

    fn cstr(s: &OsStr) -> Result<CString, RecoveryError> {
        CString::new(s.as_bytes())
            .map_err(|_| RecoveryError::BadPath(s.to_string_lossy().into_owned()))
    }

    pub fn is_symlink_err(e: &std::io::Error) -> bool {
        matches!(e.raw_os_error(), Some(libc::ELOOP) | Some(libc::ENOTDIR))
    }

    pub fn is_enoent(e: &std::io::Error) -> bool {
        e.raw_os_error() == Some(libc::ENOENT)
    }

    /// Open the worktree root without following a symlinked final component
    /// (`O_NOFOLLOW`) and, on Linux, without following *any* symlink component
    /// (`openat2` + `RESOLVE_BENEATH|RESOLVE_NO_SYMLINKS`), falling back to
    /// plain `open(O_NOFOLLOW)` where `openat2` is unavailable.
    pub fn open_root(root: &Path) -> Result<DirFd, RecoveryError> {
        #[cfg(target_os = "linux")]
        {
            match open_root_openat2(root) {
                Ok(fd) => return Ok(DirFd { fd }),
                Err(e)
                    if matches!(
                        e.raw_os_error(),
                        Some(libc::ENOSYS) | Some(libc::EINVAL) | Some(libc::EPERM)
                    ) =>
                {
                    // Kernel/filesystem does not support openat2 → fall back.
                }
                Err(e) if is_symlink_err(&e) => {
                    return Err(RecoveryError::SymlinkRefused(path_str(root)));
                }
                Err(e) => return Err(RecoveryError::Io(format!("{}: {e}", root.display()))),
            }
        }
        let c = cstr(root.as_os_str())?;
        // SAFETY: `c` is a valid NUL-terminated path; flags are constants.
        let fd = unsafe {
            libc::open(
                c.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            let e = std::io::Error::last_os_error();
            if is_symlink_err(&e) {
                return Err(RecoveryError::SymlinkRefused(path_str(root)));
            }
            return Err(RecoveryError::Io(format!("{}: {e}", root.display())));
        }
        Ok(DirFd { fd })
    }

    /// `openat2(root, RESOLVE_BENEATH|RESOLVE_NO_SYMLINKS)`. `RESOLVE_BENEATH`
    /// requires a relative pathname, so an absolute root is resolved relative
    /// to `/` (opened temporarily).
    #[cfg(target_os = "linux")]
    fn open_root_openat2(root: &Path) -> std::io::Result<c_int> {
        let abs = root.is_absolute();
        let mut base_fd: c_int = libc::AT_FDCWD;
        let rel: PathBuf = if abs {
            // SAFETY: static NUL-terminated "/" path; flags are constants.
            base_fd = unsafe {
                libc::open(
                    c"/".as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
                )
            };
            if base_fd < 0 {
                return Err(std::io::Error::last_os_error());
            }
            root.strip_prefix("/")
                .map(Path::to_path_buf)
                .unwrap_or_default()
        } else {
            root.to_path_buf()
        };
        let c = match CString::new(rel.as_os_str().as_bytes()) {
            Ok(c) => c,
            Err(_) => {
                if abs {
                    // SAFETY: base_fd is open and owned here.
                    unsafe { libc::close(base_fd) };
                }
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "NUL byte in path",
                ));
            }
        };
        // `open_how` is `#[non_exhaustive]`; populate a zeroed value.
        // SAFETY: all-zero is a valid bit pattern for this plain-old-data struct.
        let mut how: libc::open_how = unsafe { std::mem::zeroed() };
        how.flags = (libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC) as libc::__u64;
        how.mode = 0;
        how.resolve = libc::RESOLVE_BENEATH | libc::RESOLVE_NO_SYMLINKS;
        // SAFETY: `c` and `how` are valid for the duration of the call.
        let fd = unsafe {
            libc::syscall(
                libc::SYS_openat2,
                base_fd,
                c.as_ptr(),
                &how as *const libc::open_how,
                std::mem::size_of::<libc::open_how>(),
            ) as c_int
        };
        let result = if fd < 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(fd)
        };
        if abs {
            // SAFETY: base_fd was opened above and is still owned here.
            unsafe { libc::close(base_fd) };
        }
        result
    }

    /// `openat(dirfd, name, O_DIRECTORY|O_NOFOLLOW)`. A symlink target yields
    /// `ELOOP`; a non-directory yields `ENOTDIR`; either is a refusal, never a
    /// traversal.
    pub fn open_child_dir(dirfd: c_int, name: &OsStr) -> std::io::Result<DirFd> {
        let c = CString::new(name.as_bytes()).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "NUL byte in name")
        })?;
        // SAFETY: valid NUL-terminated name; flags are constants.
        let fd = unsafe {
            libc::openat(
                dirfd,
                c.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(DirFd { fd })
    }

    /// `fstatat(dirfd, name, AT_SYMLINK_NOFOLLOW)` — `lstat` relative to dirfd.
    pub fn lstat_at(dirfd: c_int, name: &OsStr) -> std::io::Result<libc::stat> {
        let c = CString::new(name.as_bytes()).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "NUL byte in name")
        })?;
        // SAFETY: zeroed stat is a valid out-param; cstr is valid.
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        let rc = unsafe { libc::fstatat(dirfd, c.as_ptr(), &mut st, libc::AT_SYMLINK_NOFOLLOW) };
        if rc != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(st)
    }

    /// Immediate child names of `dirfd`, via `fdopendir`/`readdir` on a `dup` of
    /// the pinned fd. The pinned fd keeps the directory alive even if the name
    /// is unlinked, so a swap-out cannot redirect this listing.
    pub fn read_names(dirfd: c_int) -> std::io::Result<Vec<OsString>> {
        // SAFETY: dup of a valid fd.
        let dupfd = unsafe { libc::dup(dirfd) };
        if dupfd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: dupfd is owned here and handed to fdopendir below.
        let dirp = unsafe { libc::fdopendir(dupfd) };
        if dirp.is_null() {
            let e = std::io::Error::last_os_error();
            // SAFETY: fdopendir failed, so dupfd is still owned here.
            unsafe { libc::close(dupfd) };
            return Err(e);
        }
        let mut out = Vec::new();
        loop {
            // SAFETY: dirp lives until closedir below.
            let entry = unsafe { libc::readdir(dirp) };
            if entry.is_null() {
                break;
            }
            // SAFETY: d_name is a NUL-terminated buffer owned by the DIR.
            let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
            let bytes = name.to_bytes();
            if bytes == b"." || bytes == b".." {
                continue;
            }
            out.push(OsString::from_vec(bytes.to_vec()));
        }
        // SAFETY: dirp owns dupfd and is closed exactly once here.
        unsafe { libc::closedir(dirp) };
        Ok(out)
    }

    /// `unlinkat(dirfd, name, AT_REMOVEDIR)` for directories, `unlinkat(...)` for
    /// files. Never follows the final component.
    pub fn unlink_at(dirfd: c_int, name: &OsStr, is_dir: bool) -> std::io::Result<()> {
        let c = CString::new(name.as_bytes()).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "NUL byte in name")
        })?;
        let flags = if is_dir { libc::AT_REMOVEDIR } else { 0 };
        // SAFETY: valid NUL-terminated name; flags are constants.
        let rc = unsafe { libc::unlinkat(dirfd, c.as_ptr(), flags) };
        if rc != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }

    pub fn is_symlink(st: &libc::stat) -> bool {
        (st.st_mode & libc::S_IFMT) == libc::S_IFLNK
    }

    pub fn is_dir(st: &libc::stat) -> bool {
        (st.st_mode & libc::S_IFMT) == libc::S_IFDIR
    }
}

/// Test seam: callbacks fired at the exact points QA-B's races hit, so the
/// regression tests are deterministic rather than timing-dependent. Production
/// passes [`NoSweepHook`].
trait SweepHook {
    /// Fired after a root child has been classified as a real directory bucket
    /// and *before* its fd is opened — the post-classification swap window.
    fn bucket_classified(&mut self, _path: &Path) {}
    /// Fired after a bucket entry has been enumerated and *before* it is
    /// `lstat`ed — the fast-flip window.
    fn entry_enumerated(&mut self, _path: &Path) {}
}

struct NoSweepHook;
impl SweepHook for NoSweepHook {}

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

    // (a) Collect half-created worktrees — but do NOT delete the rows yet.
    // B-EXEMPT-FORGE-01: ledger content is not a trust anchor. A forged
    // `creating` row naming another engine's live locked worktree must survive
    // as refused evidence (the git pass below decides, by live lock reason);
    // only rows the git pass does NOT refuse are deleted in (a2).
    let creating: Vec<String> = store.worktrees_in_state("creating")?;
    let own_abandoned: HashSet<String> = creating.iter().map(|p| norm_str(p)).collect();
    // Our persistent ownership proof (generated once per DB, stable across
    // restarts — so our own previous-run half-creates still reap — but bound
    // to the DB FILE identity, so a `cp` clone diverges: A-DB-COPY-01).
    let engine_id = store.engine_id()?;
    // C-UNLOCK-TRAP-01: paths this DB refused before. Both destructive passes
    // honour the set: a refused path is never auto-reaped, regardless of
    // later lock state. (A ledger-tracked LIVE path is still skipped silently
    // via exact match — persistence only blocks auto-reaping untracked paths,
    // so recording a refusal can never wedge our own future worktrees.)
    let persisted = store.refused_set()?;

    // (b) Ledger row whose directory already vanished. This must run before the
    // git pass so the now-untracked git entry is classified as an orphan.
    let mut dropped: HashSet<String> = HashSet::new();
    for (raw, _state) in store.worktree_rows()? {
        let norm = norm_str(&raw);
        if !Path::new(&norm).exists() {
            store.delete_worktree(&raw)?;
            report.dropped_missing_rows.push(norm.clone());
            dropped.insert(norm);
        }
    }

    // (c) Git knows about it, the ledger does not: unlock + prune + trash —
    // EXCEPT a worktree git reports as `locked` without OUR live lock reason,
    // which is refused (LOCK-B-iii, B-EXEMPT-FORGE-01), and entries with
    // control-char paths, which are never managed (C-NEWLINE-01).
    //
    // `known` excludes the (a) candidates: they are being reaped, not kept —
    // otherwise the git pass would mistake our own half-creates for live rows.
    let known: HashSet<String> = store
        .worktree_records()?
        .into_iter()
        .filter(|r| r.state != "creating")
        .map(|r| norm_str(&r.path))
        .collect();
    let wt_recovery =
        WorktreeManager::recover(&repo, &known, &own_abandoned, &engine_id, &persisted).await?;
    report.reaped_git_orphans = wt_recovery.reaped;
    report.refused_locked.extend(wt_recovery.refused_locked);
    report
        .refused_control_paths
        .extend(wt_recovery.refused_control);
    report
        .refused_persisted
        .extend(wt_recovery.refused_persisted);
    let refused: HashSet<String> = report
        .refused_locked
        .iter()
        .chain(report.refused_control_paths.iter())
        .chain(report.refused_persisted.iter())
        .cloned()
        .collect();

    // (a2) Resolve the (a) candidates: delete every `creating` row the git
    // pass did not refuse (already-dropped-by-(b) rows are reported there,
    // not here). A refused forged row is KEPT — recovery reaps nothing for it
    // and the row stays as evidence until a human resolves it.
    for raw in &creating {
        let norm = norm_str(raw);
        if refused.contains(&norm) || dropped.contains(&norm) {
            continue;
        }
        store.delete_worktree(raw)?;
        report.reaped_creating.push(norm);
    }

    // (d) Sweep every *real* directory under the root the ledger does not track.
    // SEC-A-TOCTOU-01: directory-fd anchored; never re-resolves a path.
    // LOCK-B-iii: registered worktrees are refused here (the git pass owns them).
    // C-UNLOCK-TRAP-01: unregistered directories with a stored refusal are
    // refused here too (unlocking never re-arms auto-reap).
    sweep_untracked(&root, &known, &persisted, &mut report, &mut NoSweepHook)?;

    // (c) and (d) can both refuse the same live locked worktree; keep one entry.
    dedup_in_place(&mut report.refused_worktrees);
    dedup_in_place(&mut report.refused_locked);
    dedup_in_place(&mut report.refused_control_paths);
    dedup_in_place(&mut report.refused_persisted);

    // (e) C-UNLOCK-TRAP-01: persist every refusal, so the next gc (or daemon
    // boot) still refuses even if the operator unlocked the path in between.
    // First refusal wins (`INSERT OR IGNORE`); symlinks need no persistence
    // (every pass re-refuses them live via `lstat`).
    for p in &report.refused_locked {
        store.record_refused(p, "locked")?;
    }
    for p in &report.refused_control_paths {
        store.record_refused(p, "control")?;
    }
    for p in &report.refused_worktrees {
        store.record_refused(p, "worktree")?;
    }
    for p in &report.refused_persisted {
        store.record_refused(p, "persisted")?;
    }

    Ok(report)
}

/// Record a registered worktree the sweep refused. Locked ones are also listed
/// under `refused_locked` (the LOCK-B-iii class); every registered worktree is
/// listed under `refused_worktrees`.
fn refuse_registered_worktree(
    report: &mut RecoveryReport,
    entry: &crate::worktree::WorktreeEntry,
    path: &Path,
) {
    let p = path_str(path);
    if entry.locked {
        report.refused_locked.push(p.clone());
    }
    if !report.refused_worktrees.contains(&p) {
        report.refused_worktrees.push(p);
    }
}

/// Remove duplicate path strings, preserving first-seen order.
fn dedup_in_place(v: &mut Vec<String>) {
    let mut seen = HashSet::new();
    v.retain(|p| seen.insert(p.clone()));
}

/// Directory-fd anchored untracked sweep (POSIX). See module docs.
#[cfg(unix)]
fn sweep_untracked(
    root: &Path,
    known: &HashSet<String>,
    persisted: &HashSet<String>,
    report: &mut RecoveryReport,
    hook: &mut dyn SweepHook,
) -> Result<(), RecoveryError> {
    let persisted_norm: HashSet<String> = persisted.iter().map(|p| norm_str(p)).collect();
    let root_fd = fd::open_root(root)?;
    let names = fd::read_names(root_fd.raw())
        .map_err(|e| RecoveryError::Io(format!("{}: {e}", root.display())))?;
    for name in names {
        let display = root.join(&name);
        let st = match fd::lstat_at(root_fd.raw(), &name) {
            Ok(s) => s,
            Err(e) if fd::is_enoent(&e) => continue, // vanished: skip
            Err(e) => return Err(RecoveryError::Io(format!("{}: {e}", display.display()))),
        };
        if fd::is_symlink(&st) {
            report.refused_symlinks.push(path_str(&display));
            continue;
        }
        // Loose files directly under the root were never swept and still are not.
        if !fd::is_dir(&st) {
            continue;
        }
        // C-NEWLINE-01: the sweep's exact match covers the real (tracked) path
        // first — a ledger-tracked live path is skipped silently even when it
        // contains control characters. An UNTRACKED control-char path is
        // refused (dedicated class), never opened, never probed, never reaped.
        if known.contains(&path_str(&display)) {
            continue;
        }
        if crate::worktree::name_has_control_bytes(&name) {
            report.refused_control_paths.push(path_str(&display));
            continue;
        }
        // LOCK-B-iii: a registered git worktree directly under the root is a live
        // (or mid-create) worktree, never a sweepable bucket — regardless of
        // whose engine/repo it belongs to.
        if let Some(entry) = crate::worktree::worktree_entry_at(&display) {
            refuse_registered_worktree(report, &entry, &display);
            continue;
        }
        hook.bucket_classified(&display);
        // Race window: the name may have become a symlink; O_NOFOLLOW refuses it.
        let bucket_fd = match fd::open_child_dir(root_fd.raw(), &name) {
            Ok(f) => f,
            Err(e) if fd::is_symlink_err(&e) => {
                report.refused_symlinks.push(path_str(&display));
                continue;
            }
            Err(e) if fd::is_enoent(&e) => continue,
            Err(e) => return Err(RecoveryError::Io(format!("{}: {e}", display.display()))),
        };
        let entries = fd::read_names(bucket_fd.raw())
            .map_err(|e| RecoveryError::Io(format!("{}: {e}", display.display())))?;
        for ename in entries {
            let edisplay = display.join(&ename);
            hook.entry_enumerated(&edisplay);
            // Fast-flip window: an enumerated entry may already be gone.
            let est = match fd::lstat_at(bucket_fd.raw(), &ename) {
                Ok(s) => s,
                Err(e) if fd::is_enoent(&e) => continue,
                Err(e) => {
                    return Err(RecoveryError::Io(format!("{}: {e}", edisplay.display())));
                }
            };
            if fd::is_symlink(&est) {
                report.refused_symlinks.push(path_str(&edisplay));
                continue;
            }
            let norm = path_str(&edisplay);
            if known.contains(&norm) {
                continue;
            }
            // C-NEWLINE-01: untracked control-char paths are refused before any
            // git probe or deletion — the exact match above already covers a
            // ledger-tracked real path, so this never hides a live worktree.
            if crate::worktree::name_has_control_bytes(&ename) {
                report.refused_control_paths.push(norm);
                continue;
            }
            // C-UNLOCK-TRAP-01: a stored refusal never re-arms, even when the
            // path is now unlocked or unregistered. A still-registered
            // worktree keeps its live classes (locked → refused_locked); an
            // unregistered-but-refused directory lands in refused_persisted.
            // Either way it is never swept.
            if persisted_norm.contains(&norm) {
                if fd::is_dir(&est) {
                    if let Some(entry) = crate::worktree::worktree_entry_at(Path::new(&norm)) {
                        refuse_registered_worktree(report, &entry, Path::new(&norm));
                        continue;
                    }
                }
                if !report.refused_persisted.contains(&norm) {
                    report.refused_persisted.push(norm);
                }
                continue;
            }
            // LOCK-B-iii: a *registered* git worktree is never scratch junk —
            // the git pass (which honours `locked` and owns half-created rows)
            // is the only place registered worktrees are ever reaped. The check
            // runs git *inside the candidate*, so a worktree belonging to
            // another engine/repo is refused too. Plain files (and symlinks,
            // refused above) are not probed.
            if fd::is_dir(&est) {
                if let Some(entry) = crate::worktree::worktree_entry_at(Path::new(&norm)) {
                    refuse_registered_worktree(report, &entry, Path::new(&norm));
                    continue;
                }
            }
            if subtree_has_symlink(&bucket_fd, &ename)? {
                report.refused_symlinks.push(norm);
            } else {
                remove_tree(&bucket_fd, &ename)?;
                report.swept_dirs.push(norm);
            }
        }
        // Remove the bucket if the sweep emptied it (fd-relative).
        let left = fd::read_names(bucket_fd.raw())
            .map_err(|e| RecoveryError::Io(format!("{}: {e}", display.display())))?;
        if left.is_empty() {
            match fd::unlink_at(root_fd.raw(), &name, true) {
                Ok(()) => {}
                Err(e) if fd::is_enoent(&e) => {}
                Err(_) => {} // best-effort, mirrors the previous `let _ =`
            }
        }
    }
    Ok(())
}

/// Non-POSIX fallback: fd-anchored traversal is unavailable, so refuse to sweep
/// (fail closed) rather than run a path-based walk that could follow a symlink.
/// The report becomes unclean; nothing is deleted.
#[cfg(not(unix))]
fn sweep_untracked(
    _root: &Path,
    _known: &HashSet<String>,
    _persisted: &HashSet<String>,
    report: &mut RecoveryReport,
    _hook: &mut dyn SweepHook,
) -> Result<(), RecoveryError> {
    report
        .refused_symlinks
        .push("<non-POSIX platform: fd-anchored sweep unavailable, refusing to sweep>".into());
    Ok(())
}

/// True when `name` under `dirfd` (or any descendant) is a symlink. Read-only:
/// used to refuse an entry *before* deleting it, so nothing is partially removed.
/// Every step is relative to the pinned fds, so a swap cannot redirect it.
#[cfg(unix)]
fn subtree_has_symlink(dirfd: &fd::DirFd, name: &std::ffi::OsStr) -> Result<bool, RecoveryError> {
    let st = match fd::lstat_at(dirfd.raw(), name) {
        Ok(s) => s,
        Err(e) if fd::is_enoent(&e) => return Ok(false),
        Err(e) => return Err(RecoveryError::Io(e.to_string())),
    };
    if fd::is_symlink(&st) {
        return Ok(true);
    }
    if !fd::is_dir(&st) {
        return Ok(false);
    }
    let child = match fd::open_child_dir(dirfd.raw(), name) {
        Ok(f) => f,
        // A symlink that appeared since the lstat: treat as a symlink.
        Err(e) if fd::is_symlink_err(&e) => return Ok(true),
        Err(e) if fd::is_enoent(&e) => return Ok(false),
        Err(e) => return Err(RecoveryError::Io(e.to_string())),
    };
    for cname in fd::read_names(child.raw()).map_err(|e| RecoveryError::Io(e.to_string()))? {
        if subtree_has_symlink(&child, &cname)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Recursively remove `name` under `dirfd`, post-order, entirely via `openat` /
/// `unlinkat`. Refuses (typed) at any symlink; never follows one.
#[cfg(unix)]
fn remove_tree(dirfd: &fd::DirFd, name: &std::ffi::OsStr) -> Result<(), RecoveryError> {
    let st = match fd::lstat_at(dirfd.raw(), name) {
        Ok(s) => s,
        Err(e) if fd::is_enoent(&e) => return Ok(()), // already gone
        Err(e) => return Err(RecoveryError::Io(e.to_string())),
    };
    if fd::is_symlink(&st) {
        return Err(RecoveryError::SymlinkRefused(
            name.to_string_lossy().into_owned(),
        ));
    }
    if fd::is_dir(&st) {
        let child = match fd::open_child_dir(dirfd.raw(), name) {
            Ok(f) => f,
            Err(e) if fd::is_symlink_err(&e) => {
                return Err(RecoveryError::SymlinkRefused(
                    name.to_string_lossy().into_owned(),
                ));
            }
            Err(e) if fd::is_enoent(&e) => return Ok(()),
            Err(e) => return Err(RecoveryError::Io(e.to_string())),
        };
        for cname in fd::read_names(child.raw()).map_err(|e| RecoveryError::Io(e.to_string()))? {
            remove_tree(&child, &cname)?;
        }
        match fd::unlink_at(dirfd.raw(), name, true) {
            Ok(()) => {}
            Err(e) if fd::is_enoent(&e) => {}
            Err(e) => return Err(RecoveryError::Io(e.to_string())),
        }
    } else {
        match fd::unlink_at(dirfd.raw(), name, false) {
            Ok(()) => {}
            Err(e) if fd::is_enoent(&e) => {}
            Err(e) => return Err(RecoveryError::Io(e.to_string())),
        }
    }
    Ok(())
}

/// Read-only integrity report. Never mutates; used by `studio verify`.
///
/// RO-VERIFY-01: `verify` never creates the worktree root. A missing root is a
/// typed unclean result (`root_missing`), not a `mkdir`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct IntegrityReport {
    pub integrity_ok: bool,
    pub integrity_detail: String,
    pub creating_rows: usize,
    pub missing_rows: usize,
    /// Rows whose `state` is not `creating`/`ready` — corruption.
    pub invalid_state_rows: usize,
    /// The worktree root does not exist (typed unclean; `verify` did not create it).
    pub root_missing: bool,
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
            && !self.root_missing
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
    // RO-VERIFY-01: never create. Canonicalize only; a missing root is typed.
    let root = match worktree_root.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            let mut report = IntegrityReport::default();
            report.integrity_detail = store.integrity_check()?;
            report.integrity_ok = report.integrity_detail == "ok";
            report.creating_rows = store.worktrees_in_state("creating")?.len();
            report.invalid_state_rows = store.invalid_state_rows()?;
            report.root_missing = true;
            // Every ledger row under the (absent) root is missing; non-contained
            // rows are still reported so `verify` stays informative.
            let lexical_root = crate::worktree::normalize_path(worktree_root);
            for rec in store.worktree_records()? {
                let norm = norm_str(&rec.path);
                if !Path::new(&norm).starts_with(&lexical_root) {
                    report.outside_root_rows.push(norm.clone());
                }
                if norm_str(&rec.repo) != path_str(&repo) {
                    report.repo_mismatch_rows.push(norm.clone());
                }
                if std::fs::symlink_metadata(&norm).is_err() {
                    report.missing_rows += 1;
                } else if std::fs::symlink_metadata(&norm)
                    .map(|m| m.file_type().is_symlink())
                    .unwrap_or(false)
                {
                    report.refused_symlinks.push(norm);
                }
            }
            return Ok(report);
        }
    };
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
        if dir_is_empty(Path::new(&norm))? {
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

    walk_root_readonly(&root, &known, &mut report)?;
    Ok(report)
}

/// Read-only, fd-anchored listing of the root for `verify`: reports refused
/// symlinks and untracked leftover entries. Deletes nothing.
#[cfg(unix)]
fn walk_root_readonly(
    root: &Path,
    known: &HashSet<String>,
    report: &mut IntegrityReport,
) -> Result<(), RecoveryError> {
    let root_fd = match fd::open_root(root) {
        Ok(f) => f,
        Err(RecoveryError::SymlinkRefused(p)) => {
            report.refused_symlinks.push(p);
            return Ok(());
        }
        Err(e) => return Err(e),
    };
    for name in fd::read_names(root_fd.raw())
        .map_err(|e| RecoveryError::Io(format!("{}: {e}", root.display())))?
    {
        let display = root.join(&name);
        let st = match fd::lstat_at(root_fd.raw(), &name) {
            Ok(s) => s,
            Err(e) if fd::is_enoent(&e) => continue,
            Err(e) => return Err(RecoveryError::Io(format!("{}: {e}", display.display()))),
        };
        if fd::is_symlink(&st) {
            report.refused_symlinks.push(path_str(&display));
            continue;
        }
        if !fd::is_dir(&st) {
            continue;
        }
        let bucket_fd = match fd::open_child_dir(root_fd.raw(), &name) {
            Ok(f) => f,
            Err(e) if fd::is_symlink_err(&e) => {
                report.refused_symlinks.push(path_str(&display));
                continue;
            }
            Err(e) if fd::is_enoent(&e) => continue,
            Err(e) => return Err(RecoveryError::Io(format!("{}: {e}", display.display()))),
        };
        for ename in fd::read_names(bucket_fd.raw())
            .map_err(|e| RecoveryError::Io(format!("{}: {e}", display.display())))?
        {
            let edisplay = display.join(&ename);
            let est = match fd::lstat_at(bucket_fd.raw(), &ename) {
                Ok(s) => s,
                Err(e) if fd::is_enoent(&e) => continue,
                Err(e) => {
                    return Err(RecoveryError::Io(format!("{}: {e}", edisplay.display())));
                }
            };
            if fd::is_symlink(&est) {
                report.refused_symlinks.push(path_str(&edisplay));
                continue;
            }
            let norm = path_str(&edisplay);
            if !known.contains(&norm) {
                report.leftover_dirs.push(norm);
            }
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn walk_root_readonly(
    _root: &Path,
    _known: &HashSet<String>,
    _report: &mut IntegrityReport,
) -> Result<(), RecoveryError> {
    Ok(())
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

/// Whether a directory is empty. A vanished directory counts as empty.
fn dir_is_empty(p: &Path) -> Result<bool, RecoveryError> {
    match std::fs::read_dir(p) {
        Ok(mut it) => Ok(it.next().is_none()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(e) => Err(RecoveryError::Io(e.to_string())),
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

    // --- RO-VERIFY-01: verify must not create the root ---

    #[tokio::test]
    async fn verify_does_not_create_a_missing_root() {
        let dir = tempfile::tempdir().unwrap();
        let repo = scratch_repo(dir.path());
        let nested = dir.path().join("a/b/c/wt");
        let store = StateStore::open(dir.path().join("studio.db").to_str().unwrap()).unwrap();

        let report = verify(&store, &repo, &nested).await.unwrap();
        assert!(
            report.root_missing,
            "a missing root must be typed unclean: {report:?}"
        );
        assert!(!report.clean(), "missing root is not clean: {report:?}");
        assert!(
            !nested.exists(),
            "verify must never create the worktree root: {}",
            nested.display()
        );
        assert!(
            !dir.path().join("a").exists(),
            "no nested dirs may be created"
        );
    }

    // --- SEC-A-TOCTOU-01: fd-anchored sweep regression tests ---

    /// (a) QA-B `a5`: the bucket is classified as a real directory, then swapped
    /// for a symlink to a live git repo before its fd is opened. The old
    /// path-based sweep followed the link and deleted the victim's contents
    /// (exit 0, no refusal). The fd-anchored sweep must refuse it.
    #[cfg(unix)]
    #[tokio::test]
    async fn sweep_refuses_a_bucket_swapped_to_a_symlink_after_classification() {
        struct SwapOnBucket {
            target: &'static str,
            victim: PathBuf,
        }
        impl SweepHook for SwapOnBucket {
            fn bucket_classified(&mut self, path: &Path) {
                if path.ends_with(self.target) {
                    std::fs::remove_dir_all(path).unwrap();
                    std::os::unix::fs::symlink(&self.victim, path).unwrap();
                }
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("wt");
        // Victim: a live git repository with content, outside the root.
        let victim = dir.path().join("victim");
        std::fs::create_dir_all(victim.join("src")).unwrap();
        std::fs::write(victim.join("precious.txt"), "PRECIOUS").unwrap();
        std::fs::write(victim.join("src/s.txt"), "s").unwrap();
        git(&victim, &["init", "-q"]);
        // RACE is a real directory at classification time.
        std::fs::create_dir_all(root.join("RACE")).unwrap();
        std::fs::write(root.join("RACE/placeholder"), "p").unwrap();

        let mut report = RecoveryReport::default();
        let mut hook = SwapOnBucket {
            target: "RACE",
            victim: victim.clone(),
        };
        sweep_untracked(
            &root,
            &HashSet::new(),
            &HashSet::new(),
            &mut report,
            &mut hook,
        )
        .unwrap();

        assert!(
            report.refused_symlinks.iter().any(|p| p.ends_with("RACE")),
            "the swapped bucket must be refused: {report:?}"
        );
        assert!(
            report.swept_dirs.is_empty(),
            "nothing may be swept through the swapped symlink: {report:?}"
        );
        assert_eq!(
            std::fs::read_to_string(victim.join("precious.txt")).unwrap(),
            "PRECIOUS",
            "victim content must survive"
        );
        assert!(victim.join("src/s.txt").exists());
        assert!(
            victim.join(".git").is_dir(),
            "the victim must remain a live git repo"
        );
    }

    /// (b) QA-B fast-flip: an entry enumerated inside a bucket vanishes before it
    /// is `lstat`ed. The old sweep aborted the whole pass with
    /// `io: No such file or directory` (exit 2); the fd-anchored sweep skips it.
    #[cfg(unix)]
    #[tokio::test]
    async fn sweep_skips_entries_that_vanish_after_enumeration() {
        struct DeleteOnEntry {
            target: PathBuf,
        }
        impl SweepHook for DeleteOnEntry {
            fn entry_enumerated(&mut self, path: &Path) {
                if path == self.target {
                    let _ = std::fs::remove_file(path);
                }
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("wt");
        let bucket = root.join("bucket");
        std::fs::create_dir_all(&bucket).unwrap();
        for name in ["e1", "e2", "e3", "e4", "e5"] {
            std::fs::write(bucket.join(name), name).unwrap();
        }

        let mut report = RecoveryReport::default();
        let mut hook = DeleteOnEntry {
            target: bucket.join("e3"),
        };
        sweep_untracked(
            &root,
            &HashSet::new(),
            &HashSet::new(),
            &mut report,
            &mut hook,
        )
        .expect("a vanished entry must not abort the sweep");

        for kept in ["e1", "e2", "e4", "e5"] {
            assert!(
                report.swept_dirs.iter().any(|p| p.ends_with(kept)),
                "{kept} should have been swept: {report:?}"
            );
        }
        assert!(
            !report.swept_dirs.iter().any(|p| p.ends_with("e3")),
            "the vanished entry must be skipped, not recorded: {report:?}"
        );
        assert!(!bucket.exists(), "the emptied bucket must be removed");
    }

    /// A symlink buried three levels inside a bucket subtree is refused as a
    /// whole; nothing under it is deleted and the target is untouched.
    #[cfg(unix)]
    #[tokio::test]
    async fn sweep_refuses_a_symlink_deep_inside_a_bucket_subtree() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("wt");
        let victim = dir.path().join("victim");
        std::fs::create_dir_all(&victim).unwrap();
        std::fs::write(victim.join("marker.txt"), "VICTIM").unwrap();
        let bucket = root.join("bucket");
        std::fs::create_dir_all(bucket.join("sub/inner")).unwrap();
        std::fs::write(bucket.join("sub/inner/real.txt"), "real").unwrap();
        std::os::unix::fs::symlink(&victim, bucket.join("sub/inner/escape")).unwrap();

        let mut report = RecoveryReport::default();
        sweep_untracked(
            &root,
            &HashSet::new(),
            &HashSet::new(),
            &mut report,
            &mut NoSweepHook,
        )
        .unwrap();

        assert!(
            report
                .refused_symlinks
                .iter()
                .any(|p| p.ends_with("bucket/sub")),
            "the whole subtree containing a symlink must be refused: {report:?}"
        );
        assert_eq!(
            std::fs::read_to_string(victim.join("marker.txt")).unwrap(),
            "VICTIM"
        );
        assert!(
            bucket.join("sub/inner/real.txt").exists(),
            "nothing under the refused subtree may be partially deleted"
        );
    }

    // --- LOCK-B-iii: never reap a worktree that is not ours ---

    /// QA-B's exact scenario (`/tmp/opencode/qa-b4/B3.sh`): a second engine with
    /// a **fresh, empty** ledger and a replaced root inode must never reap the
    /// first engine's live worktrees. The engine's live worktrees are
    /// git-`locked`; recovery refuses them regardless of lock identity, and the
    /// scratchpad sweep never reaps a *registered* worktree (even one caught
    /// mid-create, before `git worktree lock` has run).
    #[cfg(unix)]
    #[tokio::test]
    async fn second_engine_never_reaps_another_engines_worktrees() {
        let dir = tempfile::tempdir().unwrap();
        // Engine A: its repo and its live worktree root.
        let repo_a = scratch_repo(&dir.path().join("a"));
        let root = dir.path().join("shared");
        let held = crate::lock::StudioLock::acquire(&repo_a, &root).unwrap();

        // LOCK-B-iii inode bypass: the root directory is replaced under A, and
        // A keeps creating worktrees under the *path* (now a new inode).
        std::fs::rename(&root, dir.path().join("shared.old")).unwrap();
        std::fs::create_dir(&root).unwrap();

        let locked = WorktreeManager::path_for(&root, "scratch", "live");
        let midway = WorktreeManager::path_for(&root, "scratch", "mid-create");
        std::fs::create_dir_all(locked.parent().unwrap()).unwrap();
        for p in [&locked, &midway] {
            git(
                &repo_a,
                &[
                    "worktree",
                    "add",
                    "--detach",
                    "--no-checkout",
                    p.to_str().unwrap(),
                    "HEAD",
                ],
            );
        }
        git(
            &repo_a,
            &[
                "worktree",
                "lock",
                "--reason",
                // A realistic foreign owner: a proper `studio:<id>` proof from
                // ANOTHER engine (not ours, not the legacy bare word).
                "studio:another-engine-0000000000000001",
                locked.to_str().unwrap(),
            ],
        );
        std::fs::write(locked.join("precious.txt"), "PRECIOUS").unwrap();
        std::fs::write(midway.join("precious.txt"), "PRECIOUS").unwrap();

        // Engine B: a *fresh* repo and a *fresh, empty* ledger, same root path.
        let repo_b = scratch_repo(&dir.path().join("b"));
        let store_b = StateStore::open(dir.path().join("dbB").to_str().unwrap()).unwrap();
        assert!(store_b.worktree_paths().unwrap().is_empty());

        let report = recover(&store_b, &repo_b, &root).await.unwrap();

        assert!(
            report.refused_locked.iter().any(|p| p.ends_with("live")),
            "the live locked worktree must be refused: {report:?}"
        );
        assert!(
            report
                .refused_worktrees
                .iter()
                .any(|p| p.ends_with("mid-create")),
            "a registered mid-create worktree must be refused too: {report:?}"
        );
        assert!(
            report.swept_dirs.is_empty(),
            "nothing may be swept: {report:?}"
        );
        assert!(
            locked.join("precious.txt").exists(),
            "the live locked worktree was destroyed"
        );
        assert!(
            midway.join("precious.txt").exists(),
            "the mid-create worktree was destroyed"
        );
        // git still reports the live worktree as locked, and A's admin is intact.
        let entries = WorktreeManager::list_worktree_entries(&repo_a)
            .await
            .unwrap();
        assert!(
            entries
                .iter()
                .any(|e| e.locked && Path::new(&e.path).ends_with("live")),
            "A must still own its locked worktree: {entries:?}"
        );
        // B invented no ledger rows for A's worktrees.
        assert!(store_b.worktree_paths().unwrap().is_empty());
        // Honest: a refusal means the pass is not clean.
        assert!(!report.clean(), "{report:?}");

        // Idempotent, and still nothing deleted.
        let again = recover(&store_b, &repo_b, &root).await.unwrap();
        assert!(
            again.refused_locked.iter().any(|p| p.ends_with("live")),
            "{again:?}"
        );
        assert!(locked.exists() && midway.exists());
        drop(held);
    }

    /// LOCK-B-iii must not freeze ordinary GC: a `creating` row we wrote
    /// ourselves is **ours**, so recovery still unlocks and reaps it even though
    /// a crash after `git worktree lock` left it git-`locked` — provided the
    /// live lock reason is our own `studio:<engine id>` proof (B-EXEMPT-FORGE-01).
    #[cfg(unix)]
    #[tokio::test]
    async fn recovery_reaps_our_own_locked_half_created_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let repo = scratch_repo(dir.path());
        let root = dir.path().join("wt");
        let store = StateStore::open(dir.path().join("db").to_str().unwrap()).unwrap();
        let ours = crate::worktree::lock_reason_for(&store.engine_id().unwrap());

        let p = WorktreeManager::path_for(&root, "scratch", "half");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        store
            .begin_worktree(&p.to_string_lossy(), &repo.to_string_lossy(), "half")
            .unwrap();
        git(
            &repo,
            &[
                "worktree",
                "add",
                "--detach",
                "--no-checkout",
                p.to_str().unwrap(),
                "HEAD",
            ],
        );
        git(
            &repo,
            &["worktree", "lock", "--reason", &ours, p.to_str().unwrap()],
        );
        assert!(
            crate::worktree::locked_worktree_at(&p),
            "precondition: the half-create is git-locked"
        );

        let report = recover(&store, &repo, &root).await.unwrap();
        assert!(
            report.reaped_creating.iter().any(|x| x.ends_with("half")),
            "{report:?}"
        );
        assert!(
            report
                .reaped_git_orphans
                .iter()
                .any(|x| x.ends_with("half")),
            "our own locked half-create must still be unlocked + reaped: {report:?}"
        );
        assert!(report.refused_locked.is_empty(), "{report:?}");
        assert!(
            !p.exists(),
            "our own abandoned worktree must still be reaped"
        );

        let again = recover(&store, &repo, &root).await.unwrap();
        assert!(again.clean(), "{again:?}");
    }

    /// The git-orphan pass (step c) also refuses a `locked` worktree that is not
    /// ours: it is neither unlocked nor pruned, and its admin entry survives.
    /// The legacy bare `daemon` reason (pre-engine-id worktrees) is refused by
    /// default — never reap what cannot be attributed (migration rule).
    #[cfg(unix)]
    #[tokio::test]
    async fn git_pass_refuses_a_locked_orphan_of_our_own_repo() {
        let dir = tempfile::tempdir().unwrap();
        let repo = scratch_repo(dir.path());
        let root = dir.path().join("wt");
        let store = StateStore::open(dir.path().join("db").to_str().unwrap()).unwrap();

        // Live and locked, but unknown to the ledger (e.g. another engine
        // sharing this repo, or an inconsistent ledger).
        let p = WorktreeManager::path_for(&root, "scratch", "foreign");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        git(
            &repo,
            &[
                "worktree",
                "add",
                "--detach",
                "--no-checkout",
                p.to_str().unwrap(),
                "HEAD",
            ],
        );
        git(
            &repo,
            &[
                "worktree",
                "lock",
                "--reason",
                "daemon",
                p.to_str().unwrap(),
            ],
        );

        let report = recover(&store, &repo, &root).await.unwrap();
        assert!(
            report.refused_locked.iter().any(|x| x.ends_with("foreign")),
            "{report:?}"
        );
        assert!(
            !report.swept_dirs.iter().any(|x| x.ends_with("foreign")),
            "{report:?}"
        );
        assert!(p.exists(), "a locked orphan must not be reaped");
        let entries = WorktreeManager::list_worktree_entries(&repo).await.unwrap();
        assert!(
            entries
                .iter()
                .any(|e| e.locked && Path::new(&e.path).ends_with("foreign")),
            "the lock must survive recovery: {entries:?}"
        );
    }

    /// An *unlocked* git worktree of this engine's repo with no ledger row is
    /// still reaped by the git pass — LOCK-B-iii has not turned recovery into a
    /// no-op.
    #[cfg(unix)]
    #[tokio::test]
    async fn recovery_still_reaps_our_own_unlocked_orphan_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let repo = scratch_repo(dir.path());
        let root = dir.path().join("wt");
        let store = StateStore::open(dir.path().join("db").to_str().unwrap()).unwrap();

        let p = WorktreeManager::path_for(&root, "scratch", "orphan");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        git(
            &repo,
            &[
                "worktree",
                "add",
                "--detach",
                "--no-checkout",
                p.to_str().unwrap(),
                "HEAD",
            ],
        );
        assert!(!crate::worktree::locked_worktree_at(&p));

        let report = recover(&store, &repo, &root).await.unwrap();
        assert!(
            report
                .reaped_git_orphans
                .iter()
                .any(|x| x.ends_with("orphan")),
            "{report:?}"
        );
        assert!(
            report.refused_locked.is_empty() && report.refused_worktrees.is_empty(),
            "{report:?}"
        );
        assert!(!p.exists(), "an unlocked orphan must still be reaped");

        let again = recover(&store, &repo, &root).await.unwrap();
        assert!(again.clean(), "{again:?}");
    }

    // --- B-EXEMPT-FORGE-01: the exemption must prove ownership ---

    /// A second engine forges `INSERT INTO worktrees VALUES('<victim>',…,
    /// 'creating',…)` into its OWN db (same repo). The victim's live lock
    /// reason belongs to the first engine, so recovery must REFUSE:
    /// no unlock, no reap, and the forged row is kept as evidence.
    #[cfg(unix)]
    #[tokio::test]
    async fn forged_creating_row_never_reaps_a_foreign_locked_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let repo = scratch_repo(dir.path());
        let root = dir.path().join("shared");
        let victim = WorktreeManager::path_for(&root, "scratch", "victim");
        std::fs::create_dir_all(victim.parent().unwrap()).unwrap();
        git(
            &repo,
            &[
                "worktree",
                "add",
                "--detach",
                "--no-checkout",
                victim.to_str().unwrap(),
                "HEAD",
            ],
        );
        // The FIRST engine's live lock: a proper proof, but not ours.
        git(
            &repo,
            &[
                "worktree",
                "lock",
                "--reason",
                "studio:first-engine-0000000000000001",
                victim.to_str().unwrap(),
            ],
        );
        std::fs::write(victim.join("precious.txt"), "PRECIOUS").unwrap();

        // The SECOND engine: its own db, its own engine id, one forged row.
        let store_b = StateStore::open(dir.path().join("dbB").to_str().unwrap()).unwrap();
        let id_b = store_b.engine_id().unwrap();
        assert!(
            !crate::worktree::lock_reason_for(&id_b).ends_with("first-engine-0000000000000001"),
            "the second engine must own a different id"
        );
        store_b
            .begin_worktree(&victim.to_string_lossy(), &repo.to_string_lossy(), "forged")
            .unwrap();

        let report = recover(&store_b, &repo, &root).await.unwrap();

        assert!(
            report.refused_locked.iter().any(|x| x.ends_with("victim")),
            "the forged row must be refused, not honoured: {report:?}"
        );
        assert!(
            report.reaped_creating.is_empty() && report.reaped_git_orphans.is_empty(),
            "nothing may be reaped for a forged row: {report:?}"
        );
        assert!(
            report.swept_dirs.is_empty(),
            "the sweep must not touch the victim either: {report:?}"
        );
        assert!(
            victim.join("precious.txt").exists(),
            "the victim worktree was destroyed"
        );
        // The lock survives: still locked, still the first engine's reason.
        let entries = WorktreeManager::list_worktree_entries(&repo).await.unwrap();
        let back = entries
            .iter()
            .find(|e| Path::new(&e.path).ends_with("victim"))
            .expect("victim must still be registered");
        assert!(back.locked, "{entries:?}");
        assert_eq!(
            back.lock_reason.as_deref(),
            Some("studio:first-engine-0000000000000001"),
            "{entries:?}"
        );
        // The forged row is KEPT as refused evidence — recovery must not drop
        // a row it refused to act on.
        assert_eq!(
            store_b.worktrees_in_state("creating").unwrap().len(),
            1,
            "the refused forged row must survive"
        );
        // And the refusal is stable: a second pass refuses again, still intact.
        let again = recover(&store_b, &repo, &root).await.unwrap();
        assert!(
            again.refused_locked.iter().any(|x| x.ends_with("victim")),
            "{again:?}"
        );
        assert!(victim.join("precious.txt").exists());
    }

    /// Restarting with the SAME database keeps the engine id, so our own
    /// previous-run half-create (crash after lock, before `mark_ready`) still
    /// unlocks+reaps — the exemption survives restarts but no one else's id.
    #[cfg(unix)]
    #[tokio::test]
    async fn restart_with_the_same_db_still_reaps_our_own_half_create() {
        let dir = tempfile::tempdir().unwrap();
        let repo = scratch_repo(dir.path());
        let root = dir.path().join("wt");
        let db = dir.path().join("db");

        let p = WorktreeManager::path_for(&root, "scratch", "half");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        let id_before = {
            let store = StateStore::open(db.to_str().unwrap()).unwrap();
            let id = store.engine_id().unwrap();
            let ours = crate::worktree::lock_reason_for(&id);
            store
                .begin_worktree(&p.to_string_lossy(), &repo.to_string_lossy(), "half")
                .unwrap();
            git(
                &repo,
                &[
                    "worktree",
                    "add",
                    "--detach",
                    "--no-checkout",
                    p.to_str().unwrap(),
                    "HEAD",
                ],
            );
            git(
                &repo,
                &["worktree", "lock", "--reason", &ours, p.to_str().unwrap()],
            );
            id
            // `store` drops here: the "crash" + process restart.
        };

        // "Restart": reopen the SAME database file.
        let store2 = StateStore::open(db.to_str().unwrap()).unwrap();
        assert_eq!(
            store2.engine_id().unwrap(),
            id_before,
            "the engine id must survive a restart"
        );
        let report = recover(&store2, &repo, &root).await.unwrap();
        assert!(
            report.reaped_creating.iter().any(|x| x.ends_with("half")),
            "{report:?}"
        );
        assert!(
            report
                .reaped_git_orphans
                .iter()
                .any(|x| x.ends_with("half")),
            "our own locked half-create must still be unlocked + reaped after restart: {report:?}"
        );
        assert!(report.refused_locked.is_empty(), "{report:?}");
        assert!(!p.exists());
        let again = recover(&store2, &repo, &root).await.unwrap();
        assert!(again.clean(), "{again:?}");
    }

    #[test]
    fn engine_ids_are_stable_per_db_and_unique_across_dbs() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.db");
        let b = dir.path().join("b.db");
        let id_a1 = StateStore::open(a.to_str().unwrap())
            .unwrap()
            .engine_id()
            .unwrap();
        assert!(!id_a1.is_empty());
        // Reopen: same id.
        let id_a2 = StateStore::open(a.to_str().unwrap())
            .unwrap()
            .engine_id()
            .unwrap();
        assert_eq!(id_a1, id_a2, "reopening must return the same id");
        // A different database: a different id (16 random bytes as hex).
        let id_b = StateStore::open(b.to_str().unwrap())
            .unwrap()
            .engine_id()
            .unwrap();
        assert_ne!(id_a1, id_b, "distinct databases must not share an id");
        assert!(
            id_b.chars().all(|c| c.is_ascii_hexdigit()),
            "unexpected id format: {id_b}"
        );
    }

    // --- C-NEWLINE-01: control-char paths are refused, never reaped ---

    /// The `evil\nlocked` repro: a registered (here even git-locked) worktree
    /// whose path contains a newline. Recovery must refuse the REAL path
    /// (dedicated class), never sweep it, never unlock it — and must not
    /// report any truncated phantom.
    #[cfg(unix)]
    #[tokio::test]
    async fn newline_path_worktree_is_refused_never_reaped_no_phantom() {
        let dir = tempfile::tempdir().unwrap();
        let repo = scratch_repo(dir.path());
        let root = dir.path().join("wt");
        std::fs::create_dir_all(&root).unwrap();
        let store = StateStore::open(dir.path().join("db").to_str().unwrap()).unwrap();

        let evil = root.join("evil\nlocked");
        git(
            &repo,
            &[
                "worktree",
                "add",
                "--detach",
                evil.to_str().unwrap(),
                "HEAD",
            ],
        );
        git(
            &repo,
            &[
                "worktree",
                "lock",
                "--reason",
                "daemon",
                evil.to_str().unwrap(),
            ],
        );
        std::fs::write(evil.join("precious.txt"), "PRECIOUS").unwrap();

        let report = recover(&store, &repo, &root).await.unwrap();

        let real = crate::worktree::normalize_path(&evil)
            .to_string_lossy()
            .into_owned();
        assert!(
            report.refused_control_paths.contains(&real),
            "the REAL newline path must be refused: {report:?}"
        );
        assert!(
            report.swept_dirs.is_empty(),
            "nothing with a control-char path may be swept: {report:?}"
        );
        assert!(
            report.reaped_git_orphans.is_empty(),
            "a control-char entry must never be reaped: {report:?}"
        );
        assert!(
            !report.refused_locked.iter().any(|x| x.ends_with("evil")),
            "no truncated phantom may be reported: {report:?}"
        );
        assert!(
            evil.join("precious.txt").exists(),
            "the real worktree was destroyed"
        );
        // Never unlocked either: still locked with its reason intact.
        let entries = WorktreeManager::list_worktree_entries(&repo).await.unwrap();
        let back = entries
            .iter()
            .find(|e| e.path == evil.to_string_lossy())
            .expect("the real entry must still be listed verbatim");
        assert!(back.locked, "{entries:?}");
    }

    /// The sweep's exact match covers the real path: a ledger-tracked
    /// (ready) worktree whose path contains a newline is skipped silently as
    /// live — not swept, not refused, report clean.
    #[cfg(unix)]
    #[tokio::test]
    async fn tracked_newline_path_is_skipped_by_exact_match() {
        let dir = tempfile::tempdir().unwrap();
        let repo = scratch_repo(dir.path());
        let root = dir.path().join("wt");
        std::fs::create_dir_all(&root).unwrap();
        let store = StateStore::open(dir.path().join("db").to_str().unwrap()).unwrap();

        let evil = root.join("evil\nlocked");
        git(
            &repo,
            &[
                "worktree",
                "add",
                "--detach",
                evil.to_str().unwrap(),
                "HEAD",
            ],
        );
        std::fs::write(evil.join("precious.txt"), "PRECIOUS").unwrap();
        // A pre-existing ledger row for the real path (e.g. written before
        // create-time refusal existed): the sweep must match it exactly.
        store
            .begin_worktree(&evil.to_string_lossy(), &repo.to_string_lossy(), "evil")
            .unwrap();
        store.mark_worktree_ready(&evil.to_string_lossy()).unwrap();

        let report = recover(&store, &repo, &root).await.unwrap();
        assert!(
            report.clean(),
            "a tracked live path must be skipped: {report:?}"
        );
        assert!(evil.join("precious.txt").exists());
    }

    // --- A-DB-COPY-01: a copied DB plus a forged row refuses, victim intact ---

    /// Full copy attack at the library level: `cp` the victim DB, forge a
    /// `creating` row for the victim's live locked worktree in the COPY, run
    /// recovery on the copy. The copy must have diverged to its own engine id
    /// on open, so the forged row mismatches the victim's
    /// `studio:<original-id>` lock reason: refused, victim intact, forged row
    /// kept. The original engine is unaffected.
    #[cfg(unix)]
    #[tokio::test]
    async fn db_copy_with_forged_row_refuses_and_original_is_unaffected() {
        let dir = tempfile::tempdir().unwrap();
        let repo = scratch_repo(dir.path());
        let root = dir.path().join("wt");
        let db_victim = dir.path().join("victim.db");
        let db_copy = dir.path().join("copy.db");

        // The victim engine: one live locked worktree, id stamped as the lock
        // reason exactly the way the daemon does it.
        let victim = WorktreeManager::path_for(&root, "scratch", "live");
        std::fs::create_dir_all(victim.parent().unwrap()).unwrap();
        let id_victim = {
            let store = StateStore::open(db_victim.to_str().unwrap()).unwrap();
            let id = store.engine_id().unwrap();
            let ours = crate::worktree::lock_reason_for(&id);
            git(
                &repo,
                &[
                    "worktree",
                    "add",
                    "--detach",
                    "--no-checkout",
                    victim.to_str().unwrap(),
                    "HEAD",
                ],
            );
            git(
                &repo,
                &[
                    "worktree",
                    "lock",
                    "--reason",
                    &ours,
                    victim.to_str().unwrap(),
                ],
            );
            store
                .begin_worktree(&victim.to_string_lossy(), &repo.to_string_lossy(), "live")
                .unwrap();
            store
                .mark_worktree_ready(&victim.to_string_lossy())
                .unwrap();
            std::fs::write(victim.join("precious.txt"), "PRECIOUS").unwrap();
            store.checkpoint().unwrap();
            id
        };
        // `cp` the database file, then open the copy: it must diverge.
        std::fs::copy(&db_victim, &db_copy).unwrap();
        let store_c = StateStore::open(db_copy.to_str().unwrap()).unwrap();
        assert_ne!(
            store_c.engine_id().unwrap(),
            id_victim,
            "the copy must diverge to its own engine id on first open"
        );
        // Attacker surgery on the COPY only: flip the victim row to `creating`.
        rusqlite::Connection::open(&db_copy)
            .unwrap()
            .execute_batch(&format!(
                "UPDATE worktrees SET state='creating' WHERE path='{}'",
                victim.to_string_lossy()
            ))
            .unwrap();

        let report = recover(&store_c, &repo, &root).await.unwrap();
        assert!(
            report.refused_locked.iter().any(|x| x.ends_with("live")),
            "the copy's forged row must be refused: {report:?}"
        );
        assert!(
            report.reaped_creating.is_empty() && report.reaped_git_orphans.is_empty(),
            "nothing may be reaped from the copy: {report:?}"
        );
        assert!(
            victim.join("precious.txt").exists(),
            "the victim worktree was destroyed"
        );
        // The victim's lock still names the ORIGINAL engine.
        let entries = WorktreeManager::list_worktree_entries(&repo).await.unwrap();
        let back = entries
            .iter()
            .find(|e| Path::new(&e.path).ends_with("live"))
            .expect("victim still registered");
        assert_eq!(
            back.lock_reason.as_deref(),
            Some(crate::worktree::lock_reason_for(&id_victim).as_str()),
            "{entries:?}"
        );
        // The original engine is unaffected: same id, victim still `ready`,
        // and its own gc is clean (victim is ledger-tracked live).
        let store_v = StateStore::open(db_victim.to_str().unwrap()).unwrap();
        assert_eq!(store_v.engine_id().unwrap(), id_victim);
        let v = verify(&store_v, &repo, &root).await.unwrap();
        assert!(v.clean(), "the victim engine must stay clean: {v:?}");
        let own = recover(&store_v, &repo, &root).await.unwrap();
        assert!(own.clean(), "the victim's own gc must be clean: {own:?}");
    }

    // --- D-MIGRATION-01: pre-fix-shaped DB copied before first post-fix open ---

    /// Full kill chain on the pre-fix shape: strip the pair (legacy shape —
    /// also attacker-reachable via one `sqlite DELETE`), `cp` BEFORE any
    /// post-fix open, open both (ids must DIVERGE, neither keeps the legacy
    /// id), forge a `creating` row for the victim's live locked worktree in
    /// the COPY, run recovery on the copy. The forged row must be refused
    /// and the victim intact.
    #[cfg(unix)]
    #[tokio::test]
    async fn prefix_shaped_copy_with_forged_row_refuses_and_victim_survives() {
        let dir = tempfile::tempdir().unwrap();
        let repo = scratch_repo(dir.path());
        let root = dir.path().join("wt");
        let db_victim = dir.path().join("victim.db");
        let db_copy = dir.path().join("copy.db");

        let victim = WorktreeManager::path_for(&root, "scratch", "live");
        std::fs::create_dir_all(victim.parent().unwrap()).unwrap();
        let id_legacy = {
            let store = StateStore::open(db_victim.to_str().unwrap()).unwrap();
            let id = store.engine_id().unwrap();
            // NOTE: the victim's live lock is stamped with the POST-fix id
            // below (after migration), not this legacy id — pre-fix binaries
            // never issued `studio:<id>` reasons, so nothing live can
            // reference the discarded id.
            store
                .begin_worktree(&victim.to_string_lossy(), &repo.to_string_lossy(), "live")
                .unwrap();
            store
                .mark_worktree_ready(&victim.to_string_lossy())
                .unwrap();
            store.checkpoint().unwrap();
            id
        };
        // Manufacture the pre-fix shape, then copy BEFORE any post-fix open.
        rusqlite::Connection::open(&db_victim)
            .unwrap()
            .execute_batch("DELETE FROM engine_meta WHERE key IN ('engine_dev','engine_ino')")
            .unwrap();
        std::fs::copy(&db_victim, &db_copy).unwrap();
        // First post-fix open of BOTH files: both must abandon the legacy id
        // and diverge from each other.
        let id_victim = StateStore::open(db_victim.to_str().unwrap())
            .unwrap()
            .engine_id()
            .unwrap();
        let store_c = StateStore::open(db_copy.to_str().unwrap()).unwrap();
        let id_copy = store_c.engine_id().unwrap();
        assert_ne!(id_victim, id_legacy, "victim must abandon the legacy id");
        assert_ne!(id_copy, id_legacy, "copy must abandon the legacy id");
        assert_ne!(
            id_victim, id_copy,
            "pre-fix-shaped original and copy must not share an id"
        );
        // Stamp the victim's live lock with the victim's CURRENT (post-fix)
        // id, exactly the way the daemon does it.
        let ours = crate::worktree::lock_reason_for(&id_victim);
        git(
            &repo,
            &[
                "worktree",
                "add",
                "--detach",
                "--no-checkout",
                victim.to_str().unwrap(),
                "HEAD",
            ],
        );
        git(
            &repo,
            &[
                "worktree",
                "lock",
                "--reason",
                &ours,
                victim.to_str().unwrap(),
            ],
        );
        std::fs::write(victim.join("precious.txt"), "PRECIOUS").unwrap();
        // Attacker surgery on the COPY only: flip the victim row to `creating`.
        rusqlite::Connection::open(&db_copy)
            .unwrap()
            .execute_batch(&format!(
                "UPDATE worktrees SET state='creating' WHERE path='{}'",
                victim.to_string_lossy()
            ))
            .unwrap();

        let report = recover(&store_c, &repo, &root).await.unwrap();
        assert!(
            report.refused_locked.iter().any(|x| x.ends_with("live")),
            "the copy's forged row must be refused: {report:?}"
        );
        assert!(
            report.reaped_creating.is_empty() && report.reaped_git_orphans.is_empty(),
            "nothing may be reaped from the copy: {report:?}"
        );
        assert!(
            victim.join("precious.txt").exists(),
            "the victim worktree was destroyed"
        );
        // The original engine is unaffected: same post-fix id, victim still
        // `ready`, and its own gc is clean.
        let store_v = StateStore::open(db_victim.to_str().unwrap()).unwrap();
        assert_eq!(store_v.engine_id().unwrap(), id_victim);
        let own = recover(&store_v, &repo, &root).await.unwrap();
        assert!(own.clean(), "the victim's own gc must be clean: {own:?}");
    }

    // --- C-UNLOCK-TRAP-01: unlock never re-arms; release is the recourse ---

    /// Forge → gc refuses (persisted) → `git worktree unlock` (the old
    /// documented remedy) → gc MUST still refuse, victim intact, across
    /// several runs. Pre-fix the second gc reaped the victim (the trap).
    #[cfg(unix)]
    #[tokio::test]
    async fn unlock_after_refuse_still_refuses_and_victim_survives() {
        let dir = tempfile::tempdir().unwrap();
        let repo = scratch_repo(dir.path());
        let root = dir.path().join("shared");
        let victim = WorktreeManager::path_for(&root, "scratch", "victim");
        std::fs::create_dir_all(victim.parent().unwrap()).unwrap();
        git(
            &repo,
            &[
                "worktree",
                "add",
                "--detach",
                "--no-checkout",
                victim.to_str().unwrap(),
                "HEAD",
            ],
        );
        git(
            &repo,
            &[
                "worktree",
                "lock",
                "--reason",
                "studio:first-engine-0000000000000001",
                victim.to_str().unwrap(),
            ],
        );
        std::fs::write(victim.join("precious.txt"), "PRECIOUS").unwrap();

        let store = StateStore::open(dir.path().join("dbB").to_str().unwrap()).unwrap();
        store
            .begin_worktree(&victim.to_string_lossy(), &repo.to_string_lossy(), "forged")
            .unwrap();

        let first = recover(&store, &repo, &root).await.unwrap();
        assert!(
            first.refused_locked.iter().any(|x| x.ends_with("victim")),
            "first pass must refuse the forged row: {first:?}"
        );
        assert!(
            store
                .refused_reason(&victim.to_string_lossy())
                .unwrap()
                .is_some(),
            "the refusal must be persisted"
        );

        // The old remedy: unlock, then gc. This must NOT re-arm auto-reap.
        git(&repo, &["worktree", "unlock", victim.to_str().unwrap()]);
        for round in 0..3 {
            let report = recover(&store, &repo, &root).await.unwrap();
            let refused_any = !report.refused_locked.is_empty()
                || !report.refused_worktrees.is_empty()
                || !report.refused_persisted.is_empty();
            assert!(
                refused_any,
                "round {round}: an unlocked-but-refused path must still be refused: {report:?}"
            );
            assert!(
                report.reaped_creating.is_empty()
                    && report.reaped_git_orphans.is_empty()
                    && report.swept_dirs.is_empty(),
                "round {round}: nothing may be reaped for a refused path: {report:?}"
            );
            assert!(
                victim.join("precious.txt").exists(),
                "round {round}: the victim was destroyed"
            );
            assert!(
                !report.clean(),
                "round {round}: refusal is unclean: {report:?}"
            );
        }
    }

    /// Explicit recourse: after refuse + unlock, `clear_refused` (+ dropping
    /// the kept `creating` row, exactly what `studio release` does) lets the
    /// next gc treat the path as an ordinary orphan — an unlocked stale path
    /// is then reaped and the tree converges to clean.
    #[cfg(unix)]
    #[tokio::test]
    async fn release_clears_refusal_so_an_unlocked_stale_path_reaps() {
        let dir = tempfile::tempdir().unwrap();
        let repo = scratch_repo(dir.path());
        let root = dir.path().join("shared");
        let victim = WorktreeManager::path_for(&root, "scratch", "victim");
        std::fs::create_dir_all(victim.parent().unwrap()).unwrap();
        git(
            &repo,
            &[
                "worktree",
                "add",
                "--detach",
                "--no-checkout",
                victim.to_str().unwrap(),
                "HEAD",
            ],
        );
        git(
            &repo,
            &[
                "worktree",
                "lock",
                "--reason",
                "studio:first-engine-0000000000000001",
                victim.to_str().unwrap(),
            ],
        );
        std::fs::write(victim.join("precious.txt"), "PRECIOUS").unwrap();

        let store = StateStore::open(dir.path().join("dbB").to_str().unwrap()).unwrap();
        store
            .begin_worktree(&victim.to_string_lossy(), &repo.to_string_lossy(), "forged")
            .unwrap();
        let first = recover(&store, &repo, &root).await.unwrap();
        assert!(
            first.refused_locked.iter().any(|x| x.ends_with("victim")),
            "{first:?}"
        );
        git(&repo, &["worktree", "unlock", victim.to_str().unwrap()]);

        // Still refused before the release (the trap is closed) …
        let held = recover(&store, &repo, &root).await.unwrap();
        assert!(!held.clean(), "{held:?}");
        assert!(victim.exists());

        // … then the explicit operator recourse (`studio release <path>`).
        assert!(store.clear_refused(&victim.to_string_lossy()).unwrap());
        store.delete_worktree(&victim.to_string_lossy()).unwrap();
        let freed = recover(&store, &repo, &root).await.unwrap();
        assert!(
            freed
                .reaped_git_orphans
                .iter()
                .any(|x| x.ends_with("victim")),
            "an unlocked, released stale path must reap: {freed:?}"
        );
        assert!(!victim.exists(), "the stale path must be gone");
        let again = recover(&store, &repo, &root).await.unwrap();
        assert!(again.clean(), "the tree must converge: {again:?}");
    }

    /// A refused row is never auto-reaped across N gc runs while it stays
    /// refused: the forged `creating` row survives as evidence and the
    /// worktree survives on disk.
    #[cfg(unix)]
    #[tokio::test]
    async fn refused_row_is_never_auto_reaped_across_gc_runs() {
        let dir = tempfile::tempdir().unwrap();
        let repo = scratch_repo(dir.path());
        let root = dir.path().join("shared");
        let victim = WorktreeManager::path_for(&root, "scratch", "victim");
        std::fs::create_dir_all(victim.parent().unwrap()).unwrap();
        git(
            &repo,
            &[
                "worktree",
                "add",
                "--detach",
                "--no-checkout",
                victim.to_str().unwrap(),
                "HEAD",
            ],
        );
        git(
            &repo,
            &[
                "worktree",
                "lock",
                "--reason",
                "studio:first-engine-0000000000000001",
                victim.to_str().unwrap(),
            ],
        );
        std::fs::write(victim.join("precious.txt"), "PRECIOUS").unwrap();

        let store = StateStore::open(dir.path().join("dbB").to_str().unwrap()).unwrap();
        store
            .begin_worktree(&victim.to_string_lossy(), &repo.to_string_lossy(), "forged")
            .unwrap();

        for round in 0..5 {
            let report = recover(&store, &repo, &root).await.unwrap();
            assert!(
                report.refused_locked.iter().any(|x| x.ends_with("victim")),
                "round {round}: must stay refused: {report:?}"
            );
            assert!(
                report.reaped_creating.is_empty() && report.reaped_git_orphans.is_empty(),
                "round {round}: never reaped: {report:?}"
            );
            assert_eq!(
                store.worktrees_in_state("creating").unwrap().len(),
                1,
                "round {round}: the refused row must survive as evidence"
            );
            assert!(
                victim.join("precious.txt").exists(),
                "round {round}: victim intact"
            );
        }
    }
}
