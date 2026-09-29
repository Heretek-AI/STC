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

/// True when `pid` names a live process. Unix: `/proc/<pid>` exists
/// (equivalent to kill-0 for our fail-closed purpose). Non-unix: a parseable
/// pid file is taken at face value (documented residual).
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

/// Explicit-dir probe (tests use this so parallel tests never fight over the
/// process-global `STUDIO_RUN_DIR`).
pub fn probe_in(dir: &std::path::Path, expected_version: &str) -> DaemonDto {
    let pid_file = pid_path(dir).to_string_lossy().into_owned();
    let version_file = version_path(dir).to_string_lossy().into_owned();
    let pid_raw = std::fs::read_to_string(&pid_file)
        .ok()
        .map(|s| s.trim().to_owned());
    let version = std::fs::read_to_string(&version_file)
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());

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
    let Ok(pid) = pid_text.parse::<u32>() else {
        return DaemonDto {
            detail: format!(
                "daemon missing: unparseable PID file at {pid_file} ({pid_text:?}) — refusing to guess; serving read-only projection only"
            ),
            reachable: false,
            pid_file,
            version_file,
            pid: None,
            version,
        };
    };
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
    match version.clone() {
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
}
