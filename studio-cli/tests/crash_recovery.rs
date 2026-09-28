//! Crash storm: SIGKILL the single-writer daemon mid-flight, then prove the
//! database is consistent and boot-time GC leaves zero orphans.
//!
//! `Child::kill()` sends `SIGKILL` on Unix, so this exercises the exact
//! "crash anytime" path the phase requires: no unwinding, no graceful close,
//! no chance for the daemon to clean up. Each round is followed by `studio gc`
//! (boot-time recovery) and `studio verify` (read-only integrity projection).
//!
//! The shell driver `scripts/crash_storm.sh` scales this to 50 rounds with
//! `kill -9`; this test keeps the same invariant inside `cargo test`.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

const ROUNDS: usize = 5;

fn studio(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_studio"))
        .args(args)
        .output()
        .expect("studio binary runs")
}

fn git(repo: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git runs");
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

fn paths(dir: &Path) -> (String, String, String) {
    (
        dir.join("studio.db").to_string_lossy().to_string(),
        dir.join("repo").to_string_lossy().to_string(),
        dir.join("worktrees").to_string_lossy().to_string(),
    )
}

#[test]
fn sigkill_storm_leaves_consistent_db_and_zero_orphans() {
    let dir = tempfile::tempdir().unwrap();
    let repo = scratch_repo(dir.path());
    let (db, repo_s, root) = paths(dir.path());
    assert!(repo.exists());

    // Initialise so the first daemon boot has a v2 database to recover.
    assert!(studio(&["init", "--db", &db]).status.success());

    for round in 0..ROUNDS {
        let mut child = Command::new(env!("CARGO_BIN_EXE_studio"))
            .args([
                "daemon",
                "--db",
                &db,
                "--repo",
                &repo_s,
                "--worktree-root",
                &root,
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("daemon spawns");

        std::thread::sleep(Duration::from_millis(120 + (round as u64) * 40));
        child.kill().expect("SIGKILL"); // SIGKILL on Unix
        child.wait().expect("reaped");

        // Let any git child orphaned by the SIGKILL finish before we reconcile,
        // so the recovery pass sees a quiescent filesystem.
        std::thread::sleep(Duration::from_millis(250));

        let gc = studio(&[
            "gc",
            "--db",
            &db,
            "--repo",
            &repo_s,
            "--worktree-root",
            &root,
        ]);
        // gc exits 0 when clean, 1 when it reaped/refused (report not clean)
        // — matching `verify`'s contract. Only exit >= 2 is a hard failure.
        let gc_code = gc.status.code().unwrap_or(99);
        assert!(
            gc_code <= 1,
            "round {round}: gc hard-failed (exit {gc_code}): {}",
            String::from_utf8_lossy(&gc.stderr)
        );
        if gc_code == 1 {
            let gc_report: serde_json::Value = serde_json::from_slice(&gc.stdout).unwrap();
            assert_eq!(
                gc_report["clean"], false,
                "round {round}: gc exit 1 must mean a non-clean report: {gc_report}"
            );
        }

        let verify = studio(&[
            "verify",
            "--db",
            &db,
            "--repo",
            &repo_s,
            "--worktree-root",
            &root,
        ]);
        assert!(
            verify.status.success(),
            "round {round}: verify not clean: {}",
            String::from_utf8_lossy(&verify.stdout)
        );
        let report: serde_json::Value = serde_json::from_slice(&verify.stdout).unwrap();
        assert_eq!(report["clean"], true, "round {round}: {report}");
        assert_eq!(report["creating_rows"], 0);
        assert_eq!(report["missing_rows"], 0);
        assert_eq!(report["integrity_ok"], true);
    }

    // `studio status` is a pure projection read with no daemon alive.
    let status = studio(&["status", "--db", &db]);
    assert!(status.status.success());
    let snap: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(snap["source"], "studio.db");
    assert_eq!(snap["schema_version"], 2);
    assert_eq!(snap["read_only"], true);
}

/// B: the engine lock is keyed to the shared repo/worktree root, not the DB
/// path. A second engine on a *different* database but the same repo/root must
/// be refused, and unlinking any `<db>.studio-lock` must not change that.
#[test]
fn lock_is_keyed_to_shared_resource_not_db_path() {
    use std::io::BufRead;
    use std::io::BufReader;
    use std::sync::mpsc;

    let dir = tempfile::tempdir().unwrap();
    let repo = scratch_repo(dir.path());
    let (db, repo_s, root) = paths(dir.path());
    assert!(repo.exists());
    let db_b = dir.path().join("dbB").to_string_lossy().to_string();
    assert!(studio(&["init", "--db", &db]).status.success());
    assert!(studio(&["init", "--db", &db_b]).status.success());

    let mut daemon = Command::new(env!("CARGO_BIN_EXE_studio"))
        .args([
            "daemon",
            "--db",
            &db,
            "--repo",
            &repo_s,
            "--worktree-root",
            &root,
            "--ticks",
            "100000",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("daemon spawns");

    // Wait until the engine lock is held. The daemon prints its ready marker
    // after acquiring the lock and finishing boot recovery.
    let stderr = daemon.stderr.take().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines().map_while(Result::ok) {
            if line.contains("wal soft bound") {
                let _ = tx.send(());
                break;
            }
        }
    });
    rx.recv_timeout(Duration::from_secs(15))
        .expect("daemon acquired the engine lock");

    // Old, wrong identity: the db-side lockfile. Removing it must not allow a
    // second engine (it is not the lock identity any more).
    let _ = std::fs::remove_file(format!("{db}.studio-lock"));

    // A second daemon on a DIFFERENT db but the SAME repo/root is refused.
    let second = studio(&[
        "daemon",
        "--db",
        &db_b,
        "--repo",
        &repo_s,
        "--worktree-root",
        &root,
        "--ticks",
        "1",
    ]);
    assert!(
        !second.status.success(),
        "second engine on the shared repo/root must be refused: {}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert!(
        String::from_utf8_lossy(&second.stderr).contains("already owns this repo/worktree root"),
        "typed refusal expected: {}",
        String::from_utf8_lossy(&second.stderr)
    );

    // `gc --db dbB` on the same root is refused while daemon-A is alive.
    let gc = studio(&[
        "gc",
        "--db",
        &db_b,
        "--repo",
        &repo_s,
        "--worktree-root",
        &root,
    ]);
    assert!(
        !gc.status.success(),
        "gc on a different db must not run while the shared engine lock is held: {}",
        String::from_utf8_lossy(&gc.stdout)
    );

    // The first daemon is still alive.
    assert!(
        daemon.try_wait().unwrap().is_none(),
        "daemon-A must survive"
    );

    daemon.kill().expect("SIGKILL");
    daemon.wait().expect("reaped");
}
