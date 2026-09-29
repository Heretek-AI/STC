//! Tool approval tiers + object policies (the write-intent contract).
//!
//! Clean-room re-derivation (oh-my-pi, MIT) behind citation c014
//! (`f6e4638194f716ab52c1e23fd3dcfd8cbc74fe4ec5e8263fe8449ea7d78ea59f`).
//! No upstream code is copied: the re-derived rules are — every tool sits
//! in exactly one tier (read / write / exec); object policies
//! (allow / deny / prompt, with override + reason) refine per-object
//! decisions; UNKNOWN tools default to exec (the most restrictive tier,
//! never read); MCP-bridged tools (`mcp__*`) declare write MINIMUM.
//!
//! ## Tier/gate interaction with P02 (ACP gate c010 + policy resolver c009)
//!
//! The tier is a LABEL; enforcement stays in phase-02 mechanisms:
//!
//! - Tier → ACP class: Read → `ToolClass::Read`, Write → `ToolClass::Write`,
//!   Exec → `ToolClass::Execute`. `crate::acp::decide` remains the call-time
//!   enforcement point (unknown tools fail closed there too, so an unknown
//!   tool is denied twice: registry `UnknownTool` at the seam, ACP
//!   `UnknownTool` at the gate).
//! - Tier → policy floor: Exec-tier calls always resolve through
//!   `RuntimePolicy` at `Permissive` or above AND an explicit approval
//!   (mirroring `decide`'s "execution never rides a bare allow"); Write
//!   needs `Standard`+ plus a fencing lease; Read rides the ambient grant.
//! - Object policies refine WITHIN the tier, never below it: a per-object
//!   `allow` cannot downgrade an Exec tool to unapproved execution (the
//!   override carries a reason precisely so the audit trail shows who
//!   claimed the downgrade and why — the gateway still routes through
//!   `decide`, which escalates regardless).
//!
//! [`tier_of`] + [`required_option`] encode this mapping so callers cannot
//! invent a weaker one.

use crate::acp::{PermissionOption, ToolClass};
use crate::policy::{PolicyLevel, RuntimePolicy};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Approval tier: exactly one per tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalTier {
    Read,
    Write,
    Exec,
}

impl ApprovalTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            ApprovalTier::Read => "read",
            ApprovalTier::Write => "write",
            ApprovalTier::Exec => "exec",
        }
    }
}

/// Typed tier failures. No `String` errors cross this boundary.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum TierError {
    #[error("tier escalation refused: {tier} needs {floor}, grant is {grant}")]
    Escalation {
        tier: String,
        floor: String,
        grant: String,
    },
    #[error("object {object} denied for {tool}: {reason}")]
    ObjectDenied {
        tool: String,
        object: String,
        reason: String,
    },
    #[error("object {object} needs prompt for {tool}: {reason}")]
    ObjectPrompt {
        tool: String,
        object: String,
        reason: String,
    },
}

/// Classify a tool. Closed rules, fail-closed order:
///
/// 1. `mcp__*` bridged tools declare write MINIMUM (MCP=write), even when
///    the suffix names a read-class tool.
/// 2. Known tools map through the ACP class table (single source of truth
///    for the class; unknown there → rule 3).
/// 3. Unknown/malformed tools default to Exec (unknown→exec).
pub fn tier_of(tool: &str) -> ApprovalTier {
    if tool.starts_with("mcp__") {
        let suffix = tool.trim_start_matches("mcp__");
        match crate::acp::tool_class(suffix) {
            Ok(ToolClass::Execute) => ApprovalTier::Exec,
            // MCP=write floor: read-class suffix still lands on Write.
            _ => ApprovalTier::Write,
        }
    } else {
        match crate::acp::tool_class(tool) {
            Ok(ToolClass::Read) => ApprovalTier::Read,
            Ok(ToolClass::Write) => ApprovalTier::Write,
            Ok(ToolClass::Execute) | Err(_) => ApprovalTier::Exec,
        }
    }
}

/// The ACP permission option the tier demands at minimum. Enforcement is
/// `crate::acp::decide`; this is the label side of the contract.
pub fn required_option(tier: ApprovalTier) -> PermissionOption {
    match tier {
        ApprovalTier::Read => PermissionOption::AllowOnce,
        ApprovalTier::Write => PermissionOption::AllowSession,
        ApprovalTier::Exec => PermissionOption::RequireApproval,
    }
}

/// Policy floor per tier, resolved through the P02 monotonic resolver:
/// the caller's `RuntimePolicy` must be AT or ABOVE the floor (reduce-only
/// means a lower grant is a typed refusal, never an upgrade).
pub fn tier_floor(tier: ApprovalTier) -> PolicyLevel {
    match tier {
        ApprovalTier::Read => PolicyLevel::Locked,
        ApprovalTier::Write => PolicyLevel::Standard,
        ApprovalTier::Exec => PolicyLevel::Permissive,
    }
}

/// Check the caller's runtime policy against the tier floor. Uses the P02
/// `RuntimePolicy::resolve` semantics: requesting above the grant refuses
/// typed (reduce-only).
pub fn check_policy_floor(policy: &RuntimePolicy, tier: ApprovalTier) -> Result<(), TierError> {
    let floor = tier_floor(tier);
    if (policy.level as u8) < (floor as u8) {
        return Err(TierError::Escalation {
            tier: tier.as_str().into(),
            floor: floor.as_str().into(),
            grant: policy.level.as_str().into(),
        });
    }
    Ok(())
}

/// Per-object policy effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObjectEffect {
    Allow,
    Deny,
    Prompt,
}

/// One object rule: substring pattern + effect + optional override reason.
/// An `Allow` override MUST carry a reason (who approved the exception and
/// why); a missing reason on override is itself a typed prompt-escalation,
/// never a silent pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectRule {
    pub pattern: String,
    pub effect: ObjectEffect,
    pub override_reason: Option<String>,
}

/// Evaluate object policies for one (tool, object) pair. First matching
/// rule wins; no rule → tier default (Read allows, Write/Exec prompt).
/// Effects never weaken the tier: an `Allow` on an Exec tool still
/// requires the ACP approval path (returned as `ObjectPrompt` with the
/// override reason attached, so the audit trail is explicit).
pub fn evaluate_object(tool: &str, object: &str, rules: &[ObjectRule]) -> Result<(), TierError> {
    let tier = tier_of(tool);
    for r in rules {
        if object.contains(&r.pattern) {
            match r.effect {
                ObjectEffect::Deny => {
                    return Err(TierError::ObjectDenied {
                        tool: tool.into(),
                        object: object.into(),
                        reason: r
                            .override_reason
                            .clone()
                            .unwrap_or_else(|| "object policy deny".into()),
                    });
                }
                ObjectEffect::Prompt => {
                    return Err(TierError::ObjectPrompt {
                        tool: tool.into(),
                        object: object.into(),
                        reason: r
                            .override_reason
                            .clone()
                            .unwrap_or_else(|| "object policy prompt".into()),
                    });
                }
                ObjectEffect::Allow => {
                    let reason = match &r.override_reason {
                        Some(reason) => reason.clone(),
                        None => {
                            return Err(TierError::ObjectPrompt {
                                tool: tool.into(),
                                object: object.into(),
                                reason: "allow override without reason escalates to prompt".into(),
                            });
                        }
                    };
                    // Allow refines within the tier but never below it:
                    // exec tools still need approval; record the override.
                    if tier == ApprovalTier::Exec {
                        return Err(TierError::ObjectPrompt {
                            tool: tool.into(),
                            object: object.into(),
                            reason: format!("exec tier holds despite override: {reason}"),
                        });
                    }
                    return Ok(());
                }
            }
        }
    }
    match tier {
        ApprovalTier::Read => Ok(()),
        ApprovalTier::Write | ApprovalTier::Exec => Err(TierError::ObjectPrompt {
            tool: tool.into(),
            object: object.into(),
            reason: format!("{} tier default: approval required", tier.as_str()),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_tools_map_through_acp_classes() {
        assert_eq!(tier_of("read"), ApprovalTier::Read);
        assert_eq!(tier_of("code_search"), ApprovalTier::Read);
        assert_eq!(tier_of("write"), ApprovalTier::Write);
        assert_eq!(tier_of("plan_open"), ApprovalTier::Write);
        assert_eq!(tier_of("runProcess"), ApprovalTier::Exec);
        assert_eq!(tier_of("sast"), ApprovalTier::Exec);
    }

    #[test]
    fn unknown_defaults_to_exec_and_mcp_is_write_floor() {
        // unknown→exec (pairs with P02 unknown-fail-closed).
        assert_eq!(tier_of("ghost-tool"), ApprovalTier::Exec);
        assert_eq!(tier_of(""), ApprovalTier::Exec);
        // MCP=write: bridged read-class suffix still lands on Write...
        assert_eq!(tier_of("mcp__read"), ApprovalTier::Write);
        assert_eq!(tier_of("mcp__ghost"), ApprovalTier::Write);
        // ...while bridged exec stays Exec.
        assert_eq!(tier_of("mcp__runProcess"), ApprovalTier::Exec);
    }

    #[test]
    fn tier_option_and_floor_pair_with_acp_gate() {
        assert_eq!(
            required_option(ApprovalTier::Read),
            PermissionOption::AllowOnce
        );
        assert_eq!(
            required_option(ApprovalTier::Write),
            PermissionOption::AllowSession
        );
        assert_eq!(
            required_option(ApprovalTier::Exec),
            PermissionOption::RequireApproval
        );
        // The ACP gate agrees: exec never rides a bare allow.
        let d = crate::acp::decide("runProcess", "allow-session", None).unwrap();
        assert!(matches!(d, crate::acp::Decision::RequireApproval { .. }));
    }

    #[test]
    fn policy_floor_uses_p02_levels() {
        let locked = RuntimePolicy::locked();
        assert!(check_policy_floor(&locked, ApprovalTier::Read).is_ok());
        let err = check_policy_floor(&locked, ApprovalTier::Write).unwrap_err();
        assert!(matches!(err, TierError::Escalation { .. }));
        let err = check_policy_floor(&locked, ApprovalTier::Exec).unwrap_err();
        assert!(matches!(err, TierError::Escalation { .. }));
    }

    #[test]
    fn object_policies_refine_never_weaken() {
        let rules = vec![ObjectRule {
            pattern: "public".into(),
            effect: ObjectEffect::Allow,
            override_reason: Some("docs dir is world-readable".into()),
        }];
        // Read + allow → pass.
        assert!(evaluate_object("read", "docs/public.md", &rules).is_ok());
        // Exec + allow WITH reason → still prompt (tier holds), reason kept.
        let err = evaluate_object("runProcess", "docs/public.md", &rules).unwrap_err();
        assert!(matches!(err, TierError::ObjectPrompt { .. }));
        assert!(err.to_string().contains("exec tier holds"));
        // Allow WITHOUT reason → prompt escalation, never silent pass.
        let bare = vec![ObjectRule {
            pattern: "x".into(),
            effect: ObjectEffect::Allow,
            override_reason: None,
        }];
        assert!(matches!(
            evaluate_object("read", "x-file", &bare).unwrap_err(),
            TierError::ObjectPrompt { .. }
        ));
        // Deny wins with its reason.
        let deny = vec![ObjectRule {
            pattern: "secret".into(),
            effect: ObjectEffect::Deny,
            override_reason: Some("vault path".into()),
        }];
        let err = evaluate_object("read", "a/secret/b", &deny).unwrap_err();
        assert!(matches!(err, TierError::ObjectDenied { .. }));
        // No rule: read passes, write/exec prompt by tier default.
        assert!(evaluate_object("read", "anything", &[]).is_ok());
        assert!(evaluate_object("write", "anything", &[]).is_err());
    }
}
