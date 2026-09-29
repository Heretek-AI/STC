//! Tool-name wildcard grammar with hard bounds.
//!
//! Clean-room re-derivation (openchamber/openchamber, MIT) behind citation
//! c024 (`54a88c0c137eba522e05e199b0b5d75a65ee5793387a9e8c4d768fa3654b792b`).
//! No upstream code is copied: the re-derived rules are — a pattern is
//! either an exact tool name or a prefix with ONE trailing `*` (no leading
//! / embedded wildcards, no `?` classes); patterns and names obey the
//! registry length caps; one expansion matches at most 16 tools (hard
//! bound — overflow is a typed refusal, never a silent truncation).

use crate::mcp::registry::{validate_tool_name, MAX_EXPANSION, MAX_TOOL_NAME_LEN};
use thiserror::Error;

/// Typed wildcard failures. No `String` errors cross this boundary.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum MatchError {
    #[error("invalid wildcard pattern {pattern}: {reason}")]
    InvalidPattern {
        pattern: String,
        reason: &'static str,
    },
    #[error("pattern {pattern} matches {found} tools: hard bound is {MAX_EXPANSION}")]
    TooManyMatches { pattern: String, found: usize },
}

/// Validate a lens/catalog pattern WITHOUT a candidate tool: exact names
/// must satisfy the tool-name grammar; otherwise exactly one trailing `*`
/// over a grammar-valid (possibly empty) prefix. Used by
/// [`crate::mcp::lens::ToolLens::validate`] so a grammar-invalid disable
/// rule is a typed error, never a silently evaporated deny.
pub fn validate_pattern(pattern: &str) -> Result<(), MatchError> {
    if pattern.is_empty() {
        return Err(MatchError::InvalidPattern {
            pattern: pattern.into(),
            reason: "empty pattern",
        });
    }
    if pattern.len() > MAX_TOOL_NAME_LEN {
        return Err(MatchError::InvalidPattern {
            pattern: pattern.into(),
            reason: "pattern length out of bounds 1..=64",
        });
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        if prefix.contains('*') {
            return Err(MatchError::InvalidPattern {
                pattern: pattern.into(),
                reason: "only a single trailing * is allowed",
            });
        }
        if !prefix.is_empty() {
            validate_tool_name(prefix).map_err(|_| MatchError::InvalidPattern {
                pattern: pattern.into(),
                reason: "prefix fails tool-name grammar",
            })?;
        }
        Ok(())
    } else {
        if pattern.contains('*') {
            return Err(MatchError::InvalidPattern {
                pattern: pattern.into(),
                reason: "only a single trailing * is allowed",
            });
        }
        validate_tool_name(pattern).map_err(|_| MatchError::InvalidPattern {
            pattern: pattern.into(),
            reason: "pattern fails tool-name grammar",
        })?;
        Ok(())
    }
}
/// True when `pattern` (exact name or one trailing `*`) matches `tool`.
/// The pattern side is checked by [`validate_pattern`]; the candidate must
/// satisfy the canonical tool-name grammar.
pub fn matches(pattern: &str, tool: &str) -> Result<bool, MatchError> {
    validate_tool_name(tool).map_err(|_| MatchError::InvalidPattern {
        pattern: pattern.into(),
        reason: "candidate tool name fails grammar",
    })?;
    validate_pattern(pattern)?;
    if let Some(prefix) = pattern.strip_suffix('*') {
        Ok(tool.starts_with(prefix))
    } else {
        Ok(pattern == tool)
    }
}

/// Expand a pattern over a catalog. More than [`MAX_EXPANSION`] hits is a
/// typed refusal (never silent truncation — the caller narrows the prefix).
pub fn expand(pattern: &str, catalog: &[String]) -> Result<Vec<String>, MatchError> {
    let mut out = Vec::new();
    for tool in catalog {
        if matches(pattern, tool)? {
            out.push(tool.clone());
        }
    }
    if out.len() > MAX_EXPANSION {
        return Err(MatchError::TooManyMatches {
            pattern: pattern.into(),
            found: out.len(),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> Vec<String> {
        vec![
            "read".into(),
            "runProcess".into(),
            "retrieve_docs".into(),
            "write".into(),
        ]
    }

    #[test]
    fn exact_and_trailing_star_match() {
        assert!(matches("read", "read").unwrap());
        assert!(!matches("read", "write").unwrap());
        assert!(matches("r*", "read").unwrap());
        assert!(matches("r*", "runProcess").unwrap());
        assert!(matches("r*", "retrieve_docs").unwrap());
        assert!(!matches("r*", "write").unwrap());
        assert!(matches("*", "read").unwrap());
    }

    #[test]
    fn bad_grammars_fail_typed() {
        assert!(matches("*read", "read").is_err());
        assert!(matches("r*o", "read").is_err());
        assert!(matches("r**", "read").is_err());
        assert!(matches("", "read").is_err());
        assert!(matches("has space", "read").is_err());
        assert!(matches("read", "has space").is_err());
    }

    #[test]
    fn expansion_is_hard_bounded() {
        let got = expand("r*", &catalog()).unwrap();
        assert_eq!(got.len(), 3);
        let big: Vec<String> = (0..20).map(|i| format!("tool{i:02}")).collect();
        assert!(matches!(
            expand("tool*", &big).unwrap_err(),
            MatchError::TooManyMatches { .. }
        ));
        assert!(expand("zzz*", &catalog()).unwrap().is_empty());
    }
}
