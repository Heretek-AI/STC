//! MCP adapter charter: manifest→capabilities registry with admission
//! ceilings, schema/description bounds, tool-name grammar, typed failures,
//! and a single egress seam.
//!
//! Clean-room re-derivation (nearai/ironclaw, Apache-2.0 dual) behind
//! citations c015
//! (`7ad76256c19b661dfe96a07af5d1a8b8eb82f035b7a5483c819721e3ef8af4fc`)
//! and c016
//! (`3e283a44f89a1936337e7c53dc67ee58ab647b3db7a3c0b85921e4c15adeba51`).
//! No upstream code is copied: the re-derived rules are — the catalog is
//! declared by a manifest (never probed implicitly); admission is bounded
//! (tool count, per-tool schema bytes, description chars, name grammar);
//! every failure is a typed [`RegistryError`]; all execution crosses ONE
//! seam ([`Registry::invoke`]), so there is no side channel around the
//! unknown→deny check.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use thiserror::Error;

/// Admission ceilings (charter bounds). Manifests breaching any ceiling
/// fail the whole load typed — never truncate-and-allow silently.
pub const MAX_TOOLS: usize = 64;
pub const MAX_SCHEMA_BYTES: usize = 8192;
pub const MAX_DESC_CHARS: usize = 1024;
pub const MAX_TOOL_NAME_LEN: usize = 64;
/// Context-overflow cap: the bounded tool index (name + description only)
/// must fit in this budget. Breach → [`RegistryError::IndexOverflow`].
pub const MAX_INDEX_BYTES: usize = 16384;
/// Memory bound (second tier beside the context cap above): the TOTAL
/// stored-schema footprint across the manifest must fit here. Breach →
/// [`RegistryError::SchemaStoreOverflow`]. Two tiers, two rationales: the
/// index cap bounds per-call CONTEXT (what a model sees); the store cap
/// bounds process MEMORY (what the gateway holds). Per-tool schema bytes
/// are already ceilinged by `MAX_SCHEMA_BYTES`; this caps their sum so 64
/// near-ceiling schemas cannot jointly exhaust the host.
pub const MAX_SCHEMA_STORE_BYTES: usize = 256 * 1024;
/// Search/lens expansion bound shared with [`crate::mcp::toolmatch`].
pub const MAX_EXPANSION: usize = 16;

/// Tool-name grammar: `^[A-Za-z][A-Za-z0-9_.-]{0,63}$`.
/// Single canonical grammar for manifest names, lens keys, and CLI
/// subcommands — one rule, enforced at admission.
pub fn validate_tool_name(name: &str) -> Result<(), RegistryError> {
    if name.is_empty() || name.len() > MAX_TOOL_NAME_LEN {
        return Err(RegistryError::NameInvalid {
            tool: name.into(),
            reason: "length out of bounds 1..=64",
        });
    }
    let mut chars = name.chars();
    let first = chars.next().unwrap_or_default();
    if !first.is_ascii_alphabetic() {
        return Err(RegistryError::NameInvalid {
            tool: name.into(),
            reason: "must start with ASCII letter",
        });
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-')
    {
        return Err(RegistryError::NameInvalid {
            tool: name.into(),
            reason: "allowed chars are [A-Za-z0-9_.-]",
        });
    }
    Ok(())
}

/// Typed registry failures. No `String` errors cross this boundary.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RegistryError {
    #[error("unknown tool {tool}: no manifest entry — fail-closed to deny")]
    UnknownTool { tool: String },
    #[error("invalid tool name {tool}: {reason}")]
    NameInvalid { tool: String, reason: &'static str },
    #[error("manifest holds {found} tools: ceiling is {MAX_TOOLS}")]
    CatalogFull { found: usize },
    #[error("tool {tool} schema is {found} bytes: ceiling is {MAX_SCHEMA_BYTES}")]
    SchemaTooLarge { tool: String, found: usize },
    #[error("tool {tool} description is {found} chars: ceiling is {MAX_DESC_CHARS}")]
    DescriptionTooLong { tool: String, found: usize },
    #[error("tool index is {found} bytes: overflow cap is {MAX_INDEX_BYTES}")]
    IndexOverflow { found: usize },
    #[error("schema store is {found} bytes: memory cap is {MAX_SCHEMA_STORE_BYTES}")]
    SchemaStoreOverflow { found: usize },
    #[error("tool {tool} is not yet opened: call tool_open first (on-demand schema)")]
    SchemaNotOpened { tool: String },
    #[error("tool {tool} is deny-listed for this role: slice bound, uncallable")]
    DenyListed { tool: String },
    #[error("tool {tool} is disabled by lens (unlisted + uncallable)")]
    LensDisabled { tool: String },
    #[error("manifest parse: {detail}")]
    ManifestParse { detail: String },
    #[error("invoke denied: {reason}")]
    Denied { reason: String },
}

/// One manifest-declared tool. The manifest is the ONLY capability source:
/// tools not declared here do not exist as far as the gateway is concerned.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ManifestEntry {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Full JSON Schema. Stored in the schema store on load; NEVER copied
    /// into the bounded index (tool_open-on-demand semantics).
    #[serde(default)]
    pub schema: Option<serde_json::Value>,
}

/// The bounded index row: name + description + version + tags only.
/// Schemas are reachable solely through [`Registry::tool_open`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IndexEntry {
    pub name: String,
    pub description: String,
    pub version: String,
    pub tags: Vec<String>,
    pub on_demand: bool,
}

/// Per-role slice mode: Allow (listed + callable), OnDemand (listed as
/// on-demand, callable only after `tool_open`), Deny (excluded entirely).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SliceMode {
    Allow,
    OnDemand,
    Deny,
}

impl SliceMode {
    pub fn parse(s: &str) -> Result<Self, RegistryError> {
        match s {
            "allow" => Ok(SliceMode::Allow),
            "ondemand" | "on-demand" => Ok(SliceMode::OnDemand),
            "deny" => Ok(SliceMode::Deny),
            _ => Err(RegistryError::Denied {
                reason: format!("unknown slice mode {s}: fail-closed to deny"),
            }),
        }
    }
}

/// The gateway registry: manifest-loaded catalog + lazy schema store +
/// the single egress seam.
///
/// Role binding: [`slice`](Registry::slice) records which tools a role
/// holds OnDemand AND which are Deny-listed; [`invoke`](Registry::invoke)
/// refuses on-demand tools until [`tool_open`](Registry::tool_open) has
/// run, and refuses deny-listed tools unconditionally (the `opened` set
/// is the enforcement record, not telemetry). Re-slicing is last-wins:
/// each listed tool's membership follows its LATEST mode (an Allow
/// re-slice clears a prior OnDemand/Deny binding for that tool, so rows
/// and seam can never desync). Lens gating is enforced at the seam too —
/// `invoke` takes the active [`crate::mcp::lens::ToolLens`] and a disabled
/// tool is uncallable through every path.
#[derive(Debug, Default)]
pub struct Registry {
    index: HashMap<String, IndexEntry>,
    schemas: HashMap<String, serde_json::Value>,
    opened: HashSet<String>,
    ondemand: HashSet<String>,
    denied: HashSet<String>,
}

impl Registry {
    /// Load a registry from a manifest JSON array of [`ManifestEntry`].
    /// Enforces name grammar, description bound, per-tool schema bound,
    /// TOTAL schema-store bound, catalog ceiling, and the index overflow
    /// cap. Any breach fails the WHOLE load typed (never partial-allow).
    pub fn load_manifest(json: &str) -> Result<Self, RegistryError> {
        let entries: Vec<ManifestEntry> =
            serde_json::from_str(json).map_err(|e| RegistryError::ManifestParse {
                detail: e.to_string(),
            })?;
        if entries.len() > MAX_TOOLS {
            return Err(RegistryError::CatalogFull {
                found: entries.len(),
            });
        }
        let mut reg = Registry::default();
        for e in &entries {
            validate_tool_name(&e.name)?;
            if e.description.chars().count() > MAX_DESC_CHARS {
                return Err(RegistryError::DescriptionTooLong {
                    tool: e.name.clone(),
                    found: e.description.chars().count(),
                });
            }
            if let Some(s) = &e.schema {
                let bytes = serde_json::to_string(s)
                    .map(|s| s.len())
                    .unwrap_or(MAX_SCHEMA_BYTES + 1);
                if bytes > MAX_SCHEMA_BYTES {
                    return Err(RegistryError::SchemaTooLarge {
                        tool: e.name.clone(),
                        found: bytes,
                    });
                }
            }
            if reg.index.contains_key(&e.name) {
                return Err(RegistryError::Denied {
                    reason: format!("duplicate manifest tool {}", e.name),
                });
            }
            reg.index.insert(
                e.name.clone(),
                IndexEntry {
                    name: e.name.clone(),
                    description: e.description.clone(),
                    version: e.version.clone(),
                    tags: e.tags.clone(),
                    on_demand: false,
                },
            );
            if let Some(s) = e.schema.clone() {
                reg.schemas.insert(e.name.clone(), s);
            }
        }
        reg.check_overflow()?;
        reg.check_schema_store()?;
        Ok(reg)
    }

    /// Current index footprint in bytes (name + description per row).
    /// The Q9 bar budget: schemas never count (they are not in the index).
    pub fn index_bytes(&self) -> usize {
        self.index
            .values()
            .map(|e| e.name.len() + e.description.len())
            .sum()
    }

    fn check_overflow(&self) -> Result<(), RegistryError> {
        let found = self.index_bytes();
        if found > MAX_INDEX_BYTES {
            return Err(RegistryError::IndexOverflow { found });
        }
        Ok(())
    }

    /// Current schema-store footprint in bytes (serialized schemas).
    pub fn schema_store_bytes(&self) -> usize {
        self.schemas
            .values()
            .map(|s| serde_json::to_string(s).map(|t| t.len()).unwrap_or(0))
            .sum()
    }

    fn check_schema_store(&self) -> Result<(), RegistryError> {
        let found = self.schema_store_bytes();
        if found > MAX_SCHEMA_STORE_BYTES {
            return Err(RegistryError::SchemaStoreOverflow { found });
        }
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    pub fn contains(&self, tool: &str) -> bool {
        self.index.contains_key(tool)
    }

    /// Slice the catalog per role. Every requested tool must exist in the
    /// manifest (unknown → typed `UnknownTool`, never widened). Deny-mode
    /// tools are excluded from the returned rows AND recorded in the deny
    /// set: [`invoke`](Registry::invoke) refuses them with typed
    /// `DenyListed` (QA-R2: the deny membership used to live nowhere, so a
    /// deny-sliced tool still invoked `Ok`). OnDemand-mode tools are
    /// recorded in the role binding: `invoke` refuses them until
    /// [`tool_open`](Registry::tool_open) runs (QA-R1). Re-slicing is
    /// last-wins per tool (QA-R2 N2): an Allow re-slice clears a prior
    /// OnDemand/Deny binding, so the returned rows and the seam agree.
    pub fn slice(
        &mut self,
        role_tools: &[(String, SliceMode)],
    ) -> Result<Vec<IndexEntry>, RegistryError> {
        let mut out = Vec::new();
        for (name, mode) in role_tools {
            let base = self
                .index
                .get(name)
                .ok_or_else(|| RegistryError::UnknownTool { tool: name.clone() })?;
            match mode {
                SliceMode::Deny => {
                    self.ondemand.remove(name);
                    self.denied.insert(name.clone());
                }
                SliceMode::Allow => {
                    self.ondemand.remove(name);
                    self.denied.remove(name);
                    out.push(IndexEntry {
                        on_demand: false,
                        ..base.clone()
                    })
                }
                SliceMode::OnDemand => {
                    self.denied.remove(name);
                    self.ondemand.insert(name.clone());
                    out.push(IndexEntry {
                        on_demand: true,
                        ..base.clone()
                    })
                }
            }
        }
        Ok(out)
    }

    /// Lazy on-demand full-schema load. The index stays bounded: this
    /// returns an owned clone of the stored schema without copying it into
    /// any index row. Unknown tools deny typed.
    pub fn tool_open(&mut self, tool: &str) -> Result<serde_json::Value, RegistryError> {
        if !self.index.contains_key(tool) {
            return Err(RegistryError::UnknownTool { tool: tool.into() });
        }
        let schema = self
            .schemas
            .get(tool)
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        self.opened.insert(tool.into());
        Ok(schema)
    }

    pub fn is_opened(&self, tool: &str) -> bool {
        self.opened.contains(tool)
    }

    /// Write-intent detection: the write-intent contract labels a call
    /// write-class when the tool tier is Write/Exec (via
    /// [`crate::mcp::tiers::tier_of`]) OR the target looks destructive via
    /// `crate::acp::is_destructive_intent` OR the target escapes the
    /// workspace (absolute path or `..` — checked for EVERY tier, since a
    /// read-class tool pointed outside the workspace is exfiltration-shaped
    /// regardless of tier; QA-R1) OR the argument text carries mutate
    /// verbs. Read-class tools with benign relative targets return false.
    pub fn detect_write_intent(tool: &str, target: Option<&str>, argv: &str) -> bool {
        use crate::mcp::tiers::{tier_of, ApprovalTier};
        // Workspace escape is tier-independent: no read-class tool has a
        // benign reason to address an absolute path or `..`.
        if let Some(t) = target {
            if t.contains("..") || t.starts_with('/') {
                return true;
            }
        }
        if matches!(tier_of(tool), ApprovalTier::Write | ApprovalTier::Exec) {
            return true;
        }
        if crate::acp::is_destructive_intent(tool, target) {
            return true;
        }
        let low = argv.to_lowercase();
        [
            "write", "edit", "delete", "create", "remove", "mkdir", "rm ", "patch", "chmod",
            "truncate", "rename", "unlink", "mv ", "cp ",
        ]
        .iter()
        .any(|w| low.contains(w))
    }

    /// THE single egress seam. Every tool call crosses here, in
    /// fail-closed order: unknown → deny-listed → on-demand-unopened →
    /// lens-disabled → executor. No caller can bypass any check by holding
    /// the executor directly (it is a parameter, and nothing executes
    /// outside this function). The executor is taken as a parameter
    /// precisely so tests inject fakes without the registry ever owning a
    /// subprocess or socket.
    pub fn invoke<E: ToolExecutor>(
        &self,
        executor: &E,
        lens: &crate::mcp::lens::ToolLens,
        tool: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, RegistryError> {
        let entry = self
            .index
            .get(tool)
            .ok_or_else(|| RegistryError::UnknownTool { tool: tool.into() })?;
        if self.denied.contains(tool) {
            return Err(RegistryError::DenyListed { tool: tool.into() });
        }
        if self.ondemand.contains(tool) && !self.opened.contains(tool) {
            return Err(RegistryError::SchemaNotOpened { tool: tool.into() });
        }
        lens.check_callable(entry)?;
        executor
            .execute(tool, args)
            .map_err(|reason| RegistryError::Denied { reason })
    }
}

/// Executor behind the single egress seam. In-process fakes in tests;
/// the pool (`crate::mcp::pool`) binds real workers without bypassing
/// [`Registry::invoke`].
pub trait ToolExecutor {
    fn execute(&self, tool: &str, args: serde_json::Value) -> Result<serde_json::Value, String>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    pub(crate) fn manifest_two() -> String {
        json!([
            {"name": "read", "description": "read a file", "version": "1.0.0",
             "tags": ["fs"], "schema": {"type": "object"}},
            {"name": "write", "description": "write a file", "version": "1.0.0",
             "tags": ["fs"], "schema": {"type": "object"}}
        ])
        .to_string()
    }

    struct OkExec;
    impl ToolExecutor for OkExec {
        fn execute(&self, tool: &str, _a: serde_json::Value) -> Result<serde_json::Value, String> {
            Ok(json!({"tool": tool}))
        }
    }

    #[test]
    fn loads_from_manifest_with_bounds() {
        let reg = Registry::load_manifest(&manifest_two()).unwrap();
        assert_eq!(reg.len(), 2);
        assert!(reg.index_bytes() > 0);
        assert!(reg.index_bytes() <= MAX_INDEX_BYTES);
    }

    #[test]
    fn bad_names_and_bounds_fail_typed() {
        assert!(matches!(
            validate_tool_name("9bad").unwrap_err(),
            RegistryError::NameInvalid { .. }
        ));
        assert!(matches!(
            validate_tool_name("has space").unwrap_err(),
            RegistryError::NameInvalid { .. }
        ));
        let long = "a".repeat(65);
        assert!(matches!(
            validate_tool_name(&long).unwrap_err(),
            RegistryError::NameInvalid { .. }
        ));
        // Description over ceiling fails the whole load.
        let m = json!([{"name": "read", "description": "x".repeat(2000)}]).to_string();
        assert!(matches!(
            Registry::load_manifest(&m).unwrap_err(),
            RegistryError::DescriptionTooLong { .. }
        ));
        // Catalog ceiling fails the whole load.
        let many: Vec<_> = (0..70)
            .map(|i| json!({"name": format!("tool{i:02}"), "description": "d"}))
            .collect();
        assert!(matches!(
            Registry::load_manifest(&json!(many).to_string()).unwrap_err(),
            RegistryError::CatalogFull { .. }
        ));
    }

    #[test]
    fn slice_allow_ondemand_deny_and_unknown_deny() {
        let mut reg = Registry::load_manifest(&manifest_two()).unwrap();
        let rows = reg
            .slice(&[
                ("read".into(), SliceMode::Allow),
                ("write".into(), SliceMode::OnDemand),
            ])
            .unwrap();
        assert!(!rows[0].on_demand);
        assert!(rows[1].on_demand);
        // Deny excludes.
        let rows = reg.slice(&[("read".into(), SliceMode::Deny)]).unwrap();
        assert!(rows.is_empty());
        // Unknown in role list denies typed (never widened).
        assert!(matches!(
            reg.slice(&[("ghost".into(), SliceMode::Allow)])
                .unwrap_err(),
            RegistryError::UnknownTool { .. }
        ));
        assert!(matches!(
            SliceMode::parse("allow-sometimes").unwrap_err(),
            RegistryError::Denied { .. }
        ));
    }

    #[test]
    fn tool_open_is_lazy_and_index_stays_bounded() {
        let mut reg = Registry::load_manifest(&manifest_two()).unwrap();
        let before = reg.index_bytes();
        let schema = reg.tool_open("read").unwrap();
        assert_eq!(schema, json!({"type": "object"}));
        assert!(reg.is_opened("read"));
        assert!(!reg.is_opened("write"));
        // Opening a schema does not grow the bounded index.
        assert_eq!(reg.index_bytes(), before);
        assert!(matches!(
            reg.tool_open("ghost").unwrap_err(),
            RegistryError::UnknownTool { .. }
        ));
    }

    #[test]
    fn unknown_invoke_denies_before_executor() {
        use crate::mcp::lens::ToolLens;
        let lens = ToolLens::default();
        let reg = Registry::load_manifest(&manifest_two()).unwrap();
        let err = reg.invoke(&OkExec, &lens, "ghost", json!({})).unwrap_err();
        assert!(matches!(err, RegistryError::UnknownTool { .. }));
        let out = reg.invoke(&OkExec, &lens, "read", json!({})).unwrap();
        assert_eq!(out, json!({"tool": "read"}));
    }

    #[test]
    fn ondemand_bypass_is_closed_at_the_seam() {
        // QA-R1: slice(write, OnDemand) then invoke(write) used to Ok
        // without tool_open (opened was write-only telemetry). Now the
        // seam demands the open first, typed.
        use crate::mcp::lens::ToolLens;
        let lens = ToolLens::default();
        let mut reg = Registry::load_manifest(&manifest_two()).unwrap();
        reg.slice(&[("write".into(), SliceMode::OnDemand)]).unwrap();
        assert!(matches!(
            reg.invoke(&OkExec, &lens, "write", json!({})).unwrap_err(),
            RegistryError::SchemaNotOpened { .. }
        ));
        reg.tool_open("write").unwrap();
        let out = reg.invoke(&OkExec, &lens, "write", json!({})).unwrap();
        assert_eq!(out, json!({"tool": "write"}));
        // Allow-mode tools never need the open.
        let mut reg = Registry::load_manifest(&manifest_two()).unwrap();
        reg.slice(&[("read".into(), SliceMode::Allow)]).unwrap();
        assert!(reg.invoke(&OkExec, &lens, "read", json!({})).is_ok());
    }

    #[test]
    fn deny_listed_is_refused_at_the_seam_typed() {
        // QA-R2 N1 (BLOCKING): slice(write, Deny) returned [] but
        // invoke(write) → Ok (deny membership lived nowhere). Now the
        // deny set gates the seam with a dedicated typed variant.
        use crate::mcp::lens::ToolLens;
        let lens = ToolLens::default();
        let mut reg = Registry::load_manifest(&manifest_two()).unwrap();
        let rows = reg.slice(&[("write".into(), SliceMode::Deny)]).unwrap();
        assert!(rows.is_empty());
        assert!(matches!(
            reg.invoke(&OkExec, &lens, "write", json!({})).unwrap_err(),
            RegistryError::DenyListed { .. }
        ));
        // Deny holds even after tool_open (open is not an un-deny).
        reg.tool_open("write").unwrap();
        assert!(matches!(
            reg.invoke(&OkExec, &lens, "write", json!({})).unwrap_err(),
            RegistryError::DenyListed { .. }
        ));
        // Non-denied tools still invoke.
        assert!(reg.invoke(&OkExec, &lens, "read", json!({})).is_ok());
    }

    #[test]
    fn reslice_is_last_wins_no_row_seam_desync() {
        // QA-R2 N2: Allow-after-OnDemand rows reported on_demand=false
        // while the seam still demanded the open. Now re-slicing moves
        // membership, so rows and seam agree.
        use crate::mcp::lens::ToolLens;
        let lens = ToolLens::default();
        let mut reg = Registry::load_manifest(&manifest_two()).unwrap();
        reg.slice(&[("write".into(), SliceMode::OnDemand)]).unwrap();
        let rows = reg.slice(&[("write".into(), SliceMode::Allow)]).unwrap();
        assert!(!rows[0].on_demand);
        assert!(reg.invoke(&OkExec, &lens, "write", json!({})).is_ok());
        // And Deny-after-Allow binds the deny (last wins both ways).
        reg.slice(&[("write".into(), SliceMode::Deny)]).unwrap();
        assert!(matches!(
            reg.invoke(&OkExec, &lens, "write", json!({})).unwrap_err(),
            RegistryError::DenyListed { .. }
        ));
        // Allow-after-Deny releases it again.
        let rows = reg.slice(&[("write".into(), SliceMode::Allow)]).unwrap();
        assert_eq!(rows.len(), 1);
        assert!(reg.invoke(&OkExec, &lens, "write", json!({})).is_ok());
    }

    #[test]
    fn lens_disabled_is_uncallable_through_the_seam() {
        // QA-R1: a lens-disabled tool used to invoke Ok (invoke took no
        // lens). Now disabled = uncallable through the single seam.
        use crate::mcp::lens::ToolLens;
        let lens = ToolLens {
            disabled_keys: vec!["write".into()],
            ..Default::default()
        };
        let reg = Registry::load_manifest(&manifest_two()).unwrap();
        assert!(matches!(
            reg.invoke(&OkExec, &lens, "write", json!({})).unwrap_err(),
            RegistryError::LensDisabled { .. }
        ));
        assert!(reg.invoke(&OkExec, &lens, "read", json!({})).is_ok());
        // An invalid lens fails closed at the seam too (typed, never pass).
        let bad = ToolLens {
            disabled_keys: vec!["*read".into()],
            ..Default::default()
        };
        assert!(matches!(
            reg.invoke(&OkExec, &bad, "read", json!({})).unwrap_err(),
            RegistryError::Denied { .. }
        ));
    }

    #[test]
    fn write_detect_labels_tiers_verbs_and_read_escapes() {
        assert!(Registry::detect_write_intent("write", Some("a.rs"), ""));
        assert!(Registry::detect_write_intent("runProcess", None, ""));
        assert!(!Registry::detect_write_intent("read", Some("a.rs"), "{}"));
        assert!(Registry::detect_write_intent(
            "read",
            None,
            "please WRITE this"
        ));
        assert!(Registry::detect_write_intent("edit", Some("../out.rs"), ""));
        // QA-R1: read-tier tools pointed outside the workspace are
        // exfiltration-shaped regardless of tier (acp::is_destructive_intent
        // early-returns false for Read-class, so the registry checks first).
        assert!(Registry::detect_write_intent(
            "read",
            Some("/etc/passwd"),
            "{}"
        ));
        assert!(Registry::detect_write_intent(
            "read",
            Some("../secret"),
            "{}"
        ));
        assert!(Registry::detect_write_intent(
            "code_search",
            Some("/etc/shadow"),
            ""
        ));
        // QA-R1: missing mutate verbs now covered.
        assert!(Registry::detect_write_intent(
            "read",
            None,
            "run chmod +x out"
        ));
        assert!(Registry::detect_write_intent(
            "read",
            None,
            "truncate -s0 log"
        ));
        assert!(Registry::detect_write_intent("read", None, "please mv a b"));
        assert!(Registry::detect_write_intent("read", None, "please cp a b"));
        assert!(Registry::detect_write_intent(
            "read",
            None,
            "rename old new"
        ));
        assert!(Registry::detect_write_intent("read", None, "unlink stale"));
        // Benign read-class traffic still passes.
        assert!(!Registry::detect_write_intent(
            "read",
            Some("docs/guide.md"),
            "{}"
        ));
        assert!(!Registry::detect_write_intent(
            "read",
            None,
            "summarize this"
        ));
    }

    #[test]
    fn schema_store_cap_bounds_total_memory() {
        // 64 tools × ~7 KiB schemas: every per-tool ceiling holds and the
        // index stays tiny (448 B), but the 448 KiB store breaches the
        // 256 KiB memory cap → typed refusal (QA-R1 fix 7, first option).
        let big_schema = serde_json::json!({"type": "object", "pad": "x".repeat(7000)});
        let many: Vec<_> = (0..64)
            .map(|i| {
                json!({"name": format!("tool{i:02}"),
                       "description": "d",
                       "schema": big_schema})
            })
            .collect();
        let err = Registry::load_manifest(&json!(many).to_string()).unwrap_err();
        assert!(matches!(err, RegistryError::SchemaStoreOverflow { .. }));
        // Small stores load and report honestly.
        let reg = Registry::load_manifest(&manifest_two()).unwrap();
        assert!(reg.schema_store_bytes() > 0);
        assert!(reg.schema_store_bytes() <= MAX_SCHEMA_STORE_BYTES);
    }

    #[test]
    fn index_overflow_is_typed() {
        // 40 tools × ~500-char descriptions breach the 16 KiB index cap
        // while staying under every per-tool ceiling.
        let many: Vec<_> = (0..40)
            .map(|i| {
                json!({"name": format!("tool{i:02}"),
                            "description": "d".repeat(500)})
            })
            .collect();
        assert!(matches!(
            Registry::load_manifest(&json!(many).to_string()).unwrap_err(),
            RegistryError::IndexOverflow { .. }
        ));
    }
}
