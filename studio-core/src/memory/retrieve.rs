//! L3 retrieval backends (Phase 6 WS5).
//! `RetrieveBackend` is the seam: the FTS backend is authoritative;
//! `EngramBackend` is the gated eval spike (kill: recall misses or >1 day ops).
//! Engram wire format is pinned to engram 1.20.0 human output (no JSON mode
//! exists); the parser refuses anything it cannot shape-match (fail-closed).

use std::path::PathBuf;

pub trait RetrieveBackend {
    // Internal seam (single crate, no auto-trait bounds needed): async avoids
    // nesting a Tokio runtime inside daemon async contexts (panics).
    #[allow(async_fn_in_trait)]
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<(String, String)>, String>;
    fn name(&self) -> &'static str;
}

pub struct FtsBackend<'a> {
    store: &'a crate::memory::MemoryStore,
}

impl<'a> FtsBackend<'a> {
    pub fn new(store: &'a crate::memory::MemoryStore) -> Self {
        Self { store }
    }
}

impl RetrieveBackend for FtsBackend<'_> {
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<(String, String)>, String> {
        self.store
            .search_fts(query, limit)
            .map_err(|e| e.to_string())
    }

    fn name(&self) -> &'static str {
        "fts5"
    }
}

/// Engram eval backend. Pinned wire format (engram 1.20.0):
/// `[1] #<obs> (<kind>) — <title>` then indented body lines.
/// Anything else (including the "No memories found" line) yields zero rows,
/// never a fabricated hit.
pub struct EngramBackend {
    bin: PathBuf,
    project: String,
}

impl EngramBackend {
    /// Probe: `Some` iff an `engram` binary resolves on PATH.
    pub fn probe() -> Option<Self> {
        Self::probe_with_project("stc")
    }

    pub fn probe_with_project(project: &str) -> Option<Self> {
        let bin = crate::harness::find_on_path("engram")?;
        Some(Self {
            bin,
            project: project.into(),
        })
    }

    pub fn parse_output(text: &str) -> Vec<(String, String)> {
        let mut out = vec![];
        let mut cur_id: Option<String> = None;
        let mut cur_body: Vec<String> = vec![];
        let flush =
            |id: &mut Option<String>, body: &mut Vec<String>, out: &mut Vec<(String, String)>| {
                if let Some(i) = id.take() {
                    if !body.is_empty() {
                        out.push((i, body.join("\n")));
                    }
                    body.clear();
                }
            };
        for line in text.lines() {
            let t = line.trim();
            if t.is_empty() {
                continue;
            }
            if t.starts_with("No memories found") {
                return vec![];
            }
            // Entry header: `[1] #3 (manual) — title`
            if let Some(rest) = t.strip_prefix('[').and_then(|s| s.split_once("] ")) {
                if rest.0.chars().all(|c| c.is_ascii_digit()) {
                    flush(&mut cur_id, &mut cur_body, &mut out);
                    let id = rest
                        .1
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .trim_start_matches('#')
                        .to_string();
                    if !id.is_empty() {
                        cur_id = Some(id);
                    }
                    continue;
                }
            }
            if cur_id.is_some() {
                // Skip the metadata trailer line (`2026-.. | project: .. | scope: ..`).
                if !(t.contains("| project:") && t.contains("| scope:")) {
                    cur_body.push(t.to_string());
                }
            }
        }
        flush(&mut cur_id, &mut cur_body, &mut out);
        out
    }
}

impl RetrieveBackend for EngramBackend {
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<(String, String)>, String> {
        use crate::mcp::process::{run_process_unchecked, ProcessSpec};
        let spec = ProcessSpec::new(
            &self.bin.to_string_lossy(),
            vec![
                "search".into(),
                query.into(),
                "--project".into(),
                self.project.clone(),
                "--limit".into(),
                limit.to_string(),
            ],
            std::env::temp_dir(),
        );
        let out = run_process_unchecked(spec)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Self::parse_output(&String::from_utf8_lossy(&out.bytes)))
    }

    fn name(&self) -> &'static str {
        "engram-cli-1.20.0"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "Found 2 memories:\n\n[1] #3 (manual) — auth checkout\n    SecretBroker checkout materializes 0600 token files\n    2026-09-26 22:02:32 | project: stc-spike | scope: project\n\n[2] #1 (manual) — other\n    Something else here\n    2026-09-26 22:02:26 | project: stc-spike | scope: project\n";

    #[test]
    fn parses_pinned_format() {
        let rows = EngramBackend::parse_output(SAMPLE);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, "3");
        assert!(rows[0].1.contains("SecretBroker"));
        // Metadata trailer must not leak into bodies.
        assert!(!rows[0].1.contains("| scope:"));
    }

    #[test]
    fn empty_result_is_zero_rows_not_fabrication() {
        assert!(EngramBackend::parse_output("No memories found for: \"xyz\"\n").is_empty());
        assert!(EngramBackend::parse_output("garbage\nmore garbage\n").is_empty());
    }
}
