//! Composable minimal ACI tool bundles + submit/review gate as YAML config.
//!
//! Clean-room re-derivation (SWE-agent/SWE-agent, MIT) behind citations
//! c022 (`96aeb863cbfaa768044527155f8555c9b401c6644e937b5b6b0bba5538b6eee4`)
//! and c023
//! (`91a7a214299997fa28b3988a911cef4640e99b69e87ba78fa1e03d5b0c979c51`).
//! No upstream code is copied: the re-derived rules are — a bundle is a
//! minimal named tool set (least privilege: compose small bundles, never
//! one mega-bundle); the YAML gate declares which bundles exist and
//! whether submission needs human review; submitting through a bundle
//! whose tools are unknown to the catalog is a typed refusal.
//!
//! V03-A2 catalog-bound handles (harden-03-gateway, full rework): bundle
//! submission NO LONGER takes a caller-supplied catalog slice. The catalog
//! authority is [`crate::mcp::registry::Registry`] via
//! [`crate::mcp::registry::Registry::submit_bundle`], which re-validates
//! every bundle tool against its held index on EVERY submit and demands a
//! fresh [`crate::mcp::registry::CatalogHandle`]. A caller `Vec<String>`
//! cannot be substituted (ghost-submit unrepresentable at compile time —
//! there is no parameter for it); stale generations refuse typed as
//! [`BundleError::StaleHandle`].
//!
//! Lockfile-neutral: hand-rolled YAML subset (mapping + `- ` lists at one
//! indent level). No new dependencies.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

/// Typed bundle failures. No `String` errors cross this boundary.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum BundleError {
    #[error("bundle {bundle} names unknown tool {tool}: fail-closed")]
    UnknownTool { bundle: String, tool: String },
    #[error("bundle {bundle} unknown: declared bundles are [{declared}]")]
    UnknownBundle { bundle: String, declared: String },
    #[error("submit through {bundle} needs review (gate requires approval)")]
    ReviewRequired { bundle: String },
    #[error(
        "stale catalog handle: expected generation {expected}, found {found} — reload invalidates"
    )]
    StaleHandle { expected: u64, found: u64 },
    #[error("bundle gate YAML parse: {detail}")]
    Parse { detail: String },
}

/// One minimal bundle: name + least-privilege tool list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bundle {
    pub name: String,
    pub tools: Vec<String>,
}

/// The submit/review gate: declared bundles + review flag.
///
/// Data-only: parsing + offline [`BundleGate::validate`] live here.
/// Submission lives on [`crate::mcp::registry::Registry::submit_bundle`]
/// (catalog-bound, no caller catalog param).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleGate {
    pub bundles: BTreeMap<String, Bundle>,
    pub review_required: bool,
}

impl BundleGate {
    /// Parse the minimal YAML subset:
    ///
    /// ```yaml
    /// review_required: true
    /// bundles:
    ///   fs-read:
    ///     - read
    ///     - code_search
    /// ```
    ///
    /// Two-space indent, `- ` list items, `key: value` mappings only.
    /// Anything else is a typed parse failure.
    pub fn parse_yaml(text: &str) -> Result<Self, BundleError> {
        let mut review_required = false;
        let mut bundles: BTreeMap<String, Bundle> = BTreeMap::new();
        let mut section: &str = "";
        let mut current: Option<String> = None;
        let fail = |detail: &str| BundleError::Parse {
            detail: detail.into(),
        };
        for raw in text.lines() {
            if raw.trim().is_empty() || raw.trim_start().starts_with('#') {
                continue;
            }
            let indent = raw.len() - raw.trim_start().len();
            let line = raw.trim();
            if indent == 0 {
                let (k, v) = line
                    .split_once(':')
                    .ok_or_else(|| fail("expected key: value"))?;
                match k.trim() {
                    "review_required" => {
                        review_required = match v.trim() {
                            "true" => true,
                            "false" => false,
                            _ => return Err(fail("review_required must be true/false")),
                        };
                        section = "";
                    }
                    "bundles" => {
                        if !v.trim().is_empty() {
                            return Err(fail("bundles takes nested mapping only"));
                        }
                        section = "bundles";
                    }
                    other => {
                        return Err(fail(&format!("unknown top-level key {other}")));
                    }
                }
                current = None;
            } else if section == "bundles" && indent == 2 {
                let name = line
                    .strip_suffix(':')
                    .ok_or_else(|| fail("expected bundle: "))?;
                bundles.insert(
                    name.trim().into(),
                    Bundle {
                        name: name.trim().into(),
                        tools: vec![],
                    },
                );
                current = Some(name.trim().into());
            } else if section == "bundles" && indent == 4 {
                let item = line
                    .strip_prefix("- ")
                    .ok_or_else(|| fail("expected - item"))?;
                let name = current.clone().ok_or_else(|| fail("item outside bundle"))?;
                bundles
                    .get_mut(&name)
                    .unwrap()
                    .tools
                    .push(item.trim().into());
            } else {
                return Err(fail("unexpected indent level"));
            }
        }
        if bundles.is_empty() {
            return Err(fail("no bundles declared"));
        }
        Ok(BundleGate {
            bundles,
            review_required,
        })
    }

    /// Validate every bundle tool against a catalog snapshot. Unknown →
    /// typed (never silently dropped from the bundle).
    ///
    /// LINT-ONLY, NEVER AUTH (F5 footgun closure): `catalog` is a
    /// caller-supplied snapshot — a forged vector containing a ghost tool
    /// PASSES this function by construction. It exists solely for offline
    /// config lint (e.g. `studio bundle lint`) and tests. The SUBMIT seam
    /// is [`crate::mcp::registry::Registry::submit_bundle`], which is
    /// catalog-BOUND (no caller `catalog` param, held index + fresh
    /// `CatalogHandle`, ghost refused typed on EVERY submit). Never gate
    /// authorization on this function.
    ///
    /// Test-only callers: this is exercised by unit tests with a static
    /// `catalog()` helper; production code must call `submit_bundle`.
    pub fn validate(&self, catalog: &[String]) -> Result<(), BundleError> {
        for b in self.bundles.values() {
            for t in &b.tools {
                if !catalog.contains(t) {
                    return Err(BundleError::UnknownTool {
                        bundle: b.name.clone(),
                        tool: t.clone(),
                    });
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const YAML: &str = "review_required: true\nbundles:\n  fs-read:\n    - read\n    - code_search\n  fs-write:\n    - write\n";

    fn manifest_three() -> String {
        json!([
            {"name": "read", "description": "read a file", "version": "1.0.0",
             "tags": ["fs"], "schema": {"type": "object"}},
            {"name": "code_search", "description": "search code", "version": "1.0.0",
             "tags": ["fs"], "schema": {"type": "object"}},
            {"name": "write", "description": "write a file", "version": "1.0.0",
             "tags": ["fs"], "schema": {"type": "object"}}
        ])
        .to_string()
    }

    fn catalog() -> Vec<String> {
        vec!["read".into(), "code_search".into(), "write".into()]
    }

    #[test]
    fn parses_validates_and_gates_review_bound() {
        use crate::mcp::registry::Registry;
        let gate = BundleGate::parse_yaml(YAML).unwrap();
        assert!(gate.review_required);
        assert_eq!(gate.bundles.len(), 2);
        gate.validate(&catalog()).unwrap();
        // Submit seam is registry-bound (no caller catalog param):
        // review gate holds without approval, passes with it.
        let reg = Registry::load_manifest(&manifest_three()).unwrap();
        let handle = reg.catalog_handle();
        assert!(matches!(
            reg.submit_bundle(&gate, "fs-read", false, &handle)
                .unwrap_err(),
            BundleError::ReviewRequired { .. }
        ));
        assert!(reg.submit_bundle(&gate, "fs-read", true, &handle).is_ok());
        assert!(matches!(
            reg.submit_bundle(&gate, "ghost", true, &handle)
                .unwrap_err(),
            BundleError::UnknownBundle { .. }
        ));
    }

    #[test]
    fn submit_rejects_ghost_tools_catalog_bound_no_caller_catalog() {
        // V03-A2: ghost tools refuse typed via the HELD catalog. There is
        // no `catalog: &[String]` parameter to forge — the only catalog is
        // the registry index, so a caller vector with the ghost tool cannot
        // authorize it.
        use crate::mcp::registry::Registry;
        let gate =
            BundleGate::parse_yaml("review_required: false\nbundles:\n  evil:\n    - ghost-tool\n")
                .unwrap();
        let reg = Registry::load_manifest(&manifest_three()).unwrap();
        let handle = reg.catalog_handle();
        assert!(matches!(
            reg.submit_bundle(&gate, "evil", false, &handle)
                .unwrap_err(),
            BundleError::UnknownTool { .. }
        ));
        // Stale handle refuses typed after reload.
        let mut reg2 = Registry::load_manifest(&manifest_three()).unwrap();
        let stale = reg2.catalog_handle();
        reg2.reload(&manifest_three()).unwrap();
        let gate2 =
            BundleGate::parse_yaml("review_required: false\nbundles:\n  b:\n    - read\n").unwrap();
        assert!(matches!(
            reg2.submit_bundle(&gate2, "b", false, &stale).unwrap_err(),
            BundleError::StaleHandle { .. }
        ));
    }

    #[test]
    fn validate_is_lint_only_forged_catalog_passes_but_submit_refuses() {
        // F5 (qa-b): `validate(&forged_catalog)` PASSES by construction
        // (caller snapshot, lint-only) — it must NEVER gate auth. The bound
        // submit seam refuses the same ghost bundle via the HELD index.
        use crate::mcp::registry::Registry;
        let gate =
            BundleGate::parse_yaml("review_required: false\nbundles:\n  evil:\n    - ghost-tool\n")
                .unwrap();
        let forged: Vec<String> = vec!["ghost-tool".into(), "read".into()];
        // Lint passes with the forged snapshot (documents the footgun).
        assert!(gate.validate(&forged).is_ok());
        // Bound submit refuses the same bundle via the held catalog.
        let reg = Registry::load_manifest(&manifest_three()).unwrap();
        let handle = reg.catalog_handle();
        assert!(matches!(
            reg.submit_bundle(&gate, "evil", false, &handle)
                .unwrap_err(),
            BundleError::UnknownTool { .. }
        ));
    }

    #[test]
    fn unknown_tools_and_bad_yaml_refuse_typed() {
        let gate = BundleGate::parse_yaml(YAML).unwrap();
        assert!(matches!(
            gate.validate(&["read".into()]).unwrap_err(),
            BundleError::UnknownTool { .. }
        ));
        assert!(matches!(
            BundleGate::parse_yaml("review_required: maybe\nbundles:\n  a:\n    - read\n")
                .unwrap_err(),
            BundleError::Parse { .. }
        ));
        assert!(matches!(
            BundleGate::parse_yaml("review_required: true\n").unwrap_err(),
            BundleError::Parse { .. }
        ));
    }
}
