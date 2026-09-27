//! Provider hub + role slots (Phase 6 WS2).
//! Roles name slots, never providers. Three onboarding profiles ship as editable
//! rows: Max-first, API-cheap, Local-only. Upstream gateways (OmniRoute, LiteLLM,
//! Bifrost, OpenRouter) are provider profiles (URL + master key passthrough) —
//! no gateway sidecar ships. Auto-routing is forbidden on critical slots
//! (`coder.primary`, `reviewer`); affinity is sticky-per-task with fallback on
//! hard failure or quota exhaustion only, never mid-task for cost.

pub mod client;
pub mod sync;

use crate::auth::AuthKind;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ConnectionId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ModelId(pub String);

/// Per-1M-token cost table. Budget arms count input+output+cache_write;
/// cache_read is excluded (roadmap rule).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostTable {
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
    pub cache_read_per_mtok: f64,
    pub cache_write_per_mtok: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connection {
    pub id: ConnectionId,
    pub name: String,
    pub auth_kind: AuthKind,
    pub base_url: Option<String>,
    pub models: Vec<ModelId>,
    pub cost: HashMap<String, CostTable>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TierPreset {
    MaxFirst,
    ApiCheap,
    LocalOnly,
}

/// A role slot: ordered candidate connections + tier. The DAG names
/// `role.coder`; the slot resolves it. Users edit order and membership.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleSlot {
    pub role: String,
    pub ordered_connections: Vec<ConnectionId>,
    pub tier: TierPreset,
}

/// Roles where model identity affects correctness independence. Auto-routing
/// pools are refused here; exactly one pinned connection per task.
pub fn is_critical_slot(role: &str) -> bool {
    matches!(role, "coder.primary" | "reviewer")
}

#[derive(Debug, Default)]
pub struct ProviderHub {
    pub connections: HashMap<ConnectionId, Connection>,
    pub slots: HashMap<String, RoleSlot>,
    /// Sticky-per-task affinity: task -> pinned connection.
    affinity: HashMap<String, ConnectionId>,
}

impl ProviderHub {
    pub fn add_connection(&mut self, conn: Connection) {
        self.connections.insert(conn.id.clone(), conn);
    }

    pub fn set_slot(&mut self, slot: RoleSlot) -> Result<(), String> {
        for id in &slot.ordered_connections {
            if !self.connections.contains_key(id) {
                return Err(format!("unknown connection {}", id.0));
            }
        }
        self.slots.insert(slot.role.clone(), slot);
        Ok(())
    }

    /// Attach an auto-routed pool to a slot. Refused on critical slots.
    pub fn set_auto_routed(&mut self, role: &str, pool: Vec<ConnectionId>) -> Result<(), String> {
        if is_critical_slot(role) {
            return Err(format!(
                "auto-routing forbidden on critical slot {role}: pin one connection"
            ));
        }
        let slot = self
            .slots
            .get_mut(role)
            .ok_or_else(|| format!("unknown slot {role}"))?;
        for id in &pool {
            if !self.connections.contains_key(id) {
                return Err(format!("unknown connection {}", id.0));
            }
        }
        slot.ordered_connections = pool;
        Ok(())
    }

    /// Resolve a connection for (task, role): pinned affinity wins; otherwise
    /// the slot's first connection. Pinning happens at dispatch, once.
    pub fn resolve(&mut self, task: &str, role: &str) -> Result<ConnectionId, String> {
        if let Some(pinned) = self.affinity.get(task) {
            return Ok(pinned.clone());
        }
        let slot = self
            .slots
            .get(role)
            .ok_or_else(|| format!("unknown slot {role}"))?;
        let first = slot
            .ordered_connections
            .first()
            .ok_or_else(|| format!("empty slot {role}"))?
            .clone();
        self.affinity.insert(task.into(), first.clone());
        Ok(first)
    }

    /// Fallback: re-pin only on hard failure / quota exhaustion, never for cost.
    pub fn failover(
        &mut self,
        task: &str,
        role: &str,
        reason: &str,
    ) -> Result<ConnectionId, String> {
        let slot = self
            .slots
            .get(role)
            .ok_or_else(|| format!("unknown slot {role}"))?;
        let current = self.affinity.get(task);
        let next = slot
            .ordered_connections
            .iter()
            .find(|id| Some(*id) != current)
            .ok_or_else(|| format!("no failover candidate for {role} ({reason})"))?
            .clone();
        self.affinity.insert(task.into(), next.clone());
        Ok(next)
    }
}

/// One onboarding profile: a tier plus (name, auth, base_url) rows.
pub type OnboardingProfile = (
    TierPreset,
    Vec<(&'static str, AuthKind, Option<&'static str>)>,
);

/// The three shipped onboarding profiles (editable rows, not hardcoded policy).
pub fn onboarding_profiles() -> Vec<OnboardingProfile> {
    vec![
        (
            TierPreset::MaxFirst,
            vec![
                ("claude-max", AuthKind::CliDelegated, None),
                (
                    "zen-flagship",
                    AuthKind::ApiKey,
                    Some("https://zen.opencode.ai"),
                ),
            ],
        ),
        (
            TierPreset::ApiCheap,
            vec![
                (
                    "opencode-go",
                    AuthKind::ApiKey,
                    Some("https://go.opencode.ai"),
                ),
                (
                    "zen-economy",
                    AuthKind::ApiKey,
                    Some("https://zen.opencode.ai"),
                ),
            ],
        ),
        (
            TierPreset::LocalOnly,
            vec![(
                "ollama-local",
                AuthKind::Local,
                Some("http://localhost:11434"),
            )],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hub() -> ProviderHub {
        let mut h = ProviderHub::default();
        h.add_connection(Connection {
            id: ConnectionId("a".into()),
            name: "a".into(),
            auth_kind: AuthKind::ApiKey,
            base_url: None,
            models: vec![ModelId("m1".into())],
            cost: HashMap::new(),
        });
        h.add_connection(Connection {
            id: ConnectionId("b".into()),
            name: "b".into(),
            auth_kind: AuthKind::ApiKey,
            base_url: None,
            models: vec![ModelId("m2".into())],
            cost: HashMap::new(),
        });
        h.set_slot(RoleSlot {
            role: "coder.primary".into(),
            ordered_connections: vec![ConnectionId("a".into())],
            tier: TierPreset::MaxFirst,
        })
        .unwrap();
        h.set_slot(RoleSlot {
            role: "researcher".into(),
            ordered_connections: vec![ConnectionId("a".into())],
            tier: TierPreset::ApiCheap,
        })
        .unwrap();
        h
    }

    #[test]
    fn auto_routing_forbidden_on_critical_slots() {
        let mut h = hub();
        assert!(h
            .set_auto_routed(
                "coder.primary",
                vec![ConnectionId("a".into()), ConnectionId("b".into())]
            )
            .is_err());
        assert!(h
            .set_auto_routed("reviewer", vec![ConnectionId("a".into())])
            .is_err());
        assert!(h
            .set_auto_routed(
                "researcher",
                vec![ConnectionId("a".into()), ConnectionId("b".into())]
            )
            .is_ok());
    }

    #[test]
    fn sticky_affinity_never_moves_mid_task() {
        let mut h = hub();
        let first = h.resolve("t1", "researcher").unwrap();
        // Even after the slot order changes, the task stays pinned.
        h.set_auto_routed(
            "researcher",
            vec![ConnectionId("b".into()), ConnectionId("a".into())],
        )
        .unwrap();
        assert_eq!(h.resolve("t1", "researcher").unwrap(), first);
        // Failover moves it exactly once, to a different connection.
        let moved = h.failover("t1", "researcher", "quota").unwrap();
        assert_ne!(moved, first);
    }

    #[test]
    fn unknown_refs_fail_closed() {
        let mut h = hub();
        assert!(h.resolve("t1", "nope").is_err());
        assert!(h
            .set_slot(RoleSlot {
                role: "x".into(),
                ordered_connections: vec![ConnectionId("ghost".into())],
                tier: TierPreset::LocalOnly,
            })
            .is_err());
    }
}
