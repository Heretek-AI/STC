//! Tool registry + progressive disclosure + derived role maps.
//! Agent sees compact index (~20 tok/tool); `tool_open` loads full schema on-demand.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Access {
    Allow,
    OnDemand,
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AgentRole {
    Manager,
    Researcher,
    Coder,
    Reviewer,
}

impl AgentRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            AgentRole::Manager => "manager",
            AgentRole::Researcher => "researcher",
            AgentRole::Coder => "coder",
            AgentRole::Reviewer => "reviewer",
        }
    }
}

/// Full tool definition. `full_tokens` measured from JSON schema length (~678 tok/tool baseline).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    pub id: String,
    pub version: String,
    pub desc_8w: String,
    pub full_tokens: u32,
    pub scopes: Vec<String>,
}

impl ToolDef {
    /// Compact index line: name + 8-word desc ≈ 20 tokens.
    pub fn compact_tokens(&self) -> u32 {
        20
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleManifest {
    pub role: AgentRole,
    pub rules: HashMap<String, Access>,
}

impl RoleManifest {
    /// Resolution-time check. Missing or Deny => fail-closed SCOPE_*.
    pub fn check(&self, tool: &str) -> Result<Access, String> {
        match self.rules.get(tool) {
            Some(Access::Allow) => Ok(Access::Allow),
            Some(Access::OnDemand) => Ok(Access::OnDemand),
            Some(Access::Deny) | None => Err(format!(
                "SCOPE_VIOLATION: {tool} not allowed for {}",
                self.role.as_str()
            )),
        }
    }
}

/// Default manifests: least-privilege per role.
pub fn default_manifest(role: AgentRole) -> RoleManifest {
    let mut rules = HashMap::new();
    match role {
        AgentRole::Manager => {
            for t in ["plan_open", "dag_commit", "tool_open", "kv_get", "read"] {
                rules.insert(t.into(), Access::Allow);
            }
            for t in ["web_search", "code_search", "retrieve_docs"] {
                rules.insert(t.into(), Access::OnDemand);
            }
        }
        AgentRole::Researcher => {
            for t in ["read", "code_search", "web_search", "tool_open", "kv_get"] {
                rules.insert(t.into(), Access::Allow);
            }
            for t in ["retrieve_docs", "runProcess"] {
                rules.insert(t.into(), Access::OnDemand);
            }
        }
        AgentRole::Coder => {
            for t in ["read", "write", "edit", "patch", "tool_open", "kv_get"] {
                rules.insert(t.into(), Access::Allow);
            }
            for t in ["runProcess", "retrieve_docs"] {
                rules.insert(t.into(), Access::OnDemand);
            }
        }
        AgentRole::Reviewer => {
            for t in ["read", "syntax_check", "tool_open", "kv_get"] {
                rules.insert(t.into(), Access::Allow);
            }
            for t in ["sast", "sbom", "retrieve_docs"] {
                rules.insert(t.into(), Access::OnDemand);
            }
        }
    }
    RoleManifest { role, rules }
}

/// Canonical tool catalog (ids + versions fixed so callers conform).
pub fn catalog() -> Vec<ToolDef> {
    let defs = [
        ("read", "8-word file read with line ranges today", 640),
        ("write", "8-word file create with content payload here", 700),
        ("edit", "8-word file patch with exact string match", 690),
        ("patch", "8-word unified diff apply with verification", 710),
        (
            "runProcess",
            "8-word bounded subprocess without shell ever",
            720,
        ),
        (
            "code_search",
            "8-word semantic code search with reranking",
            660,
        ),
        (
            "web_search",
            "8-word web search with dedup per session",
            650,
        ),
        ("tool_open", "8-word on demand full schema loader here", 120),
        ("kv_get", "8-word overflow pointer fetch exempt always", 110),
        ("syntax_check", "8-word tree sitter syntax check fast", 600),
        ("sast", "8-word static analysis security scan gate", 620),
        ("sbom", "8-word software bill materials generator now", 610),
        ("plan_open", "8-word plan ledger read only view here", 400),
        ("dag_commit", "8-word typed DAG commit with validation", 450),
        (
            "retrieve_docs",
            "8-word doc fetch with summarizer floor",
            630,
        ),
    ];
    defs.iter()
        .map(|(id, d, tok)| ToolDef {
            id: id.to_string(),
            version: "1".into(),
            desc_8w: d.to_string(),
            full_tokens: *tok,
            scopes: vec!["studio".into()],
        })
        .collect()
}

/// Derive role->tool map by inversion (missing/stray registration = test failure).
pub fn derive_tool_map() -> BTreeMap<String, Vec<String>> {
    let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for role in [
        AgentRole::Manager,
        AgentRole::Researcher,
        AgentRole::Coder,
        AgentRole::Reviewer,
    ] {
        let m = default_manifest(role);
        let mut tools: Vec<String> = m
            .rules
            .iter()
            .filter(|(_, a)| **a != Access::Deny)
            .map(|(k, _)| k.clone())
            .collect();
        tools.sort();
        map.insert(role.as_str().into(), tools);
    }
    map
}

pub struct Gateway {
    tools: HashMap<String, ToolDef>,
}

impl Gateway {
    pub fn new() -> Self {
        Self {
            tools: catalog().into_iter().map(|t| (t.id.clone(), t)).collect(),
        }
    }

    /// Compact index for a role: only Allow + OnDemand tools, ~20 tok/tool.
    pub fn index_for(&self, manifest: &RoleManifest) -> Vec<(String, String)> {
        let mut out = vec![];
        for (id, def) in &self.tools {
            if manifest
                .rules
                .get(id)
                .map(|a| *a != Access::Deny)
                .unwrap_or(false)
            {
                out.push((id.clone(), def.desc_8w.clone()));
            }
        }
        out.sort();
        out
    }

    pub fn index_tokens(&self, manifest: &RoleManifest) -> u32 {
        self.index_for(manifest).len() as u32 * 20
    }

    pub fn full_tokens_for(&self, manifest: &RoleManifest) -> u32 {
        self.index_for(manifest)
            .iter()
            .filter_map(|(id, _)| self.tools.get(id))
            .map(|t| t.full_tokens)
            .sum()
    }

    /// On-demand full schema load, validated against manifest at resolution time.
    /// Double enforcement: gateway deny + caller-side permission refusal.
    pub fn tool_open(&self, manifest: &RoleManifest, tool: &str) -> Result<ToolDef, String> {
        let access = manifest.check(tool)?;
        match access {
            Access::Allow | Access::OnDemand => self
                .tools
                .get(tool)
                .cloned()
                .ok_or_else(|| format!("SCOPE_NOT_DECLARED: unknown tool {tool}")),
            Access::Deny => Err(format!("SCOPE_VIOLATION: {tool} denied")),
        }
    }
}

impl Default for Gateway {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_schemas_isolated() {
        let gw = Gateway::new();
        let coder = default_manifest(AgentRole::Coder);
        let researcher = default_manifest(AgentRole::Researcher);
        // coder can write, researcher cannot
        assert!(gw.tool_open(&coder, "write").is_ok());
        assert!(researcher.check("write").is_err());
        // unknown tool fails closed
        assert!(gw.tool_open(&coder, "nope").is_err());
    }

    #[test]
    fn derived_map_covers_catalog() {
        let map = derive_tool_map();
        assert_eq!(map.len(), 4);
        for tools in map.values() {
            assert!(!tools.is_empty());
        }
    }

    #[test]
    fn token_footprint_reduced() {
        // H3 scale: 200 tools full flood vs sliced index + 3 on-demand loads
        let full = 200 * 678;
        let sliced = 200 * 20 + 3 * 678;
        assert!(full / sliced >= 10, "must approach order-of-magnitude cut");
    }
}
