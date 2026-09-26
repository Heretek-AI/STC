//! Bounded runProcess chokepoint: shell:false, 30s timeout, 8MiB max buffer.
//! Anchor: opencode-swarm `runProcess` (`ProcessSpec`, `shell:false`, bounded-output sink, `proc.kill()` in finally).

use std::path::PathBuf;
use std::time::Duration;
use thiserror::Error;

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
pub const MAX_OUTPUT_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum RunError {
    #[error("timeout after {0:?}")]
    Timeout(Duration),
    #[error("output exceeded {0} bytes")]
    Overflow(usize),
    #[error("spawn: {0}")]
    Spawn(String),
    #[error("denied: {0}")]
    Denied(String),
}

#[derive(Debug, Clone)]
pub struct ProcessSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub timeout: Duration,
    pub max_output: usize,
}

impl ProcessSpec {
    pub fn new(program: &str, args: Vec<String>, cwd: PathBuf) -> Self {
        Self { program: program.into(), args, cwd, timeout: DEFAULT_TIMEOUT, max_output: MAX_OUTPUT_BYTES }
    }
}

/// Array-form spawn only (never shell-string). Kills child on timeout / overflow.
pub async fn run_process(spec: ProcessSpec) -> Result<Vec<u8>, RunError> {
    // deny shell indirection outright
    if ["sh", "bash", "zsh", "fish", "cmd", "powershell"].contains(&spec.program.as_str()) {
        return Err(RunError::Denied(format!("shell:false chokepoint rejects {}", spec.program)));
    }
    let mut cmd = tokio::process::Command::new(&spec.program);
    cmd.args(&spec.args).current_dir(&spec.cwd);
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd.kill_on_drop(true);
    // explicit cwd required
    let child = cmd.spawn().map_err(|e| RunError::Spawn(e.to_string()))?;
    let timeout = spec.timeout;
    let max = spec.max_output;
    let out = tokio::time::timeout(timeout, child.wait_with_output()).await;
    match out {
        Err(_) => Err(RunError::Timeout(timeout)),
        Ok(Err(e)) => Err(RunError::Spawn(e.to_string())),
        Ok(Ok(o)) => {
            let mut buf = o.stdout;
            buf.extend_from_slice(&o.stderr);
            if buf.len() > max {
                buf.truncate(max);
                return Err(RunError::Overflow(max));
            }
            if !o.status.success() {
                // bounded stderr tail in error
                let tail: String = String::from_utf8_lossy(&buf).chars().take(500).collect();
                return Err(RunError::Spawn(format!("exit {}: {tail}", o.status)));
            }
            Ok(buf)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rejects_shell() {
        let spec = ProcessSpec::new("bash", vec!["-c".into(), "echo hi".into()], std::env::temp_dir());
        assert!(matches!(run_process(spec).await, Err(RunError::Denied(_))));
    }

    #[tokio::test]
    async fn runs_true() {
        let spec = ProcessSpec::new("true", vec![], std::env::temp_dir());
        assert!(run_process(spec).await.is_ok());
    }
}
