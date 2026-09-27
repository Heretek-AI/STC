//! Fallow audit evidence for the DoD diagnostics budget (Phase 6 WS6 subset).
//! Runs `fallow audit --format json` through the `runProcess` chokepoint and
//! reduces the typed JSON contract to one number: NEW error-level findings.
//! TS/JS surfaces only — never gate Rust code with fallow (clippy + tree-sitter
//! own that). Observed schema (fallow 3.27.0): root `kind: "audit"`,
//! `verdict`, `attribution.{dead_code,complexity,duplication,styling}_introduced`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FallowEvidence {
    /// False when the tool is missing, times out, or output is unparseable.
    /// Unavailable evidence never reads as green (fail-closed).
    pub available: bool,
    /// Sum of `*_introduced` attribution counters (new findings only; inherited excluded).
    pub new_errors: u64,
    pub verdict: String,
    pub detail: String,
}

impl FallowEvidence {
    pub fn unavailable(detail: &str) -> Self {
        Self {
            available: false,
            new_errors: 0,
            verdict: "unknown".into(),
            detail: detail.into(),
        }
    }

    /// Budget holds iff the tool ran, verdict is pass, and zero new findings.
    pub fn budget_ok(&self) -> bool {
        self.available && self.verdict == "pass" && self.new_errors == 0
    }

    /// Parse one `fallow audit --format json` document. Leading non-JSON
    /// preamble (e.g. WARN lines on merged stderr) is skipped to the first `{`.
    pub fn parse_json(doc: &str) -> Result<Self, String> {
        let start = doc
            .find('{')
            .ok_or_else(|| "no JSON object in fallow output".to_string())?;
        let v: serde_json::Value =
            serde_json::from_str(&doc[start..]).map_err(|e| format!("fallow JSON parse: {e}"))?;
        if v.get("kind").and_then(|k| k.as_str()) != Some("audit") {
            return Err("not a fallow audit document".into());
        }
        let verdict = v
            .get("verdict")
            .and_then(|x| x.as_str())
            .unwrap_or("unknown")
            .to_string();
        let attr = v.get("attribution");
        let mut new_errors = 0u64;
        if let Some(a) = attr {
            for key in [
                "dead_code_introduced",
                "complexity_introduced",
                "duplication_introduced",
                "styling_introduced",
            ] {
                new_errors += a.get(key).and_then(|x| x.as_u64()).unwrap_or(0);
            }
        }
        Ok(Self {
            available: true,
            new_errors,
            verdict,
            detail: format!("introduced={new_errors}"),
        })
    }
}

/// Run `fallow audit --format json` in `cwd` via the chokepoint. Exit 1
/// (findings) is a normal outcome, so this uses `run_process_unchecked`.
pub async fn fallow_audit(cwd: &std::path::Path) -> FallowEvidence {
    fallow_audit_with("fallow", cwd).await
}

pub async fn fallow_audit_with(program: &str, cwd: &std::path::Path) -> FallowEvidence {
    use crate::mcp::process::{run_process_unchecked, ProcessSpec};
    let spec = ProcessSpec::new(
        program,
        vec![
            "audit".into(),
            "--format".into(),
            "json".into(),
            "--quiet".into(),
        ],
        cwd.to_path_buf(),
    );
    match run_process_unchecked(spec).await {
        Err(e) => FallowEvidence::unavailable(&format!("fallow spawn: {e}")),
        Ok(out) => {
            let text = String::from_utf8_lossy(&out.bytes);
            match FallowEvidence::parse_json(&text) {
                Ok(ev) => ev,
                Err(e) => FallowEvidence::unavailable(&e.to_string()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_PASS: &str = r#"{"kind":"audit","verdict":"pass","attribution":{"dead_code_introduced":0,"complexity_introduced":0,"duplication_introduced":0,"styling_introduced":0}}"#;
    const SAMPLE_FAIL: &str = r#"{"kind":"audit","verdict":"fail","attribution":{"dead_code_introduced":2,"complexity_introduced":1,"duplication_introduced":0,"styling_introduced":0}}"#;

    #[test]
    fn parses_real_shapes() {
        let ok = FallowEvidence::parse_json(SAMPLE_PASS).unwrap();
        assert!(ok.available && ok.budget_ok());
        let bad = FallowEvidence::parse_json(SAMPLE_FAIL).unwrap();
        assert!(bad.available && !bad.budget_ok() && bad.new_errors == 3);
    }

    #[test]
    fn skips_stderr_preamble() {
        let doc = format!(" WARN fallow: something\n{SAMPLE_PASS}");
        assert!(FallowEvidence::parse_json(&doc).unwrap().budget_ok());
    }

    #[test]
    fn rejects_non_audit_docs() {
        assert!(FallowEvidence::parse_json(r#"{"kind":"health"}"#).is_err());
        assert!(FallowEvidence::parse_json("no json here").is_err());
    }

    #[tokio::test]
    async fn missing_binary_is_unavailable_not_green() {
        let ev = fallow_audit_with(
            "fallow-definitely-not-installed-xyz",
            std::path::Path::new("."),
        )
        .await;
        assert!(!ev.available && !ev.budget_ok());
    }

    #[tokio::test]
    async fn live_fallow_passes_clean_tree() {
        if std::process::Command::new("fallow")
            .arg("--version")
            .output()
            .is_err()
        {
            return; // tool absent: unit paths above already cover the contract
        }
        let dir = tempfile::tempdir().unwrap();
        let run = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(dir.path())
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@t")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@t")
                .output()
                .expect("git")
        };
        run(&["init", "-q", "-b", "main", "."]);
        std::fs::write(
            dir.path().join("a.ts"),
            "export const x = 1;\nconsole.log(x);\n",
        )
        .unwrap();
        run(&["add", "-A"]);
        run(&["commit", "-qm", "init"]);
        let ev = fallow_audit_with("fallow", dir.path()).await;
        assert!(
            ev.available,
            "fallow ran but evidence unavailable: {}",
            ev.detail
        );
        assert!(ev.budget_ok(), "clean tree must hold budget: {}", ev.detail);
    }
}
