//! Harness manager (Phase 6 WS3): probe-then-install for agent CLIs.
//! Probes host CLIs first (`HarnessSource::Host`); installs only what's missing
//! into versioned `~/.studio/harness/bin/<name>/<version>/` dirs — never
//! prebaked into images, never host binaries mounted into containers
//! (ABI/shebang hazard). Spawning always goes through the `ProcessSpec`
//! chokepoint with per-lane config env.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HarnessName {
    Claude,
    Codex,
    Opencode,
    Pi,
    Gemini,
}

impl HarnessName {
    pub fn binary(&self) -> &'static str {
        match self {
            HarnessName::Claude => "claude",
            HarnessName::Codex => "codex",
            HarnessName::Opencode => "opencode",
            HarnessName::Pi => "pi",
            HarnessName::Gemini => "gemini",
        }
    }

    /// Known config dirs, checked in order. Presence marks a configured host install.
    pub fn config_dirs(&self) -> Vec<PathBuf> {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        let h = Path::new(&home);
        match self {
            HarnessName::Claude => vec![h.join(".claude")],
            HarnessName::Codex => vec![h.join(".codex")],
            HarnessName::Opencode => vec![h.join(".config").join("opencode"), h.join(".opencode")],
            HarnessName::Pi => vec![h.join(".pi")],
            HarnessName::Gemini => vec![h.join(".gemini")],
        }
    }

    pub fn all() -> &'static [HarnessName] {
        &[
            HarnessName::Claude,
            HarnessName::Codex,
            HarnessName::Opencode,
            HarnessName::Pi,
            HarnessName::Gemini,
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HarnessSource {
    /// Host binary + config reused as-is. Zero reconfiguration.
    Host { bin: PathBuf, configured: bool },
    /// Manager-installed into the versioned harness dir.
    Managed {
        name: HarnessName,
        version: String,
        dir: PathBuf,
    },
    /// Not installed anywhere; see `InstallMethod::instructions`.
    Missing,
}

/// Search explicit dirs (no shell) for an executable file.
/// `find_on_path` wraps this with the live PATH; prefer this pure form in
/// tests and hot loops (no global env mutation, no races).
pub fn find_on_paths(binary: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    dirs.iter()
        .map(|d| d.join(binary))
        .find(|c| is_executable(c))
}

/// Search PATH manually (no shell) for an executable file.
pub fn find_on_path(binary: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    find_on_paths(binary, &std::env::split_paths(&paths).collect::<Vec<_>>())
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.is_file()
        && p.metadata()
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(p: &Path) -> bool {
    p.is_file()
}

/// Probe one harness: host binary (+configured flag) or Missing.
pub fn probe(name: HarnessName) -> HarnessSource {
    match find_on_path(name.binary()) {
        Some(bin) => {
            let configured = name.config_dirs().iter().any(|d| d.exists());
            HarnessSource::Host { bin, configured }
        }
        None => HarnessSource::Missing,
    }
}

/// Probe every known harness.
pub fn probe_all() -> Vec<(HarnessName, HarnessSource)> {
    HarnessName::all().iter().map(|n| (*n, probe(*n))).collect()
}

/// Versioned install dir: `<base>/harness/bin/<name>/<version>/`.
/// Version dir (not image layer) absorbs CLI update cadence.
pub fn install_dir(base: &Path, name: HarnessName, version: &str) -> PathBuf {
    base.join("harness")
        .join("bin")
        .join(name.binary())
        .join(version)
}

/// Official install channel per harness. `Manual` = manager refuses to guess;
/// it reports instructions instead of fabricating a package name.
#[derive(Debug, Clone)]
pub enum InstallMethod {
    NpmPackage { package: String },
    Manual { instructions: String },
}

pub fn default_install_method(name: HarnessName) -> InstallMethod {
    match name {
        // Verified live: `npm view opencode-ai version` resolves.
        HarnessName::Opencode => InstallMethod::NpmPackage { package: "opencode-ai".into() },
        // Verified live: `@anthropic-ai/claude-code` resolves on npm.
        HarnessName::Claude => InstallMethod::NpmPackage { package: "@anthropic-ai/claude-code".into() },
        HarnessName::Codex | HarnessName::Pi | HarnessName::Gemini => InstallMethod::Manual {
            instructions: format!(
                "install {} from its official channel, or place the binary so `probe` finds it on PATH",
                name.binary()
            ),
        },
    }
}

/// Ensure a managed install exists. Returns the source; refuses Manual with
/// instructions (fail-closed, never fabricates packages).
pub async fn ensure_installed(
    base: &Path,
    name: HarnessName,
    version: &str,
) -> Result<HarnessSource, String> {
    // Host install always wins (detect-then-install).
    if let HarnessSource::Host { .. } = probe(name) {
        return Ok(probe(name));
    }
    let dir = install_dir(base, name, version);
    let bin = dir.join("node_modules").join(".bin").join(name.binary());
    if is_executable(&bin) {
        return Ok(HarnessSource::Managed {
            name,
            version: version.into(),
            dir,
        });
    }
    match default_install_method(name) {
        InstallMethod::Manual { instructions } => Err(instructions),
        InstallMethod::NpmPackage { package } => {
            use crate::mcp::process::{run_process, ProcessSpec};
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let spec = ProcessSpec::new(
                "npm",
                vec![
                    "install".into(),
                    "--prefix".into(),
                    dir.to_string_lossy().to_string(),
                    format!("{package}@{version}"),
                ],
                dir.clone(),
            );
            run_process(spec)
                .await
                .map_err(|e| format!("npm install failed: {e}"))?;
            if is_executable(&bin) {
                Ok(HarnessSource::Managed {
                    name,
                    version: version.into(),
                    dir,
                })
            } else {
                Err(format!(
                    "npm reported success but {} missing",
                    bin.display()
                ))
            }
        }
    }
}

/// Per-lane config env, all rooted under `profile_dir(base, lane)`.
/// `GEMINI_FORCE_FILE_STORAGE=true` is required for Antigravity-derived CLIs
/// (else tokens are lost on every restart); harmless elsewhere.
pub fn lane_env(base: &str, lane: &str) -> Vec<(String, String)> {
    let root = crate::roles::profile_dir(base, lane);
    vec![
        ("CLAUDE_CONFIG_DIR".into(), format!("{root}/claude")),
        ("CODEX_HOME".into(), format!("{root}/codex")),
        ("OPENCODE_CONFIG_DIR".into(), format!("{root}/opencode")),
        ("GEMINI_FORCE_FILE_STORAGE".into(), "true".into()),
        ("STUDIO_LANE".into(), lane.into()),
    ]
}

/// Spawn request: binary must be an absolute path previously returned by
/// `probe`/`ensure_installed` (no PATH hijack); execution goes through the
/// `ProcessSpec` chokepoint with lane env.
pub struct SpawnRequest {
    pub bin: PathBuf,
    pub allowed_roots: Vec<PathBuf>,
    pub args: Vec<String>,
    pub base: String,
    pub lane: String,
    pub task_id: String,
    pub role: String,
    pub cwd: PathBuf,
}

pub async fn spawn_harness(req: SpawnRequest) -> Result<crate::mcp::process::RunOutput, String> {
    use crate::mcp::process::{run_process_unchecked, ProcessSpec};
    if !req.bin.is_absolute() {
        return Err("harness binary must be an absolute path".into());
    }
    if !req.allowed_roots.iter().any(|r| req.bin.starts_with(r)) {
        return Err(format!(
            "harness binary outside allowed roots: {}",
            req.bin.display()
        ));
    }
    let mut spec = ProcessSpec::new(&req.bin.to_string_lossy(), req.args, req.cwd);
    let mut env: Vec<(String, String)> = lane_env(&req.base, &req.lane);
    env.push(("STUDIO_TASK_ID".into(), req.task_id));
    env.push(("STUDIO_ROLE".into(), req.role));
    spec.env = env;
    run_process_unchecked(spec).await.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn fake_bin(dir: &Path, name: &str) -> PathBuf {
        let p = dir.join(name);
        #[cfg(unix)]
        {
            std::fs::write(&p, "#!/bin/sh\nexit 0\n").unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        #[cfg(not(unix))]
        {
            std::fs::write(&p, "exit 0").unwrap();
        }
        p
    }

    #[test]
    fn probe_finds_host_binaries_without_shell() {
        let dir = tempfile::tempdir().unwrap();
        let bin = fake_bin(dir.path(), "opencode");
        assert_eq!(
            find_on_paths("opencode", &[dir.path().to_path_buf()]),
            Some(bin)
        );
        assert!(find_on_paths(
            "studio-definitely-not-a-binary-xyz",
            &[dir.path().to_path_buf()]
        )
        .is_none());
    }

    #[test]
    fn missing_binary_reports_missing() {
        // A name that cannot exist on PATH and has no install method issue here:
        // probe by binary name directly.
        assert!(find_on_path("studio-definitely-not-a-binary-xyz").is_none());
    }

    #[test]
    fn install_layout_is_versioned() {
        let root = Path::new("/tmp/studio-test-base");
        let d = install_dir(root, HarnessName::Opencode, "1.2.4");
        assert_eq!(d, root.join("harness/bin/opencode/1.2.4"));
    }

    #[test]
    fn manual_harness_refuses_with_instructions() {
        match default_install_method(HarnessName::Pi) {
            InstallMethod::Manual { instructions } => assert!(instructions.contains("pi")),
            _ => panic!("pi must not have a fabricated package name"),
        }
    }

    #[test]
    fn lane_env_is_lane_rooted() {
        let env: HashMap<_, _> = lane_env("/base", "lane-1").into_iter().collect();
        assert!(env["CLAUDE_CONFIG_DIR"].starts_with("/base/profiles/lane-1"));
        assert_eq!(env["GEMINI_FORCE_FILE_STORAGE"], "true");
    }

    #[tokio::test]
    async fn spawn_rejects_non_absolute_and_foreign_paths() {
        let base = || SpawnRequest {
            bin: PathBuf::new(),
            allowed_roots: vec![],
            args: vec![],
            base: "/base".into(),
            lane: "lane".into(),
            task_id: "t".into(),
            role: "coder".into(),
            cwd: PathBuf::from("."),
        };
        let mut bad_rel = base();
        bad_rel.bin = PathBuf::from("relative/bin");
        assert!(spawn_harness(bad_rel).await.is_err());
        let mut foreign = base();
        foreign.bin = PathBuf::from("/etc/passwd");
        foreign.allowed_roots = vec![PathBuf::from("/allowed")];
        assert!(spawn_harness(foreign).await.is_err());
    }

    #[tokio::test]
    async fn spawn_runs_allowed_binary_with_lane_env() {
        let dir = tempfile::tempdir().unwrap();
        // env(1) prints its environment: proves lane env propagation.
        let out = spawn_harness(SpawnRequest {
            bin: PathBuf::from("/usr/bin/env"),
            allowed_roots: vec![PathBuf::from("/usr/bin")],
            args: vec![],
            base: "/base".into(),
            lane: "lane-9".into(),
            task_id: "task-7".into(),
            role: "coder".into(),
            cwd: dir.path().to_path_buf(),
        })
        .await
        .unwrap();
        let text = String::from_utf8_lossy(&out.bytes);
        assert!(text.contains("STUDIO_TASK_ID=task-7"));
        assert!(text.contains("STUDIO_LANE=lane-9"));
    }
}
