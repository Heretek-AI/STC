//! Action registry + surface exposure matrix (Phase 6 WS8, happier pattern).
//! Every daemon action is typed `{action_id, surfaces, exposure, policy}`.
//! The scheduler enforces it: unknown actions and unlisted surfaces deny
//! fail-closed. `Discoverable` hides an action from indexes (Tool Lens parity:
//! compact index vs on-demand) without changing its permission.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Surface {
    Cockpit,
    Tui,
    Cli,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Exposure {
    /// Listed in indexes and menus.
    Direct,
    /// Hidden from indexes; invokable when known (on-demand).
    Discoverable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Policy {
    Allow,
    /// Requires human approval; enforced as blocked-pending, never assumed.
    Ask,
    Deny,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionDef {
    pub id: String,
    pub surfaces: Vec<Surface>,
    pub exposure: Exposure,
    pub policy: Policy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow,
    /// Blocked pending human approval.
    Ask,
    Deny(String),
}

#[derive(Debug, Default)]
pub struct ActionRegistry {
    actions: HashMap<String, ActionDef>,
}

impl ActionRegistry {
    pub fn register(&mut self, def: ActionDef) {
        self.actions.insert(def.id.clone(), def);
    }

    /// Authorize an action on a surface. Unknown action or unlisted surface
    /// denies fail-closed with a typed reason.
    pub fn authorize(&self, action_id: &str, surface: Surface) -> Decision {
        match self.actions.get(action_id) {
            None => Decision::Deny(format!("unknown action {action_id}")),
            Some(def) if !def.surfaces.contains(&surface) => {
                Decision::Deny(format!("action {action_id} not exposed on {surface:?}"))
            }
            Some(def) => match def.policy {
                Policy::Allow => Decision::Allow,
                Policy::Ask => Decision::Ask,
                Policy::Deny => Decision::Deny(format!("action {action_id} denied by policy")),
            },
        }
    }

    /// Index view: Direct actions only. Discoverable actions stay hidden
    /// until invoked by name (progressive disclosure).
    pub fn visible_actions(&self, surface: Surface) -> Vec<String> {
        let mut out: Vec<String> = self
            .actions
            .values()
            .filter(|d| d.surfaces.contains(&surface) && d.exposure == Exposure::Direct)
            .map(|d| d.id.clone())
            .collect();
        out.sort();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> ActionRegistry {
        let mut r = ActionRegistry::default();
        r.register(ActionDef {
            id: "merge.land".into(),
            surfaces: vec![Surface::Cockpit, Surface::Cli],
            exposure: Exposure::Direct,
            policy: Policy::Ask,
        });
        r.register(ActionDef {
            id: "debug.inspect".into(),
            surfaces: vec![Surface::Cli],
            exposure: Exposure::Discoverable,
            policy: Policy::Allow,
        });
        r
    }

    #[test]
    fn unknown_and_foreign_surface_deny() {
        let r = registry();
        assert!(matches!(
            r.authorize("nope", Surface::Cli),
            Decision::Deny(_)
        ));
        assert!(matches!(
            r.authorize("merge.land", Surface::Tui),
            Decision::Deny(_)
        ));
    }

    #[test]
    fn ask_is_blocked_not_assumed() {
        let r = registry();
        assert_eq!(r.authorize("merge.land", Surface::Cockpit), Decision::Ask);
        assert_eq!(r.authorize("merge.land", Surface::Cli), Decision::Ask);
    }

    #[test]
    fn discoverable_hidden_but_invokable() {
        let r = registry();
        assert!(!r
            .visible_actions(Surface::Cli)
            .contains(&"debug.inspect".to_string()));
        assert_eq!(r.authorize("debug.inspect", Surface::Cli), Decision::Allow);
        assert_eq!(
            r.visible_actions(Surface::Cli),
            vec!["merge.land".to_string()]
        );
    }
}
