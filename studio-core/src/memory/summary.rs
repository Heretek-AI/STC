//! Anchored summaries + hard rung budgets (Phase 6 WS5).
//! The high-value memory artifact is a ≤250-token summary anchored to
//! `(path, symbol, commit_sha, plan_hash)`. Raw trajectories are never
//! injected. Token counts are `len/4` estimates (documented heuristic, not a
//! tokenizer) — budgets are guardrails, not billing.

/// Max tokens for one anchored summary.
pub const SUMMARY_TOKEN_BUDGET: usize = 250;
/// Rung-2 retrieval budget window.
pub const RETRIEVAL_TOKEN_MIN: usize = 300;
pub const RETRIEVAL_TOKEN_MAX: usize = 500;
/// L2 `MEMORY.md` hard bounds (Claude Code's rule).
pub const MEMORY_MD_MAX_LINES: usize = 200;
pub const MEMORY_MD_MAX_BYTES: usize = 25 * 1024;
/// L1 write-path body cap: longer bodies must arrive as anchored summaries.
pub const L1_BODY_MAX_BYTES: usize = 8 * 1024;

pub fn estimate_tokens(text: &str) -> usize {
    text.len().div_ceil(4)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnchoredSummary {
    pub path: String,
    pub symbol: String,
    pub commit_sha: String,
    pub plan_hash: String,
    pub body: String,
}

impl AnchoredSummary {
    pub fn new(
        path: &str,
        symbol: &str,
        commit_sha: &str,
        plan_hash: &str,
        body: &str,
    ) -> Result<Self, String> {
        if path.is_empty() || symbol.is_empty() || commit_sha.is_empty() || plan_hash.is_empty() {
            return Err("anchored summary requires path, symbol, commit_sha, plan_hash".into());
        }
        if estimate_tokens(body) > SUMMARY_TOKEN_BUDGET {
            return Err(format!(
                "summary exceeds {}-token budget ({} estimated)",
                SUMMARY_TOKEN_BUDGET,
                estimate_tokens(body)
            ));
        }
        Ok(Self {
            path: path.into(),
            symbol: symbol.into(),
            commit_sha: commit_sha.into(),
            plan_hash: plan_hash.into(),
            body: body.into(),
        })
    }

    pub fn doc_id(&self) -> String {
        format!(
            "{}#{}@{}",
            self.path,
            self.symbol,
            &self.commit_sha[..self.commit_sha.len().min(8)]
        )
    }
}

/// Greedy prefix fill: take whole texts while the running total fits.
pub fn fit_budget(texts: &[String], max_tokens: usize) -> Vec<String> {
    let mut out = vec![];
    let mut used = 0;
    for t in texts {
        let cost = estimate_tokens(t);
        if used + cost > max_tokens {
            break;
        }
        used += cost;
        out.push(t.clone());
    }
    out
}

/// Enforce L2 `MEMORY.md` bounds before save.
pub fn check_memory_md(body: &str) -> Result<(), String> {
    let lines = body.lines().count();
    if lines > MEMORY_MD_MAX_LINES {
        return Err(format!(
            "MEMORY.md exceeds {MEMORY_MD_MAX_LINES} lines ({lines})"
        ));
    }
    if body.len() > MEMORY_MD_MAX_BYTES {
        return Err(format!("MEMORY.md exceeds 25KB ({} bytes)", body.len()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_enforced() {
        assert!(AnchoredSummary::new("a.rs", "f", "abc123", "h", "short").is_ok());
        assert!(AnchoredSummary::new("a.rs", "f", "abc123", "h", &"x ".repeat(1000)).is_err());
        assert!(AnchoredSummary::new("", "f", "abc123", "h", "x").is_err());
    }

    #[test]
    fn budget_fill_is_prefix() {
        let texts = vec!["a".repeat(400), "b".repeat(400), "c".repeat(40000)];
        let fit = fit_budget(&texts, 250);
        assert_eq!(fit.len(), 2);
    }

    #[test]
    fn memory_md_bounds() {
        assert!(check_memory_md("# hello").is_ok());
        let big = (0..201)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(check_memory_md(&big).is_err());
    }
}
