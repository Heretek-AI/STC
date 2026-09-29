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
//! whose tools are all unknown to the catalog (or unlisted by the lens)
//! is a typed refusal.
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

    /// Validate every bundle tool against the catalog. Unknown → typed
    /// (never silently dropped from the bundle).
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

    /// Submit through a bundle: unknown bundles deny typed; every bundle
    /// tool is re-validated against `catalog` on EVERY submit (QA-R1: the
    /// old signature never consulted the catalog, so `submit("evil")`
    /// passed with ghost tools unless the caller remembered `validate`).
    /// Lens gating rides the invoke seam downstream, so a bundle that
    /// passes here is still lens-checked at call time. When the gate
    /// requires review and `reviewed` is false, submission is a typed
    /// refusal (the caller obtains approval through the P02 approval
    /// service, then retries with `reviewed = true`).
    pub fn submit(
        &self,
        bundle: &str,
        reviewed: bool,
        catalog: &[String],
    ) -> Result<(), BundleError> {
        let declared = self.bundles.keys().cloned().collect::<Vec<_>>().join(",");
        let b = self
            .bundles
            .get(bundle)
            .ok_or_else(|| BundleError::UnknownBundle {
                bundle: bundle.into(),
                declared,
            })?;
        for t in &b.tools {
            if !catalog.contains(t) {
                return Err(BundleError::UnknownTool {
                    bundle: b.name.clone(),
                    tool: t.clone(),
                });
            }
        }
        if self.review_required && !reviewed {
            return Err(BundleError::ReviewRequired {
                bundle: bundle.into(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const YAML: &str = "review_required: true\nbundles:\n  fs-read:\n    - read\n    - code_search\n  fs-write:\n    - write\n";

    fn catalog() -> Vec<String> {
        vec!["read".into(), "code_search".into(), "write".into()]
    }

    #[test]
    fn parses_validates_and_gates_review() {
        let gate = BundleGate::parse_yaml(YAML).unwrap();
        assert!(gate.review_required);
        assert_eq!(gate.bundles.len(), 2);
        gate.validate(&catalog()).unwrap();
        // Review gate holds without approval...
        assert!(matches!(
            gate.submit("fs-read", false, &catalog()).unwrap_err(),
            BundleError::ReviewRequired { .. }
        ));
        // ...and passes with it; unknown bundles deny typed.
        assert!(gate.submit("fs-read", true, &catalog()).is_ok());
        assert!(matches!(
            gate.submit("ghost", true, &catalog()).unwrap_err(),
            BundleError::UnknownBundle { .. }
        ));
    }

    #[test]
    fn submit_rejects_ghost_tools_without_prior_validate() {
        // QA-R1: review_required:false + bundles:{evil:[ghost-tool]} used to
        // submit Ok without ever consulting the catalog. Now every submit
        // re-validates against the catalog it is given.
        let gate =
            BundleGate::parse_yaml("review_required: false\nbundles:\n  evil:\n    - ghost-tool\n")
                .unwrap();
        assert!(matches!(
            gate.submit("evil", false, &catalog()).unwrap_err(),
            BundleError::UnknownTool { .. }
        ));
        // A catalog that actually declares the tool passes (no review gate).
        assert!(gate.submit("evil", false, &["ghost-tool".into()]).is_ok());
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
