//! Semgrep Guardian evidence for generation-time security gates (issue #15).
//! Runs `semgrep --config <stc-guardian.yaml> --json` through the `runProcess`
//! chokepoint and reduces the typed JSON contract to one number: findings.
//! Mirrors `verify/fallow.rs` (typed evidence, fail-closed unavailability).
//! TS/JS + Python lane surfaces; Rust keeps clippy + tree-sitter.
//! Observed schema (semgrep 1.166.0): root `{version, results[], errors[]}`,
//! each result `{check_id, path, start{line}, extra{message, severity}}`.
//! Exit 1 (findings) is a normal outcome, so this uses `run_process_unchecked`.

use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemgrepEvidence {
    /// False when the tool is missing, times out, or output is unparseable.
    /// Unavailable evidence never reads as green (fail-closed).
    pub available: bool,
    /// Number of findings (all severities; trial rules are ERROR-only).
    pub new_errors: u64,
    /// `pass` iff the tool ran and reported zero findings.
    pub verdict: String,
    pub detail: String,
}

impl SemgrepEvidence {
    pub fn unavailable(detail: &str) -> Self {
        Self {
            available: false,
            new_errors: 0,
            verdict: "unknown".into(),
            detail: detail.into(),
        }
    }

    /// Budget holds iff the tool ran and reported zero findings.
    pub fn budget_ok(&self) -> bool {
        self.available && self.verdict == "pass" && self.new_errors == 0
    }

    pub fn blocked(&self) -> bool {
        self.available && self.new_errors > 0
    }

    /// Parse one `semgrep --json` document. Leading non-JSON preamble
    /// (e.g. upgrade notices on merged stderr) is skipped to the first `{`.
    pub fn parse_json(doc: &str) -> Result<Self, String> {
        let start = doc
            .find('{')
            .ok_or_else(|| "no JSON object in semgrep output".to_string())?;
        let v: serde_json::Value =
            serde_json::from_str(&doc[start..]).map_err(|e| format!("semgrep JSON parse: {e}"))?;
        let results = v
            .get("results")
            .and_then(|r| r.as_array())
            .ok_or_else(|| "not a semgrep scan document (missing results)".to_string())?;
        let mut rules: Vec<String> = vec![];
        for r in results {
            if let Some(id) = r.get("check_id").and_then(|c| c.as_str()) {
                let short = id.rsplit('.').next().unwrap_or(id);
                if !rules.contains(&short.to_string()) {
                    rules.push(short.to_string());
                }
            }
        }
        rules.sort();
        let n = results.len() as u64;
        Ok(Self {
            available: true,
            new_errors: n,
            verdict: if n == 0 { "pass".into() } else { "fail".into() },
            detail: format!("findings={n} rules={}", rules.join(",")),
        })
    }

    /// Typed block reasons: `rule_id path:line` per finding (for receipts).
    pub fn block_reasons(doc: &str) -> Result<Vec<String>, String> {
        let start = doc
            .find('{')
            .ok_or_else(|| "no JSON object in semgrep output".to_string())?;
        let v: serde_json::Value =
            serde_json::from_str(&doc[start..]).map_err(|e| format!("semgrep JSON parse: {e}"))?;
        let results = v
            .get("results")
            .and_then(|r| r.as_array())
            .ok_or_else(|| "not a semgrep scan document (missing results)".to_string())?;
        Ok(results
            .iter()
            .map(|r| {
                let id = r
                    .get("check_id")
                    .and_then(|c| c.as_str())
                    .unwrap_or("unknown");
                let short = id.rsplit('.').next().unwrap_or(id);
                let path = r.get("path").and_then(|p| p.as_str()).unwrap_or("?");
                let line = r
                    .get("start")
                    .and_then(|s| s.get("line"))
                    .and_then(|l| l.as_u64())
                    .unwrap_or(0);
                format!("{short} {path}:{line}")
            })
            .collect())
    }
}

/// Resolve the Guardian ruleset: `$STC_SEMGREP_CONFIG` override, else the
/// repo-tracked trial ruleset (lane-local, no registry fetch).
pub fn default_config() -> String {
    std::env::var("STC_SEMGREP_CONFIG").unwrap_or_else(|_| "semgrep-rules/stc-guardian.yaml".into())
}

/// Scan `cwd` with the Guardian ruleset via the chokepoint. Semgrep cold
/// start is slower than fallow; the timeout reflects the slower tool.
pub async fn semgrep_scan(cwd: &std::path::Path) -> SemgrepEvidence {
    semgrep_scan_with("semgrep", &default_config(), cwd).await
}

pub async fn semgrep_scan_with(
    program: &str,
    config: &str,
    cwd: &std::path::Path,
) -> SemgrepEvidence {
    use crate::mcp::process::{run_process_unchecked, ProcessSpec};
    let mut spec = ProcessSpec::new(
        program,
        vec![
            "--config".into(),
            config.into(),
            "--json".into(),
            "--quiet".into(),
            ".".into(),
        ],
        cwd.to_path_buf(),
    );
    spec.timeout = Duration::from_secs(120);
    match run_process_unchecked(spec).await {
        Err(e) => SemgrepEvidence::unavailable(&format!("semgrep spawn: {e}")),
        Ok(out) => {
            let text = String::from_utf8_lossy(&out.bytes);
            match SemgrepEvidence::parse_json(&text) {
                Ok(ev) => ev,
                Err(e) => SemgrepEvidence::unavailable(&e.to_string()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_CLEAN: &str =
        r#"{"version":"1.0","results":[],"errors":[],"paths":{"scanned":["a.py"]}}"#;
    const SAMPLE_DIRTY: &str = r#"{"version":"1.0","results":[{"check_id":"stc-guardian.stc-hardcoded-secret","path":"seed.py","start":{"line":2},"extra":{"message":"m","severity":"ERROR"}},{"check_id":"stc-guardian.stc-dangerous-sink","path":"seed.py","start":{"line":5},"extra":{"message":"m","severity":"ERROR"}}],"errors":[]}"#;

    #[test]
    fn parses_real_shapes() {
        let ok = SemgrepEvidence::parse_json(SAMPLE_CLEAN).unwrap();
        assert!(ok.available && ok.budget_ok() && !ok.blocked());
        let bad = SemgrepEvidence::parse_json(SAMPLE_DIRTY).unwrap();
        assert!(bad.available && !bad.budget_ok() && bad.blocked());
        assert_eq!(bad.new_errors, 2);
        assert!(bad.detail.contains("stc-dangerous-sink"));
    }

    #[test]
    fn block_reasons_are_typed() {
        let reasons = SemgrepEvidence::block_reasons(SAMPLE_DIRTY).unwrap();
        assert_eq!(reasons.len(), 2);
        assert!(reasons[0].contains("stc-hardcoded-secret"));
        assert!(reasons[0].contains("seed.py:2"));
    }

    #[test]
    fn skips_stderr_preamble() {
        let doc = format!("A new version of Semgrep is available.\n{SAMPLE_CLEAN}");
        assert!(SemgrepEvidence::parse_json(&doc).unwrap().budget_ok());
    }

    #[test]
    fn rejects_non_scan_docs() {
        assert!(SemgrepEvidence::parse_json(r#"{"kind":"audit"}"#).is_err());
        assert!(SemgrepEvidence::parse_json("no json here").is_err());
    }

    #[test]
    fn default_config_env_override() {
        unsafe {
            std::env::set_var("STC_SEMGREP_CONFIG", "/tmp/x.yaml");
        }
        assert_eq!(default_config(), "/tmp/x.yaml");
        unsafe {
            std::env::remove_var("STC_SEMGREP_CONFIG");
        }
        assert_eq!(default_config(), "semgrep-rules/stc-guardian.yaml");
    }

    #[tokio::test]
    async fn missing_binary_is_unavailable_not_green() {
        let ev = semgrep_scan_with(
            "semgrep-definitely-not-installed-xyz",
            "cfg",
            std::path::Path::new("."),
        )
        .await;
        assert!(!ev.available && !ev.budget_ok() && !ev.blocked());
    }

    #[tokio::test]
    async fn live_trial_blocks_seeds_without_false_positives() {
        if std::process::Command::new("semgrep")
            .arg("--version")
            .output()
            .is_err()
        {
            return; // tool absent: unit paths above already cover the contract
        }
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let config = manifest.join("..").join("semgrep-rules/stc-guardian.yaml");
        if !config.exists() {
            return; // ruleset not co-located (e.g. packaged build): skip live path
        }
        let seed_dir = tempfile::tempdir().unwrap();
        std::fs::write(
            seed_dir.path().join("seed.py"),
            "import os\nAPI_KEY = \"sk-live-0123456789abcdef\"\nimport totally_made_up_pkg_xyz\nuser = input()\neval(user)\n",
        )
        .unwrap();
        let legit_dir = tempfile::tempdir().unwrap();
        std::fs::write(
            legit_dir.path().join("legit.py"),
            "import os\nimport sys\nfrom pathlib import Path\n\ndef legit(path):\n    return Path(path).read_text()\n",
        )
        .unwrap();
        let config_str = config.to_str().unwrap();
        let seeded = semgrep_scan_with("semgrep", config_str, seed_dir.path()).await;
        assert!(
            seeded.available,
            "semgrep ran but evidence unavailable: {}",
            seeded.detail
        );
        assert!(seeded.blocked(), "seeded vuln/dep/secret must block");
        assert!(
            seeded.detail.contains("stc-hardcoded-secret")
                && seeded.detail.contains("stc-hallucinated-dep")
                && seeded.detail.contains("stc-dangerous-sink"),
            "all three trial rules must fire: {}",
            seeded.detail
        );
        // False-positive measurement on legit work: must hold budget.
        let legit = semgrep_scan_with("semgrep", config_str, legit_dir.path()).await;
        assert!(
            legit.available && legit.budget_ok(),
            "legit work must not trip Guardian: {}",
            legit.detail
        );
    }
}
