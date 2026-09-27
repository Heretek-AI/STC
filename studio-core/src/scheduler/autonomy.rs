//! Manager autonomy dial (issue #8): advisory ↔ full authority, default full.
//! Anchors (by symbol): humanlayer `hld/approval/manager.go`
//! (`CreateApproval` branches: skip-permissions → auto-approve, edit-tools →
//! auto-approve, else pending + `waiting_input`); block-buzz
//! `buzz-workflow/executor.rs` (`RequestApproval` → `Suspended{approval_token}`)
//! and `buzz-db/workflow.rs` (`ApprovalRecord`, hashed token, `waiting_approval`);
//! OpenHands `settings.ts` (`confirmation_mode`).
//!
//! Enforcement lives here, never in the UI: advisory mode pauses
//! scope/dispatch/merge decisions as blocked-pending-approval, destructive ops
//! always require explicit approval in both modes, and registry `Deny` verdicts
//! are never overridden (anti-circumvention).

use crate::scheduler::actions::{ActionRegistry, Decision, Surface};

/// Per-project authority level. Default is [`Autonomy::Full`] (audit log is the
/// safety net); [`Autonomy::Advisory`] pauses consequential actions for approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Autonomy {
    Full,
    Advisory,
}

impl Autonomy {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Advisory => "advisory",
        }
    }

    /// Parse a persisted mode. Unknown values fail closed to [`Autonomy::Advisory`]
    /// (less authority), never silently escalate to full.
    pub fn parse(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "advisory" => Self::Advisory,
            "full" => Self::Full,
            _ => Self::Advisory,
        }
    }

    /// Default for projects with no stored setting: full authority.
    pub fn default_for_new_project() -> Self {
        Self::Full
    }
}

/// Repo key for the autonomy table. Mirrors the `STUDIO_REPO` bind the compose
/// stack uses; falls back to `.` so snapshots always resolve a key.
pub fn repo_key_from_env() -> String {
    std::env::var("STUDIO_REPO").unwrap_or_else(|_| ".".into())
}

/// Destructive ops: deploy, secret rotation, infra mutation, incident-resolve.
/// ALWAYS require explicit human approval regardless of mode.
pub fn is_destructive(action_id: &str) -> bool {
    let a = action_id.trim().to_lowercase();
    a.starts_with("deploy")
        || a.starts_with("secret.")
        || a.starts_with("infra.")
        || a == "incident.resolve"
}

/// Consequential ops paused for approval in advisory mode: scope decisions,
/// dispatches, merges.
pub fn is_consequential(action_id: &str) -> bool {
    let a = action_id.trim().to_lowercase();
    a.starts_with("scope.") || a.starts_with("task.dispatch") || a.starts_with("merge.")
}

/// Authorize an action under an autonomy mode. Registry `Deny` verdicts stick
/// (autonomy never escalates a denial); destructive ops force `Ask` in both
/// modes; advisory mode pauses consequential ops as `Ask`.
pub fn authorize_with_autonomy(
    registry: &ActionRegistry,
    autonomy: Autonomy,
    action_id: &str,
    surface: Surface,
) -> Decision {
    if is_destructive(action_id) {
        return match registry.authorize(action_id, surface) {
            Decision::Deny(reason) => Decision::Deny(reason),
            _ => Decision::Ask,
        };
    }
    match registry.authorize(action_id, surface) {
        Decision::Deny(reason) => Decision::Deny(reason),
        Decision::Ask => Decision::Ask,
        Decision::Allow => {
            if autonomy == Autonomy::Advisory && is_consequential(action_id) {
                Decision::Ask
            } else {
                Decision::Allow
            }
        }
    }
}

/// Receipt evidence for a gate decision (audit trail in full mode, pause record
/// in advisory mode). Serialized into `receipts.evidence`.
pub fn gate_evidence(autonomy: Autonomy, action_id: &str, surface: Surface) -> String {
    serde_json::json!({
        "autonomy": autonomy.as_str(),
        "action": action_id,
        "surface": format!("{surface:?}"),
        "destructive": is_destructive(action_id),
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduler::actions::{ActionDef, Exposure, Policy};

    fn registry() -> ActionRegistry {
        let mut r = ActionRegistry::default();
        for (id, policy) in [
            ("task.dispatch", Policy::Allow),
            ("merge.land", Policy::Ask),
            ("scope.decide", Policy::Allow),
            ("deploy.prod", Policy::Allow),
            ("secret.rotate", Policy::Allow),
            ("debug.inspect", Policy::Allow),
            ("config.write", Policy::Deny),
        ] {
            r.register(ActionDef {
                id: id.into(),
                surfaces: vec![Surface::Cockpit, Surface::Tui, Surface::Cli],
                exposure: Exposure::Direct,
                policy,
            });
        }
        r
    }

    #[test]
    fn full_mode_dispatches_without_taps() {
        let r = registry();
        assert_eq!(
            authorize_with_autonomy(&r, Autonomy::Full, "task.dispatch", Surface::Cli),
            Decision::Allow
        );
    }

    #[test]
    fn advisory_pauses_scope_dispatch_merge() {
        let r = registry();
        for action in ["task.dispatch", "scope.decide", "merge.land"] {
            assert_eq!(
                authorize_with_autonomy(&r, Autonomy::Advisory, action, Surface::Cockpit),
                Decision::Ask,
                "{action} must pause in advisory mode"
            );
        }
    }

    #[test]
    fn destructive_ops_always_require_approval() {
        let r = registry();
        for mode in [Autonomy::Full, Autonomy::Advisory] {
            for action in [
                "deploy.prod",
                "secret.rotate",
                "infra.mutate",
                "incident.resolve",
            ] {
                // infra.mutate / incident.resolve are unknown to the registry:
                // unknown denies, which also satisfies "requires approval".
                let d = authorize_with_autonomy(&r, mode, action, Surface::Cli);
                assert!(
                    matches!(d, Decision::Ask | Decision::Deny(_)),
                    "{action} in {mode:?} must not Allow, got {d:?}"
                );
            }
        }
        // Registered-but-Allowed destructive ops force Ask even in full mode.
        assert_eq!(
            authorize_with_autonomy(&r, Autonomy::Full, "deploy.prod", Surface::Cli),
            Decision::Ask
        );
    }

    #[test]
    fn deny_sticks_under_both_modes() {
        let r = registry();
        for mode in [Autonomy::Full, Autonomy::Advisory] {
            assert!(matches!(
                authorize_with_autonomy(&r, mode, "config.write", Surface::Cli),
                Decision::Deny(_)
            ));
        }
    }

    #[test]
    fn unknown_mode_fails_closed_to_advisory() {
        assert_eq!(Autonomy::parse("bogus"), Autonomy::Advisory);
        assert_eq!(Autonomy::parse("full"), Autonomy::Full);
        assert_eq!(Autonomy::default_for_new_project(), Autonomy::Full);
    }
}
