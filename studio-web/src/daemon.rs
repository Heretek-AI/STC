//! Daemon ensure-running probe (f04-sandbox-daemon clean-room, Apache-2.0).
//!
//! Expected files (written by the daemon lane; read here, never written):
//! - `<run_dir>/studio-daemon.pid` — decimal pid of the live daemon.
//! - `<run_dir>/studio-daemon.version` — daemon build version (`<vers>` must
//!   equal this crate's `CARGO_PKG_VERSION`; mismatch = restart scope).
//!
//! This module only *reports*: `Missing` / `StalePid` / `VersionMismatch`
//! are legible typed outcomes surfaced on `/api/health` and the cockpit
//! banner. The web server never starts, restarts, or masks the daemon — a
//! missing daemon means read-only projection serving with a visible banner
//! (fail closed, legible), never silent degradation.

use crate::api::DaemonDto;

/// Where the probe looks. `STUDIO_RUN_DIR` wins; otherwise `./.studio-run`.
pub fn run_dir() -> std::path::PathBuf {
    std::env::var("STUDIO_RUN_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from(".studio-run"))
}

fn pid_path(dir: &std::path::Path) -> std::path::PathBuf {
    dir.join("studio-daemon.pid")
}

fn version_path(dir: &std::path::Path) -> std::path::PathBuf {
    dir.join("studio-daemon.version")
}

/// Maximum daemon version string length (V7 bound).
///
/// The version file is operator-written; without a bound a multi-MB file
/// would be loaded into memory and echoed into `/api/health` detail
/// (response bloat). Longer content is fail-closed `VersionMismatch` with a
/// truncated preview — never echoed in full, never treated as a match.
///
/// PID files get the same bound discipline (fix 6): the pid file is read
/// bounded (never a full 10M `read_to_string`) and echoed truncated.
pub const VERSION_MAX_LEN: usize = 128;
/// Bound for the pid-file echo (same 128-char discipline as the version).
pub const PID_MAX_LEN: usize = 128;
/// Bounded-read cap for probe files (pid + version): the probe never loads
/// more than this many bytes even when the operator file is megabytes.
pub const PROBE_READ_CAP: u64 = 4096;

/// True when `pid` names a live process. Unix: `/proc/<pid>` exists
/// (equivalent to kill-0 for our fail-closed purpose). Non-unix: a parseable
/// pid file is taken at face value (documented residual).
///
/// V6 PID-reuse note: `/proc` existence proves *a* process holds the number,
/// not that it is still the daemon (PID reuse window). The probe therefore
/// requires BOTH a live pid AND a version-file match; a reused pid with a
/// stale/missing version still reports missing/mismatch (fail closed).
/// Pids 0/1 can never be the daemon (idle/init) and are stale by construction.
fn pid_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        std::path::Path::new(&format!("/proc/{pid}")).exists()
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        true
    }
}

/// Probe the daemon lane. Never panics on missing/unparseable files.
pub fn probe(expected_version: &str) -> DaemonDto {
    probe_in(&run_dir(), expected_version)
}

/// Bounded file read: at most `PROBE_READ_CAP` bytes, plus the on-disk
/// total (from metadata) so huge files are reported truncated without ever
/// loading them fully. Returns `(head_text, total_len, was_truncated)`.
fn read_bounded(path: &std::path::Path) -> Option<(String, usize, bool)> {
    let meta_len = std::fs::metadata(path).ok().map(|m| m.len() as usize);
    let file = std::fs::File::open(path).ok()?;
    use std::io::Read as _;
    let mut take = file.take(PROBE_READ_CAP);
    let mut buf = Vec::new();
    take.read_to_end(&mut buf).ok()?;
    let total = meta_len.unwrap_or(buf.len());
    let truncated = total as u64 > PROBE_READ_CAP;
    // Lossy for the preview only; pid parse below fails closed on garbage.
    let text = String::from_utf8_lossy(&buf).into_owned();
    Some((text, total, truncated))
}

/// Truncated preview discipline (shared pid/version): first 64 chars +
/// `…[truncated N chars]` — never echo the full operator string.
fn truncated_preview(s: &str, total_len: usize) -> String {
    let preview: String = s.chars().take(64).collect();
    format!("{preview}…[truncated {total_len} chars]")
}

/// Explicit-dir probe (tests use this so parallel tests never fight over the
/// process-global `STUDIO_RUN_DIR`).
pub fn probe_in(dir: &std::path::Path, expected_version: &str) -> DaemonDto {
    let pid_file = pid_path(dir).to_string_lossy().into_owned();
    let version_file = version_path(dir).to_string_lossy().into_owned();
    // Fix 6: bounded reads — never a full 10M `read_to_string`. The preview
    // discipline truncates both pid and version echoes at 128 chars.
    let (pid_raw, pid_total) = match read_bounded(&pid_path(dir)) {
        Some((t, total, _)) => {
            let v = t.trim().to_owned();
            if v.is_empty() {
                (None, total)
            } else {
                (Some(v), total)
            }
        }
        None => (None, 0),
    };
    let pid_raw = pid_raw.filter(|s| !s.is_empty());
    let (version_text, version_total, version_capped) = match read_bounded(&version_path(dir)) {
        Some((t, total, capped)) => {
            let v = t.trim().to_owned();
            if v.is_empty() {
                (None, total, capped)
            } else {
                (Some(v), total, capped)
            }
        }
        None => (None, 0, false),
    };
    // A version file larger than the read cap counts as over-length even
    // when only the head was loaded (fail closed without loading MBs).
    let version_overlong = match &version_text {
        Some(v) => v.len() > VERSION_MAX_LEN || version_capped || version_total > VERSION_MAX_LEN,
        None => version_capped || version_total > VERSION_MAX_LEN,
    };
    let version = version_text.filter(|s| !s.is_empty());

    let Some(pid_text) = pid_raw.filter(|s| !s.is_empty()) else {
        return DaemonDto {
            detail: format!(
                "daemon missing: no PID file at {pid_file} — start it with `studio daemon`; serving read-only projection only"
            ),
            reachable: false,
            pid_file,
            version_file,
            pid: None,
            version,
        };
    };
    // Fix 6: pid echo bound like the version (128 + [truncated]). A 1M pid
    // file must not bloat the health response. `pid_total` is the on-disk
    // size so a capped read still reports the true length.
    if pid_text.len() > PID_MAX_LEN || pid_total > PID_MAX_LEN {
        let total = pid_total.max(pid_text.len());
        let preview = truncated_preview(&pid_text, total);
        return DaemonDto {
            detail: format!(
                "daemon missing: unparseable PID file at {pid_file} (exceeds {PID_MAX_LEN} chars; preview {preview:?}) — refusing to guess; serving read-only projection only"
            ),
            reachable: false,
            pid_file,
            version_file,
            pid: None,
            version,
        };
    }
    // Strict decimal discipline: optional `+`/`-`, hex, or whitespace
    // inside the number must not parse as a live pid (e.g. `+123` must not
    // report reachable). Only ASCII digits parse; everything else is
    // unparseable fail-closed.
    let pid_parsed: Option<u32> =
        if !pid_text.is_empty() && pid_text.chars().all(|c| c.is_ascii_digit()) {
            pid_text.parse::<u32>().ok()
        } else {
            None
        };
    let Some(pid) = pid_parsed else {
        // Truncate the echo even inside the bound (defence in depth).
        let shown = if pid_text.len() > 64 {
            truncated_preview(&pid_text, pid_text.len())
        } else {
            pid_text.clone()
        };
        return DaemonDto {
            detail: format!(
                "daemon missing: unparseable PID file at {pid_file} ({shown:?}) — refusing to guess; serving read-only projection only"
            ),
            reachable: false,
            pid_file,
            version_file,
            pid: None,
            version,
        };
    };
    // V6: pids 0/1 are never the daemon (idle/init). A forged pid file
    // naming them must not report reachable even though /proc/1 exists.
    if pid <= 1 {
        return DaemonDto {
            detail: format!(
                "daemon missing: stale PID file at {pid_file} (pid {pid} is never a daemon) — start it with `studio daemon`; serving read-only projection only"
            ),
            reachable: false,
            pid_file,
            version_file,
            pid: Some(pid),
            version,
        };
    }
    if !pid_alive(pid) {
        return DaemonDto {
            detail: format!(
                "daemon missing: stale PID file at {pid_file} (pid {pid} not running) — start it with `studio daemon`; serving read-only projection only"
            ),
            reachable: false,
            pid_file,
            version_file,
            pid: Some(pid),
            version,
        };
    }
    // Huge version file with only whitespace trims to empty: still a
    // length-bound mismatch, not degraded (never load it fully).
    if version.is_none() && version_overlong {
        let preview = String::new();
        return DaemonDto {
            detail: format!(
                "daemon version mismatch: version file at {version_file} exceeds {VERSION_MAX_LEN} chars (got {version_total}) — refusing to echo; preview {preview:?}…; serving read-only projection only"
            ),
            reachable: false,
            pid_file,
            version_file,
            pid: Some(pid),
            version: Some(format!("{preview}…[truncated {version_total} chars]")),
        };
    }
    match version.clone() {
        Some(v) if version_overlong || v.len() > VERSION_MAX_LEN => {
            // V7: bound the echoed version (fail closed, truncated preview).
            // `version_total` is the on-disk size, so a 10M file reports
            // truncated without ever loading it fully.
            let total = if version_total > v.len() {
                version_total
            } else {
                v.len()
            };
            let preview: String = v.chars().take(64).collect();
            DaemonDto {
                detail: format!(
                    "daemon version mismatch: version file at {version_file} exceeds {VERSION_MAX_LEN} chars (got {total}) — refusing to echo; preview {preview:?}…; serving read-only projection only"
                ),
                reachable: false,
                pid_file,
                version_file,
                pid: Some(pid),
                version: Some(format!("{preview}…[truncated {total} chars]")),
            }
        }
        Some(v) if v != expected_version => DaemonDto {
            detail: format!(
                "daemon version mismatch: running {v} but cockpit expects {expected_version} — restart the daemon; serving read-only projection only"
            ),
            reachable: false,
            pid_file,
            version_file,
            pid: Some(pid),
            version: Some(v),
        },
        Some(v) => DaemonDto {
            detail: format!("daemon running: pid {pid}, version {v}"),
            reachable: true,
            pid_file,
            version_file,
            pid: Some(pid),
            version: Some(v),
        },
        None => DaemonDto {
            detail: format!(
                "daemon degraded: pid {pid} running but no version file at {version_file} — cannot confirm version match; serving read-only projection only"
            ),
            reachable: false,
            pid_file,
            version_file,
            pid: Some(pid),
            version: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_daemon_fails_closed_legibly() {
        let dir = tempfile::tempdir().unwrap();
        let d = probe_in(dir.path(), "0.1.0");
        assert!(!d.reachable);
        assert!(d.pid.is_none());
        assert!(d.detail.contains("daemon missing"));
        assert!(d.detail.contains("studio daemon"));
    }

    #[test]
    fn stale_pid_is_missing_not_running() {
        let dir = tempfile::tempdir().unwrap();
        // An absurdly large pid is not running on any test machine.
        std::fs::write(dir.path().join("studio-daemon.pid"), b"4294967290\n").unwrap();
        let d = probe_in(dir.path(), "0.1.0");
        assert!(!d.reachable);
        assert!(d.detail.contains("stale PID file"));
    }

    #[test]
    fn live_pid_with_matching_version_is_reachable() {
        let dir = tempfile::tempdir().unwrap();
        let me = std::process::id();
        std::fs::write(dir.path().join("studio-daemon.pid"), format!("{me}\n")).unwrap();
        std::fs::write(dir.path().join("studio-daemon.version"), b"0.1.0\n").unwrap();
        let d = probe_in(dir.path(), "0.1.0");
        assert!(d.reachable);
        assert_eq!(d.pid, Some(me));
    }

    #[test]
    fn live_pid_with_wrong_version_is_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let me = std::process::id();
        std::fs::write(dir.path().join("studio-daemon.pid"), format!("{me}\n")).unwrap();
        std::fs::write(dir.path().join("studio-daemon.version"), b"9.9.9\n").unwrap();
        let d = probe_in(dir.path(), "0.1.0");
        assert!(!d.reachable);
        assert!(d.detail.contains("version mismatch"));
    }

    #[test]
    fn v6_pid_zero_and_one_are_stale_never_reachable() {
        // V6 PID-reuse hardening: 0/1 can never be the daemon even though
        // /proc/1 exists. FAILS pre-fix (pid 1 reported reachable when
        // /proc/1 exists + version matched); PASSES post-fix.
        for pid in [0u32, 1u32] {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join("studio-daemon.pid"), format!("{pid}\n")).unwrap();
            std::fs::write(dir.path().join("studio-daemon.version"), b"0.1.0\n").unwrap();
            let d = probe_in(dir.path(), "0.1.0");
            assert!(!d.reachable, "pid {pid} must never be reachable");
            assert!(
                d.detail.contains("stale PID file"),
                "pid {pid}: {}",
                d.detail
            );
            assert!(
                d.detail.contains("never a daemon"),
                "pid {pid}: {}",
                d.detail
            );
        }
    }

    #[test]
    fn v7_version_length_bound_is_fail_closed_truncated() {
        // V7 version-length bound: a huge version file must not bloat
        // /api/health. FAILS pre-fix (full multi-KB version echoed in
        // detail); PASSES post-fix (mismatch + truncated preview).
        let dir = tempfile::tempdir().unwrap();
        let me = std::process::id();
        std::fs::write(dir.path().join("studio-daemon.pid"), format!("{me}\n")).unwrap();
        let huge = "v".repeat(VERSION_MAX_LEN + 100);
        std::fs::write(dir.path().join("studio-daemon.version"), &huge).unwrap();
        let d = probe_in(dir.path(), "0.1.0");
        assert!(!d.reachable);
        assert!(d.detail.contains("exceeds"), "{}", d.detail);
        assert!(
            !d.detail.contains(&huge),
            "detail must not echo full version"
        );
        assert!(
            d.version.as_ref().unwrap().contains("[truncated"),
            "{:?}",
            d.version
        );
        // Boundary: exactly MAX_LEN still compares normally (mismatch, full echo ok).
        let dir2 = tempfile::tempdir().unwrap();
        std::fs::write(dir2.path().join("studio-daemon.pid"), format!("{me}\n")).unwrap();
        let at_bound = "w".repeat(VERSION_MAX_LEN);
        std::fs::write(dir2.path().join("studio-daemon.version"), &at_bound).unwrap();
        let d2 = probe_in(dir2.path(), "0.1.0");
        assert!(!d2.reachable);
        assert!(d2.detail.contains("version mismatch"), "{}", d2.detail);
    }

    #[test]
    fn x2_probe_oracle_all_arms_legible() {
        // X2 oracle: every probe arm reports a legible typed outcome with a
        // named file + fail-closed guidance (never a panic, never silent).
        let me = std::process::id();
        // Missing.
        let dir = tempfile::tempdir().unwrap();
        let d = probe_in(dir.path(), "0.1.0");
        assert!(!d.reachable && d.detail.contains("daemon missing"));
        // Unparseable.
        std::fs::write(dir.path().join("studio-daemon.pid"), b"not-a-pid\n").unwrap();
        let d = probe_in(dir.path(), "0.1.0");
        assert!(!d.reachable && d.detail.contains("unparseable"));
        // Stale (absurd pid).
        std::fs::write(dir.path().join("studio-daemon.pid"), b"4294967290\n").unwrap();
        let d = probe_in(dir.path(), "0.1.0");
        assert!(!d.reachable && d.detail.contains("stale PID file"));
        // Mismatch.
        std::fs::write(dir.path().join("studio-daemon.pid"), format!("{me}\n")).unwrap();
        std::fs::write(dir.path().join("studio-daemon.version"), b"9.9.9\n").unwrap();
        let d = probe_in(dir.path(), "0.1.0");
        assert!(!d.reachable && d.detail.contains("version mismatch"));
        // Reachable.
        std::fs::write(dir.path().join("studio-daemon.version"), b"0.1.0\n").unwrap();
        let d = probe_in(dir.path(), "0.1.0");
        assert!(d.reachable && d.detail.contains("daemon running"));
        // Degraded (live pid, no version file).
        let dir2 = tempfile::tempdir().unwrap();
        std::fs::write(dir2.path().join("studio-daemon.pid"), format!("{me}\n")).unwrap();
        let d = probe_in(dir2.path(), "0.1.0");
        assert!(!d.reachable && d.detail.contains("degraded"));
    }

    #[test]
    fn pid_file_bound_truncates_huge_echo() {
        // Fix 6: a 1M pid file must not bloat the health response — bounded
        // read + truncated echo. FAILS pre-fix (full 1M echoed in detail).
        let dir = tempfile::tempdir().unwrap();
        let me = std::process::id();
        let huge_pid = "1".repeat(1_000_000);
        std::fs::write(dir.path().join("studio-daemon.pid"), &huge_pid).unwrap();
        std::fs::write(dir.path().join("studio-daemon.version"), b"0.1.0\n").unwrap();
        let d = probe_in(dir.path(), "0.1.0");
        assert!(!d.reachable);
        assert!(
            !d.detail.contains(&huge_pid),
            "detail must not echo full pid file"
        );
        assert!(
            d.detail.len() < 10_000,
            "detail bound, got {}",
            d.detail.len()
        );
        // A live pid still parses when the file is small.
        let dir2 = tempfile::tempdir().unwrap();
        std::fs::write(dir2.path().join("studio-daemon.pid"), format!("{me}\n")).unwrap();
        std::fs::write(dir2.path().join("studio-daemon.version"), b"0.1.0\n").unwrap();
        let d2 = probe_in(dir2.path(), "0.1.0");
        assert!(d2.reachable);
    }

    #[test]
    fn version_huge_file_is_bounded_without_full_read() {
        // Fix 6 second leg: a 100KB version file reports truncated without
        // loading it fully (PROBE_READ_CAP discipline). FAILS pre-fix
        // (full echo, unbounded read_to_string).
        let dir = tempfile::tempdir().unwrap();
        let me = std::process::id();
        std::fs::write(dir.path().join("studio-daemon.pid"), format!("{me}\n")).unwrap();
        let huge = "v".repeat(100_000);
        std::fs::write(dir.path().join("studio-daemon.version"), &huge).unwrap();
        let d = probe_in(dir.path(), "0.1.0");
        assert!(!d.reachable);
        assert!(d.detail.contains("exceeds"), "{}", d.detail);
        assert!(!d.detail.contains(&huge));
        assert!(d.version.as_ref().unwrap().contains("[truncated"));
        assert!(d.detail.len() < 10_000, "{}", d.detail.len());
    }
}
