//! Scope claims + deterministic scope-hash extractor.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ScopeClaim {
    pub file: String,
    pub func: Option<String>,
    pub symbol: Option<String>,
}

impl ScopeClaim {
    pub fn file_only(file: &str) -> Self {
        Self {
            file: file.into(),
            func: None,
            symbol: None,
        }
    }
}

/// `scope_hash(file|fn|symbol)` — sha256 hex (truncated to 16 for broker keys).
pub fn scope_hash(c: &ScopeClaim) -> String {
    let mut h = Sha256::new();
    h.update(
        format!(
            "{}|{}|{}",
            c.file,
            c.func.as_deref().unwrap_or(""),
            c.symbol.as_deref().unwrap_or("")
        )
        .as_bytes(),
    );
    hex::encode(h.finalize())[..16].to_string()
}

/// Extract claimable scopes from source text. Heuristic fn/symbol detection for
/// Rust/Python/Go/TS; unknown grammars -> single file-granularity claim.
pub fn extract_scopes(file: &str, content: &str) -> Vec<ScopeClaim> {
    let lang = file.rsplit('.').next().unwrap_or("");
    let known = ["rs", "py", "go", "ts", "tsx", "js", "jsx"];
    if !known.contains(&lang) {
        return vec![ScopeClaim::file_only(file)];
    }
    let mut out = vec![];
    for line in content.lines() {
        let t = line.trim_start();
        let name = parse_fn_name(t, lang);
        if let Some(n) = name {
            out.push(ScopeClaim {
                file: file.into(),
                func: Some(n),
                symbol: None,
            });
        }
    }
    if out.is_empty() {
        out.push(ScopeClaim::file_only(file));
    }
    out
}

fn parse_fn_name(line: &str, lang: &str) -> Option<String> {
    let word_after = |s: &str, kw: &str| {
        s.strip_prefix(kw)?
            .trim_start()
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .next()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
    };
    match lang {
        "rs" => {
            let t = line.strip_prefix("pub ").unwrap_or(line);
            let t = t.strip_prefix("async ").unwrap_or(t);
            if t.starts_with("fn ") {
                word_after(t, "fn ")
            } else {
                None
            }
        }
        "py" => {
            if line.starts_with("def ") {
                word_after(line, "def ").map(|s| s.trim_end_matches(':').to_string())
            } else {
                None
            }
        }
        "go" => {
            if line.starts_with("func ") {
                let rest = line.strip_prefix("func ").unwrap().trim_start();
                // skip receiver: func (r R) Name(
                let rest = if rest.starts_with('(') {
                    rest.split(')').nth(1).unwrap_or("").trim_start()
                } else {
                    rest
                };
                rest.split(|c: char| !c.is_alphanumeric() && c != '_')
                    .next()
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
            } else {
                None
            }
        }
        "ts" | "tsx" | "js" | "jsx" => {
            if line.starts_with("function ") {
                word_after(line, "function ")
            } else if line.starts_with("export function ") {
                word_after(line, "export function ")
            } else {
                None
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_rust_fns() {
        let scopes = extract_scopes("a.rs", "pub fn alpha() {}\nfn beta() {}\nlet x = 1;\n");
        assert!(scopes.iter().any(|s| s.func.as_deref() == Some("alpha")));
        assert!(scopes.iter().any(|s| s.func.as_deref() == Some("beta")));
    }

    #[test]
    fn unknown_grammar_falls_back_to_file() {
        let scopes = extract_scopes("data.blob", "whatever ((( ");
        assert_eq!(scopes, vec![ScopeClaim::file_only("data.blob")]);
    }

    #[test]
    fn hash_stable() {
        let c = ScopeClaim {
            file: "a".into(),
            func: Some("f".into()),
            symbol: None,
        };
        assert_eq!(scope_hash(&c), scope_hash(&c));
    }
}
