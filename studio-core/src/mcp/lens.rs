//! Tool-lens primitives: enable/disable by keys+tags, search-tools
//! transform, version-range filters for composed servers.
//!
//! Clean-room re-derivation (jlowin/fastmcp, Apache-2.0) behind citations
//! c017 (`329fd541c0ad35de30fb80a5f7cf8be90523213aa2647678c8117bf7f562784`),
//! c018 (`27d825c8e4e3506303caabf851ef575c32132005bd7ad8cd8ac943fd67d3a641`),
//! c019 (`f4d13cb0bcadbb3dd16b0ead778cfe229ac05bb15ff2ebe028d03162dbcd0640`).
//! No upstream code is copied: the re-derived rules are — a disabled key
//! (exact name or [`crate::mcp::toolmatch`] wildcard) or a disabled tag
//! makes a tool UNLISTED and UNCALLABLE (both, never one without the
//! other); `search_tools` is a pure transform over the enabled set with a
//! hard result bound; version filters admit only tools whose version
//! satisfies the requirement.

use crate::mcp::registry::{IndexEntry, MAX_EXPANSION};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use thiserror::Error;

/// Typed lens failures. No `String` errors cross this boundary.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum LensError {
    #[error("lens pattern {pattern} invalid: {reason}")]
    BadPattern { pattern: String, reason: String },
    #[error("version requirement {req} invalid: expected [=|^|>=]<major>[.<minor>[.<patch>]]")]
    BadVersionReq { req: String },
}

/// Lens: disabled keys (exact names or trailing-`*` wildcards), disabled
/// tags, and one optional version requirement shared by the composed set.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolLens {
    #[serde(default)]
    pub disabled_keys: Vec<String>,
    #[serde(default)]
    pub disabled_tags: Vec<String>,
    #[serde(default)]
    pub version_req: Option<String>,
}

impl ToolLens {
    fn key_disabled(&self, name: &str) -> bool {
        self.disabled_keys
            .iter()
            .any(|pat| crate::mcp::toolmatch::matches(pat, name).unwrap_or(false))
    }

    /// Validate the lens itself: every disable key must satisfy the
    /// wildcard grammar ([`crate::mcp::toolmatch::validate_pattern`]) and
    /// the version requirement must parse. A grammar-invalid disable rule
    /// is a typed error here — it NEVER evaporates into `is_enabled ==
    /// true` (QA-R1: `*read` used to fail open through `unwrap_or(false)`).
    pub fn validate(&self) -> Result<(), LensError> {
        for pat in &self.disabled_keys {
            crate::mcp::toolmatch::validate_pattern(pat).map_err(|e| LensError::BadPattern {
                pattern: pat.clone(),
                reason: e.to_string(),
            })?;
        }
        if let Some(req) = &self.version_req {
            // Exercise the requirement grammar against a dummy version:
            // parse failures surface as typed errors without admitting
            // anything.
            version_satisfies("0.0.0", req)?;
        }
        Ok(())
    }

    /// Disabled = unlisted AND uncallable. One predicate answers both so
    /// the two can never drift apart. A lens that fails [`validate`] lists
    /// NOTHING (fail-closed: a misconfigured lens hides the catalog rather
    /// than exposing it; the typed reason lives in [`check_callable`]).
    pub fn is_enabled(&self, entry: &IndexEntry) -> bool {
        if self.validate().is_err() {
            return false;
        }
        if self.key_disabled(&entry.name) {
            return false;
        }
        let tags: HashSet<&str> = entry.tags.iter().map(String::as_str).collect();
        if self.disabled_tags.iter().any(|t| tags.contains(t.as_str())) {
            return false;
        }
        match &self.version_req {
            None => true,
            Some(req) => version_satisfies(&entry.version, req).unwrap_or(false),
        }
    }

    /// Filter rows to the enabled set (listing side of the contract).
    pub fn apply(&self, rows: Vec<IndexEntry>) -> Vec<IndexEntry> {
        rows.into_iter().filter(|e| self.is_enabled(e)).collect()
    }

    /// Callable check (call side of the contract): same predicate as
    /// listing, PLUS lens validation first. An invalid disable pattern is
    /// a typed refusal (never a silent pass), and returns
    /// [`crate::mcp::registry::RegistryError::LensDisabled`]
    /// so the seam error type stays uniform.
    pub fn check_callable(
        &self,
        entry: &IndexEntry,
    ) -> Result<(), crate::mcp::registry::RegistryError> {
        if let Err(e) = self.validate() {
            return Err(crate::mcp::registry::RegistryError::Denied {
                reason: format!("invalid lens pattern: {e}"),
            });
        }
        if self.is_enabled(entry) {
            Ok(())
        } else {
            Err(crate::mcp::registry::RegistryError::LensDisabled {
                tool: entry.name.clone(),
            })
        }
    }

    /// Search-tools transform: case-insensitive substring over
    /// name + description + tags, over the ENABLED set only, hard-bounded
    /// to [`MAX_EXPANSION`] rows (overflow beyond the bound is truncated
    /// deterministically by name order — the bound, not the truncation, is
    /// the contract; callers narrow the query for full recall).
    pub fn search_tools(&self, rows: &[IndexEntry], query: &str) -> Vec<IndexEntry> {
        let q = query.to_lowercase();
        let mut hits: Vec<IndexEntry> = rows
            .iter()
            .filter(|e| self.is_enabled(e))
            .filter(|e| {
                e.name.to_lowercase().contains(&q)
                    || e.description.to_lowercase().contains(&q)
                    || e.tags.iter().any(|t| t.to_lowercase().contains(&q))
            })
            .cloned()
            .collect();
        hits.sort_by(|a, b| a.name.cmp(&b.name));
        hits.truncate(MAX_EXPANSION);
        hits
    }
}

/// Minimal version requirement grammar: `=1.2.3` (exact), `^1` / `^1.2`
/// (compatible-major), `>=1.2.0` (floor). Missing version on the tool
/// satisfies NOTHING when a requirement is set (fail-closed: unversioned
/// tools do not sneak through a version gate).
pub fn version_satisfies(version: &str, req: &str) -> Result<bool, LensError> {
    fn parse(v: &str) -> Option<(u64, u64, u64)> {
        let mut it = v.split('.');
        let maj = it.next()?.parse().ok()?;
        let min = it.next().map(|s| s.parse().unwrap_or(0)).unwrap_or(0);
        let pat = it.next().map(|s| s.parse().unwrap_or(0)).unwrap_or(0);
        Some((maj, min, pat))
    }
    if version.is_empty() {
        return Ok(false);
    }
    let v = parse(version).ok_or_else(|| LensError::BadVersionReq { req: req.into() })?;
    if let Some(exact) = req.strip_prefix('=') {
        let r = parse(exact).ok_or_else(|| LensError::BadVersionReq { req: req.into() })?;
        return Ok(v == r);
    }
    if let Some(caret) = req.strip_prefix('^') {
        let r = parse(caret).ok_or_else(|| LensError::BadVersionReq { req: req.into() })?;
        return Ok(v.0 == r.0 && v >= r);
    }
    if let Some(floor) = req.strip_prefix(">=") {
        let r = parse(floor).ok_or_else(|| LensError::BadVersionReq { req: req.into() })?;
        return Ok(v >= r);
    }
    Err(LensError::BadVersionReq { req: req.into() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, tags: &[&str], version: &str) -> IndexEntry {
        IndexEntry {
            name: name.into(),
            description: format!("does {name}"),
            version: version.into(),
            tags: tags.iter().map(|s| s.to_string()).collect(),
            on_demand: false,
        }
    }

    #[test]
    fn disabled_means_unlisted_and_uncallable() {
        let lens = ToolLens {
            disabled_keys: vec!["write".into()],
            disabled_tags: vec!["exec".into()],
            version_req: None,
        };
        let rows = vec![
            entry("read", &["fs"], "1.0.0"),
            entry("write", &["fs"], "1.0.0"),
            entry("runProcess", &["exec"], "1.0.0"),
        ];
        let listed = lens.apply(rows.clone());
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "read");
        assert!(lens.check_callable(&rows[0]).is_ok());
        assert!(lens.check_callable(&rows[1]).is_err());
        assert!(lens.check_callable(&rows[2]).is_err());
        // Wildcard keys disable whole families.
        let lens = ToolLens {
            disabled_keys: vec!["run*".into()],
            ..Default::default()
        };
        assert!(!lens.is_enabled(&rows[2]));
        assert!(lens.is_enabled(&rows[0]));
    }

    #[test]
    fn search_is_transform_over_enabled_only() {
        let lens = ToolLens {
            disabled_keys: vec!["write".into()],
            ..Default::default()
        };
        let rows = vec![
            entry("read", &["fs"], "1.0.0"),
            entry("write", &["fs"], "1.0.0"),
            entry("retrieve_docs", &["docs"], "1.0.0"),
        ];
        let hits = lens.search_tools(&rows, "re");
        assert!(hits.iter().all(|e| e.name != "write"));
        assert!(hits.iter().any(|e| e.name == "read"));
        assert!(hits.iter().any(|e| e.name == "retrieve_docs"));
    }

    #[test]
    fn invalid_disable_pattern_fails_closed_typed() {
        // QA-R1: `*read` (leading star) used to evaporate through
        // `unwrap_or(false)` into `is_enabled == true`. Now the lens is
        // invalid: nothing lists, every call refuses typed.
        let lens = ToolLens {
            disabled_keys: vec!["*read".into()],
            ..Default::default()
        };
        let err = lens.validate().unwrap_err();
        assert!(matches!(err, LensError::BadPattern { .. }));
        let row = entry("read", &["fs"], "1.0.0");
        assert!(!lens.is_enabled(&row));
        assert!(matches!(
            lens.check_callable(&row).unwrap_err(),
            crate::mcp::registry::RegistryError::Denied { .. }
        ));
        assert!(lens.apply(vec![row]).is_empty());
        // Bad version requirements are typed too.
        let lens = ToolLens {
            version_req: Some("soon".into()),
            ..Default::default()
        };
        assert!(matches!(
            lens.validate().unwrap_err(),
            LensError::BadVersionReq { .. }
        ));
    }
    #[test]
    fn version_filters_hold_unversioned_closed() {
        assert!(version_satisfies("1.2.3", "=1.2.3").unwrap());
        assert!(!version_satisfies("1.2.4", "=1.2.3").unwrap());
        assert!(version_satisfies("1.4.0", "^1.2").unwrap());
        assert!(!version_satisfies("2.0.0", "^1.2").unwrap());
        assert!(version_satisfies("1.3.0", ">=1.2.0").unwrap());
        assert!(!version_satisfies("1.1.9", ">=1.2.0").unwrap());
        assert!(!version_satisfies("", ">=1.0.0").unwrap());
        assert!(version_satisfies("9.9.9", ">=1.0.0").unwrap());
        assert!(matches!(
            version_satisfies("1.0.0", "soon").unwrap_err(),
            LensError::BadVersionReq { .. }
        ));
    }
}
