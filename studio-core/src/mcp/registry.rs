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
//!
//! V03 hardening (harden-03-gateway, full rework):
//! - A1 registry-held lens: [`Registry::invoke`] takes NO lens parameter.
//!   The active [`crate::mcp::lens::ToolLens`] lives in the registry and
//!   changes only via [`Registry::rotate_lens`], which validates and appends
//!   a [`LensAuditEntry`]. A caller-held permissive lens cannot be passed,
//!   so forged-lens invocation is unrepresentable (compile-time: no param).
//! - A2 catalog-bound handles: bundle submission lives on
//!   [`Registry::submit_bundle`] and takes NO caller catalog slice. The
//!   catalog authority is `self.index`; a caller-supplied `Vec<String>`
//!   cannot be passed, so ghost-submit is unrepresentable. A
//!   [`CatalogHandle`] (generation-bound, no public constructor) is demanded
//!   so reloads invalidate old handles with typed [`crate::mcp::bundles::BundleError::StaleHandle`].
//! - C closed-by-default: a fresh registry covers NOTHING. Both
//!   [`Registry::invoke`] and [`Registry::tool_open`] demand a
//!   [`SliceReceipt`] whose generation matches [`Registry::generation`];
//!   otherwise [`RegistryError::StaleHandle`]. A known tool never sliced in
//!   the current generation refuses with [`RegistryError::NotSliced`].
//!   [`Registry::reload`] bumps the generation and clears coverage, so old
//!   receipts/handles invalidate typed.
//! - B canonical containment: [`Registry::check_containment`] binds a
//!   canonical workspace root (see [`Registry::bind_root`], reusing
//!   `crate::worktree::normalize_path` + symlink refusal). The seam never
//!   relies on substring-only path checks (`contains("..")` etc. remain as
//!   heuristics in [`Registry::detect_write_intent`], but the authoritative
//!   path gate is canonical `starts_with` on normalized absolute paths).
//! - D ordered egress: invoke enforces
//!   unknown → coverage → deny → ondemand → lens → tier/object/write-intent
//!   (incl. canonical containment) → executor. Each stage has a distinct
//!   typed variant so order is pinned by tests.
//!
//! Lockfile-neutral: std only (+ serde/serde_json/thiserror already in
//! tree). No new runtime deps. No GPL/AGPL code copied.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
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
    #[error(
        "tool {tool} was never sliced in generation {generation}: closed-by-default, slice first"
    )]
    NotSliced { tool: String, generation: u64 },
    #[error("stale handle: expected generation {expected}, found {found} — reload invalidates")]
    StaleHandle { expected: u64, found: u64 },
    #[error("containment denied for {tool} on {target}: {reason}")]
    ContainmentDenied {
        tool: String,
        target: String,
        reason: String,
    },
    #[error("tier/object gate refused {tool}: {reason}")]
    TierDenied { tool: String, reason: String },
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

/// Versioned slice receipt. Demanded by [`Registry::invoke`] and
/// [`Registry::tool_open`]. Fields are private with NO public constructor:
/// the only producer is [`Registry::slice`], so a receipt cannot be forged
/// without slicing. The receipt is generation-bound (see
/// [`Registry::generation`]): any receipt whose generation mismatches the
/// registry is refused with [`RegistryError::StaleHandle`]. Enforcement is
/// generation + receipt snapshot bound to the live slice (authoritative
/// together): `invoke`/`tool_open` demand `receipt.covered` contain the
/// tool, and `invoke` additionally demands `receipt.rows` mode match the
/// live mode (Allow vs OnDemand; Deny has no row and never invokes `Ok`).
/// A receipt minted before a tool was sliced, or before a mode change
/// (e.g. Deny→Allow), does NOT authorize the new state — re-slicing
/// within a generation DOES narrow older receipts (fail-closed); only
/// [`Registry::reload`] bumps the generation and invalidates all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SliceReceipt {
    generation: u64,
    covered: Vec<String>,
    rows: Vec<IndexEntry>,
}

impl SliceReceipt {
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn covered(&self) -> &[String] {
        &self.covered
    }
    pub fn rows(&self) -> &[IndexEntry] {
        &self.rows
    }
}

/// Catalog-bound handle for bundle submission. Demanded by
/// [`Registry::submit_bundle`]. No public constructor: only
/// [`Registry::catalog_handle`] produces one, so a caller-supplied catalog
/// vector cannot be substituted (ghost-submit unrepresentable). Stale
/// generations refuse typed via `BundleError::StaleHandle`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogHandle {
    generation: u64,
}

impl CatalogHandle {
    pub fn generation(&self) -> u64 {
        self.generation
    }
}

/// Audit record for every lens rotation. Appended by
/// [`Registry::rotate_lens`]; never edited or removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LensAuditEntry {
    pub seq: u64,
    pub generation: u64,
    pub disabled_keys: Vec<String>,
    pub disabled_tags: Vec<String>,
    pub version_req: Option<String>,
    pub note: String,
}

/// The gateway registry: manifest-loaded catalog + lazy schema store +
/// the single egress seam, plus the registry-held active lens, the bound
/// workspace root, and the closed-by-default coverage set.
///
/// Role binding: [`slice`](Registry::slice) records which tools a role
/// holds Allow/OnDemand AND which are Deny-listed, and returns a
/// [`SliceReceipt`]. [`invoke`](Registry::invoke) demands a fresh receipt
/// and refuses never-sliced tools with [`RegistryError::NotSliced`],
/// deny-listed with `DenyListed`, on-demand-unopened with
/// `SchemaNotOpened`, lens-disabled via the HELD lens (no caller lens
/// param), tier/object/write-intent (incl. canonical containment) with
/// `TierDenied`/`ContainmentDenied`, and only then delegates to the
/// executor. Re-slicing is last-wins per tool.
#[derive(Debug)]
pub struct Registry {
    index: HashMap<String, IndexEntry>,
    schemas: HashMap<String, serde_json::Value>,
    opened: HashSet<String>,
    ondemand: HashSet<String>,
    denied: HashSet<String>,
    allowed: HashSet<String>,
    covered: HashSet<String>,
    generation: u64,
    active_lens: crate::mcp::lens::ToolLens,
    lens_seq: u64,
    lens_audit: Vec<LensAuditEntry>,
    root: PathBuf,
    object_rules: Vec<crate::mcp::tiers::ObjectRule>,
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            index: HashMap::new(),
            schemas: HashMap::new(),
            opened: HashSet::new(),
            ondemand: HashSet::new(),
            denied: HashSet::new(),
            allowed: HashSet::new(),
            covered: HashSet::new(),
            generation: 0,
            active_lens: crate::mcp::lens::ToolLens::default(),
            lens_seq: 0,
            lens_audit: Vec::new(),
            root: PathBuf::new(),
            object_rules: Vec::new(),
        }
    }
}

fn default_root() -> PathBuf {
    std::env::current_dir()
        .ok()
        .and_then(|cwd| cwd.canonicalize().ok())
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

fn build_from_entries(
    entries: Vec<ManifestEntry>,
    generation: u64,
) -> Result<Registry, RegistryError> {
    if entries.len() > MAX_TOOLS {
        return Err(RegistryError::CatalogFull {
            found: entries.len(),
        });
    }
    let mut reg = Registry {
        generation,
        root: default_root(),
        ..Registry::default()
    };
    // `..Default::default()` reset generation to 0; restore.
    reg.generation = generation;
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

impl Registry {
    /// Load a registry from a manifest JSON array of [`ManifestEntry`].
    /// Enforces name grammar, description bound, per-tool schema bound,
    /// TOTAL schema-store bound, catalog ceiling, and the index overflow
    /// cap. Any breach fails the WHOLE load typed (never partial-allow).
    /// Fresh registries cover NOTHING (closed-by-default): every tool
    /// refuses with `NotSliced` until [`Registry::slice`] runs. Generation
    /// starts at 1.
    pub fn load_manifest(json: &str) -> Result<Self, RegistryError> {
        let entries: Vec<ManifestEntry> =
            serde_json::from_str(json).map_err(|e| RegistryError::ManifestParse {
                detail: e.to_string(),
            })?;
        build_from_entries(entries, 1)
    }

    /// Reload the catalog in place. Parses and validates `json` exactly
    /// like [`Registry::load_manifest`], then swaps the catalog, bumps the
    /// generation, and clears ALL slice-bound state (`covered`, `allowed`,
    /// `ondemand`, `denied`, `opened`). Old [`SliceReceipt`]s and
    /// [`CatalogHandle`]s invalidate typed (`StaleHandle`). The held lens,
    /// its sequence/audit, the bound root, and object rules survive reload
    /// (they are gateway posture, not catalog content).
    pub fn reload(&mut self, json: &str) -> Result<(), RegistryError> {
        let entries: Vec<ManifestEntry> =
            serde_json::from_str(json).map_err(|e| RegistryError::ManifestParse {
                detail: e.to_string(),
            })?;
        let fresh = build_from_entries(entries, self.generation + 1)?;
        self.index = fresh.index;
        self.schemas = fresh.schemas;
        self.generation = fresh.generation;
        self.opened.clear();
        self.ondemand.clear();
        self.denied.clear();
        self.allowed.clear();
        self.covered.clear();
        // NOTE: root / lens / object_rules intentionally preserved.
        Ok(())
    }

    /// Current catalog generation. Starts at 1; [`Registry::reload`]
    /// increments. Receipts/handles carrying any other generation are
    /// stale typed.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The registry-held active lens. Read-only: mutate only via
    /// [`Registry::rotate_lens`] (audited). There is deliberately NO setter
    /// that skips validation/audit, and [`Registry::invoke`] takes no lens
    /// argument, so a forged caller lens cannot reach the seam.
    pub fn active_lens(&self) -> &crate::mcp::lens::ToolLens {
        &self.active_lens
    }

    pub fn lens_seq(&self) -> u64 {
        self.lens_seq
    }

    pub fn lens_audit(&self) -> &[LensAuditEntry] {
        &self.lens_audit
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Bind the workspace root for canonical containment. Refuses symlinks
    /// typed (the engine never traverses through one) and canonicalizes
    /// the stored root so `starts_with` is component-wise, never
    /// substring-only. Missing directories are created (then canonicalized)
    /// via `crate::worktree::canonicalize_or_create`.
    pub fn bind_root(&mut self, root: &Path) -> Result<(), RegistryError> {
        if let Ok(md) = std::fs::symlink_metadata(root) {
            if md.file_type().is_symlink() {
                return Err(RegistryError::ContainmentDenied {
                    tool: "<bind>".into(),
                    target: root.display().to_string(),
                    reason: "symlink refused: root must be a real directory".into(),
                });
            }
        }
        // Refuse a symlinked ancestor chain component too (best-effort:
        // walk up until an existing ancestor; any symlink on the way denies).
        let mut probe: Option<&Path> = Some(root);
        while let Some(p) = probe {
            match std::fs::symlink_metadata(p) {
                Ok(md) if md.file_type().is_symlink() => {
                    return Err(RegistryError::ContainmentDenied {
                        tool: "<bind>".into(),
                        target: root.display().to_string(),
                        reason: format!("symlink refused on ancestor {}", p.display()),
                    });
                }
                Ok(_) => break,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    probe = p.parent();
                    continue;
                }
                Err(e) => {
                    return Err(RegistryError::Denied {
                        reason: format!("bind_root metadata: {e}"),
                    });
                }
            }
        }
        let canonical =
            crate::worktree::canonicalize_or_create(root).map_err(|e| RegistryError::Denied {
                reason: format!("bind_root: {e}"),
            })?;
        self.root = canonical;
        Ok(())
    }

    /// Install per-object policy rules refined WITHIN the tier (see
    /// `crate::mcp::tiers::evaluate_object`). Empty (default) means ambient:
    /// Read allows, Write/Exec pass the seam here (approval lives in the P02
    /// ACP gate); non-empty means the seam enforces the rules and maps any
    /// refusal to [`RegistryError::TierDenied`].
    pub fn set_object_rules(&mut self, rules: Vec<crate::mcp::tiers::ObjectRule>) {
        self.object_rules = rules;
    }

    pub fn object_rules(&self) -> &[crate::mcp::tiers::ObjectRule] {
        &self.object_rules
    }

    /// Rotate the registry-held active lens. Validates first: an invalid
    /// lens refuses typed (`Denied`) and changes NOTHING (fail-closed, no
    /// audit entry for a refused rotation). A valid rotation sets the lens,
    /// bumps `lens_seq`, and appends a [`LensAuditEntry`].
    pub fn rotate_lens(
        &mut self,
        lens: crate::mcp::lens::ToolLens,
        note: String,
    ) -> Result<u64, RegistryError> {
        if let Err(e) = lens.validate() {
            return Err(RegistryError::Denied {
                reason: format!("invalid lens refused: {e}"),
            });
        }
        self.active_lens = lens.clone();
        self.lens_seq += 1;
        self.lens_audit.push(LensAuditEntry {
            seq: self.lens_seq,
            generation: self.generation,
            disabled_keys: lens.disabled_keys.clone(),
            disabled_tags: lens.disabled_tags.clone(),
            version_req: lens.version_req.clone(),
            note,
        });
        Ok(self.lens_seq)
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
    /// tools are recorded in the deny set (the seam refuses them with
    /// `DenyListed`); Allow/OnDemand are recorded in `allowed`/`ondemand`
    /// plus the `covered` set. EVERY sliced tool (all three modes) joins
    /// `covered`, so a fresh registry (empty `covered`) invokes NOTHING
    /// (`NotSliced`) — closed-by-default. Re-slicing is last-wins per tool.
    /// Slicing a tool REVOKES its prior `opened` state (fail-closed): an
    /// OnDemand tool sliced to Deny then back to OnDemand demands a fresh
    /// `tool_open` — a stale open never survives a mode change. Only the
    /// sliced tools lose `opened` (unrelated tools keep theirs).
    /// Returns a generation-bound [`SliceReceipt`] for
    /// [`Registry::invoke`]/[`Registry::tool_open`]. The receipt snapshot
    /// is authoritative together with the live slice (see `SliceReceipt`):
    /// older receipts do not widen to newly sliced tools or modes.
    pub fn slice(
        &mut self,
        role_tools: &[(String, SliceMode)],
    ) -> Result<SliceReceipt, RegistryError> {
        let mut rows: Vec<IndexEntry> = Vec::new();
        for (name, mode) in role_tools {
            let base = self
                .index
                .get(name)
                .ok_or_else(|| RegistryError::UnknownTool { tool: name.clone() })?;
            // Fail-closed: any re-slice revokes the prior open for this tool.
            // A Deny→OnDemand (or Allow→OnDemand, OnDemand→OnDemand) without
            // a fresh `tool_open` must NOT invoke `Ok` via a stale `opened`.
            self.opened.remove(name);
            match mode {
                SliceMode::Deny => {
                    self.ondemand.remove(name);
                    self.allowed.remove(name);
                    self.denied.insert(name.clone());
                    self.covered.insert(name.clone());
                }
                SliceMode::Allow => {
                    self.ondemand.remove(name);
                    self.denied.remove(name);
                    self.allowed.insert(name.clone());
                    self.covered.insert(name.clone());
                    rows.push(IndexEntry {
                        on_demand: false,
                        ..base.clone()
                    });
                }
                SliceMode::OnDemand => {
                    self.denied.remove(name);
                    self.allowed.remove(name);
                    self.ondemand.insert(name.clone());
                    self.covered.insert(name.clone());
                    rows.push(IndexEntry {
                        on_demand: true,
                        ..base.clone()
                    });
                }
            }
        }
        rows.sort_by(|a, b| a.name.cmp(&b.name));
        let mut covered: Vec<String> = self.covered.iter().cloned().collect();
        covered.sort();
        Ok(SliceReceipt {
            generation: self.generation,
            covered,
            rows,
        })
    }

    /// Mint a catalog-bound handle for [`Registry::submit_bundle`].
    /// Generation-bound; reloads invalidate old handles typed.
    pub fn catalog_handle(&self) -> CatalogHandle {
        CatalogHandle {
            generation: self.generation,
        }
    }

    /// Lazy on-demand full-schema load. Demands a fresh [`SliceReceipt`]:
    /// stale generations refuse with `StaleHandle`; tools never sliced in
    /// this generation refuse with `NotSliced` (closed-by-default, even for
    /// `tool_open`). The receipt snapshot is bound: `receipt.covered` must
    /// contain the tool (an older receipt minted before the tool was sliced
    /// refuses `NotSliced` even when the live `covered` now has it).
    /// Deny-listed tools MAY be opened (open is not an un-deny — the seam
    /// still refuses them); the index stays bounded. Unknown tools deny typed.
    pub fn tool_open(
        &mut self,
        tool: &str,
        receipt: &SliceReceipt,
    ) -> Result<serde_json::Value, RegistryError> {
        if !self.index.contains_key(tool) {
            return Err(RegistryError::UnknownTool { tool: tool.into() });
        }
        if receipt.generation != self.generation {
            return Err(RegistryError::StaleHandle {
                expected: self.generation,
                found: receipt.generation,
            });
        }
        // Receipt binding (F3): the receipt must have covered the tool at
        // mint time — a pre-slice receipt never widens to a later slice.
        if !receipt.covered.contains(&tool.to_string()) {
            return Err(RegistryError::NotSliced {
                tool: tool.into(),
                generation: self.generation,
            });
        }
        if !self.covered.contains(tool) {
            return Err(RegistryError::NotSliced {
                tool: tool.into(),
                generation: self.generation,
            });
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

    /// Canonical containment gate (V03-B, authoritative — never
    /// substring-only). `target` is resolved against the bound root:
    /// relative targets join the root, absolute targets resolve from `/`;
    /// the walk is symlink-aware PRE-fold (each `Normal` component is
    /// checked via `symlink_metadata` as it is pushed, BEFORE a later
    /// `..` can lexically erase it). The final absolute path MUST be
    /// `starts_with(root)` (component-wise, so `/root-evil` does NOT match
    /// `/root`). Any symlink component at or below the root refuses typed
    /// — including `link/../evil`, `a/../link/../evil2`, and absolute
    /// `<root>/link/../evil3` (the `link` is seen before `..` folds it
    /// away; the real FS would resolve `link` before `..`, so lexical
    /// `root/evil` ≠ actual target). An empty/missing root fails closed.
    /// `None`/empty targets pass (nothing to contain).
    ///
    /// Residual (documented, `openat2` deferred per darkharvest F3
    /// clean-room verdict): ancestor components ABOVE the root are not
    /// re-vetted on every call (bind-time only), and there is an inherent
    /// check-then-use window between `check_containment` and executor use.
    /// The lexical-fold hole above IS closed (walk-then-fold, never
    /// fold-then-walk).
    pub fn check_containment(&self, tool: &str, target: Option<&str>) -> Result<(), RegistryError> {
        let Some(t) = target else { return Ok(()) };
        if t.is_empty() {
            return Ok(());
        }
        if self.root.as_os_str().is_empty() {
            return Err(RegistryError::ContainmentDenied {
                tool: tool.into(),
                target: t.into(),
                reason: "no bound root: bind_root first (fail-closed)".into(),
            });
        }
        use std::path::Component;
        let candidate = Path::new(t);
        // Symlink-aware walk PRE-fold: check each pushed component before a
        // later `..` can erase it (closes F1 lexical-fold bypass). Relative
        // targets walk from the bound root; absolute from `/`.
        let mut logical: PathBuf = if candidate.is_absolute() {
            PathBuf::from("/")
        } else {
            self.root.clone()
        };
        for comp in candidate.components() {
            match comp {
                Component::Prefix(_) | Component::RootDir => continue,
                Component::CurDir => continue,
                Component::ParentDir => {
                    logical.pop();
                    continue;
                }
                Component::Normal(c) => {
                    logical.push(c);
                    // Only vetted at/below the root (ancestors above root
                    // are bind-time only — documented TOCTOU, openat2
                    // deferred). The root itself is excluded (bind vetted).
                    if logical.starts_with(&self.root) && logical != self.root {
                        if let Ok(md) = std::fs::symlink_metadata(&logical) {
                            if md.file_type().is_symlink() {
                                return Err(RegistryError::ContainmentDenied {
                                    tool: tool.into(),
                                    target: t.into(),
                                    reason: format!("symlink refused at {}", logical.display()),
                                });
                            }
                        }
                    }
                }
            }
        }
        if !logical.starts_with(&self.root) {
            return Err(RegistryError::ContainmentDenied {
                tool: tool.into(),
                target: t.into(),
                reason: format!("escapes bound root {}", self.root.display()),
            });
        }
        // Defense-in-depth: re-walk the final logical path for symlinks
        // that may have appeared as non-terminal prefixes (same check as
        // above, but over the folded path — covers direct `link/secret`
        // and races that materialized between push-time and now).
        let mut cur: Option<&Path> = Some(logical.as_path());
        while let Some(p) = cur {
            if p == self.root {
                break;
            }
            if let Ok(md) = std::fs::symlink_metadata(p) {
                if md.file_type().is_symlink() {
                    return Err(RegistryError::ContainmentDenied {
                        tool: tool.into(),
                        target: t.into(),
                        reason: format!("symlink refused at {}", p.display()),
                    });
                }
            }
            cur = p.parent();
            if cur.is_none() {
                break;
            }
        }
        Ok(())
    }

    /// Submit through a bundle against the REGISTRY catalog (A2
    /// catalog-bound, no caller catalog param). Demands a fresh
    /// [`CatalogHandle`]: stale generations refuse typed
    /// (`BundleError::StaleHandle`). Every bundle tool is re-validated
    /// against `self.index` on EVERY submit, so ghost tools refuse typed
    /// even with review disabled and no prior `validate` call. Lens gating
    /// rides the invoke seam downstream. When the gate requires review and
    /// `reviewed` is false, submission is a typed refusal.
    pub fn submit_bundle(
        &self,
        gate: &crate::mcp::bundles::BundleGate,
        bundle: &str,
        reviewed: bool,
        handle: &CatalogHandle,
    ) -> Result<(), crate::mcp::bundles::BundleError> {
        if handle.generation != self.generation {
            return Err(crate::mcp::bundles::BundleError::StaleHandle {
                expected: self.generation,
                found: handle.generation,
            });
        }
        let declared = gate.bundles.keys().cloned().collect::<Vec<_>>().join(",");
        let b = gate.bundles.get(bundle).ok_or_else(|| {
            crate::mcp::bundles::BundleError::UnknownBundle {
                bundle: bundle.into(),
                declared,
            }
        })?;
        for t in &b.tools {
            if !self.index.contains_key(t) {
                return Err(crate::mcp::bundles::BundleError::UnknownTool {
                    bundle: b.name.clone(),
                    tool: t.clone(),
                });
            }
        }
        if gate.review_required && !reviewed {
            return Err(crate::mcp::bundles::BundleError::ReviewRequired {
                bundle: bundle.into(),
            });
        }
        Ok(())
    }

    /// Write-intent detection: the write-intent contract labels a call
    /// write-class when the tool tier is Write/Exec (via
    /// [`crate::mcp::tiers::tier_of`]) OR the target looks destructive via
    /// `crate::acp::is_destructive_intent` OR the target escapes the
    /// workspace (absolute path or `..` — checked for EVERY tier, since a
    /// read-class tool pointed outside the workspace is exfiltration-shaped
    /// regardless of tier; QA-R1) OR the argument text carries mutate
    /// verbs. Read-class tools with benign relative targets return false.
    /// NOTE: this is a HEURISTIC. The authoritative path gate on the seam is
    /// [`Registry::check_containment`] (canonical, component-wise); this
    /// function alone never authorizes a path.
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

    fn check_tier_stage(&self, tool: &str, args: &serde_json::Value) -> Result<(), RegistryError> {
        use crate::mcp::tiers::{tier_of, ApprovalTier};
        let target = args
            .get("path")
            .or_else(|| args.get("target"))
            .or_else(|| args.get("object"))
            .and_then(|v| v.as_str());
        // 1. Canonical containment first (authoritative path gate).
        self.check_containment(tool, target)?;
        // 2. Read-tier write-intent escalation: a Read-class tool carrying
        // write intent (mutate verbs, escape-shaped target, destructive
        // intent) is refused — honest Write/Exec tools carry write intent
        // by nature and pass here.
        let argv = args.to_string();
        if Self::detect_write_intent(tool, target, &argv)
            && matches!(tier_of(tool), ApprovalTier::Read)
        {
            return Err(RegistryError::TierDenied {
                tool: tool.into(),
                reason: "read-tier tool with write intent (mutate verbs or escape-shaped target)"
                    .into(),
            });
        }
        // 3. Object policies, only when configured. Empty (default) is
        // ambient so honest Write/Exec pass; non-empty enforces and maps
        // any refusal to TierDenied (tier holds, never weakened).
        if !self.object_rules.is_empty() {
            let object = target.unwrap_or("");
            if let Err(e) = crate::mcp::tiers::evaluate_object(tool, object, &self.object_rules) {
                return Err(RegistryError::TierDenied {
                    tool: tool.into(),
                    reason: e.to_string(),
                });
            }
        }
        Ok(())
    }

    /// THE single egress seam. Every tool call crosses here, in fail-closed
    /// order: unknown → coverage (stale-handle, then receipt-bound
    /// not-sliced, then live not-sliced, then receipt-mode desync) →
    /// deny-listed → on-demand-unopened → lens-disabled (HELD lens only) →
    /// tier/object/write-intent (incl. canonical containment) → executor.
    /// No caller can bypass any check: the executor is a parameter (tests
    /// inject fakes; the registry never owns a subprocess/socket), the lens
    /// is held (no param to forge), the catalog is held (no param to forge),
    /// and coverage demands a [`SliceReceipt`] bound to the live slice.
    /// Executor failures map to `Denied` with the SAME redaction guard as
    /// `Ok` (F2: `Err` strings are scanned; secret-shaped errors fail
    /// closed as `Denied` without echoing raw material); executor `Ok`
    /// outputs carrying secret-shaped material fail closed with `Denied`
    /// (redaction guard — the raw secret never leaves as `Ok`).
    pub fn invoke<E: ToolExecutor>(
        &self,
        executor: &E,
        tool: &str,
        args: serde_json::Value,
        receipt: &SliceReceipt,
    ) -> Result<serde_json::Value, RegistryError> {
        // 1. Unknown (no manifest entry — fail-closed to deny).
        let entry = self
            .index
            .get(tool)
            .ok_or_else(|| RegistryError::UnknownTool { tool: tool.into() })?;
        // 2. Coverage: stale-handle first, then receipt-bound
        // closed-by-default, then live closed-by-default, then mode binding.
        if receipt.generation != self.generation {
            return Err(RegistryError::StaleHandle {
                expected: self.generation,
                found: receipt.generation,
            });
        }
        // F3: receipt snapshot is authoritative — an older receipt minted
        // before this tool was sliced never widens to it.
        if !receipt.covered.contains(&tool.to_string()) {
            return Err(RegistryError::NotSliced {
                tool: tool.into(),
                generation: self.generation,
            });
        }
        if !self.covered.contains(tool) {
            return Err(RegistryError::NotSliced {
                tool: tool.into(),
                generation: self.generation,
            });
        }
        // 3. Deny-listed (live wins — even a fresh Allow receipt cannot
        // un-deny; a stale Allow receipt after a Deny re-slice also lands
        // here as DenyListed, fail-closed).
        if self.denied.contains(tool) {
            return Err(RegistryError::DenyListed { tool: tool.into() });
        }
        // F3 mode binding: receipt rows must match the live mode. Deny has
        // no row and never invokes `Ok`; an old Deny receipt (no row) after
        // an Allow re-slice, or an old Allow receipt after an OnDemand
        // re-slice (and vice versa), refuses as desync — never `Ok`.
        let receipt_allow = receipt.rows.iter().any(|r| r.name == tool && !r.on_demand);
        let receipt_ondemand = receipt.rows.iter().any(|r| r.name == tool && r.on_demand);
        if self.allowed.contains(tool) {
            if !receipt_allow {
                return Err(RegistryError::Denied {
                    reason: format!(
                        "stale receipt: live Allow for {tool} but receipt lacks Allow row (re-slice narrowed it)"
                    ),
                });
            }
        } else if self.ondemand.contains(tool) {
            if !receipt_ondemand {
                return Err(RegistryError::Denied {
                    reason: format!(
                        "stale receipt: live OnDemand for {tool} but receipt lacks OnDemand row (re-slice narrowed it)"
                    ),
                });
            }
        } else {
            // Covered but neither Allow nor OnDemand (and not Denied above)
            // — inconsistent state, fail closed.
            return Err(RegistryError::NotSliced {
                tool: tool.into(),
                generation: self.generation,
            });
        }
        // 4. On-demand unopened (stale `opened` never survives `slice` —
        // slicing revokes it, so Deny→OnDemand without a fresh `tool_open`
        // lands here as `SchemaNotOpened`).
        if self.ondemand.contains(tool) && !self.opened.contains(tool) {
            return Err(RegistryError::SchemaNotOpened { tool: tool.into() });
        }
        // 5. Lens (registry-held active lens only).
        self.active_lens.check_callable(entry)?;
        // 6. Tier/object/write-intent (incl. canonical containment).
        self.check_tier_stage(tool, &args)?;
        // 7. Executor (F2: BOTH paths redacted — `Err` strings are scanned
        // like `Ok` outputs; a hostile executor leaking via `Err` fails
        // closed without echoing raw material).
        let out = executor.execute(tool, args).map_err(|reason| {
            if crate::mcp::fixtures::contains_secret_material(&reason) {
                RegistryError::Denied {
                    reason: "output redacted: secret material blocked".into(),
                }
            } else {
                RegistryError::Denied { reason }
            }
        })?;
        // Redaction guard: secret-shaped outputs never leave as Ok.
        if crate::mcp::fixtures::contains_secret_material(&out.to_string()) {
            return Err(RegistryError::Denied {
                reason: "output redacted: secret material blocked".into(),
            });
        }
        Ok(out)
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

    fn sliced(reg: &mut Registry, modes: &[(&str, SliceMode)]) -> SliceReceipt {
        let v: Vec<(String, SliceMode)> = modes.iter().map(|(n, m)| ((*n).into(), *m)).collect();
        reg.slice(&v).unwrap()
    }

    #[test]
    fn loads_from_manifest_with_bounds() {
        let reg = Registry::load_manifest(&manifest_two()).unwrap();
        assert_eq!(reg.len(), 2);
        assert!(reg.index_bytes() > 0);
        assert!(reg.index_bytes() <= MAX_INDEX_BYTES);
        assert_eq!(reg.generation(), 1);
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
    fn fresh_registry_invokes_nothing_closed_by_default() {
        // V03-C: fresh registry (no slice) invokes NOTHING Ok — even with a
        // receipt minted before any slice (empty coverage) or no coverage.
        let mut reg = Registry::load_manifest(&manifest_two()).unwrap();
        let receipt = sliced(&mut reg, &[]);
        // Empty slice covers nothing: both tools refuse NotSliced.
        assert!(matches!(
            reg.invoke(&OkExec, "read", json!({}), &receipt)
                .unwrap_err(),
            RegistryError::NotSliced { .. }
        ));
        assert!(matches!(
            reg.invoke(&OkExec, "write", json!({}), &receipt)
                .unwrap_err(),
            RegistryError::NotSliced { .. }
        ));
        // tool_open is equally closed.
        assert!(matches!(
            reg.tool_open("read", &receipt).unwrap_err(),
            RegistryError::NotSliced { .. }
        ));
    }

    #[test]
    fn slice_allow_ondemand_deny_and_unknown_deny() {
        let mut reg = Registry::load_manifest(&manifest_two()).unwrap();
        let receipt = sliced(
            &mut reg,
            &[("read", SliceMode::Allow), ("write", SliceMode::OnDemand)],
        );
        assert_eq!(receipt.rows().len(), 2);
        assert!(!receipt.rows()[0].on_demand);
        // Deny excludes from rows but joins covered (so seam reports
        // DenyListed, not NotSliced — coverage passes, deny stage fires).
        let receipt2 = reg.slice(&[("read".into(), SliceMode::Deny)]).unwrap();
        assert!(receipt2.rows().is_empty() || receipt2.rows().iter().all(|r| r.name != "read"));
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
        let receipt = sliced(&mut reg, &[("read", SliceMode::Allow)]);
        let before = reg.index_bytes();
        let schema = reg.tool_open("read", &receipt).unwrap();
        assert_eq!(schema, json!({"type": "object"}));
        assert!(reg.is_opened("read"));
        assert!(!reg.is_opened("write"));
        // Opening a schema does not grow the bounded index.
        assert_eq!(reg.index_bytes(), before);
        assert!(matches!(
            reg.tool_open("ghost", &receipt).unwrap_err(),
            RegistryError::UnknownTool { .. }
        ));
    }

    #[test]
    fn unknown_invoke_denies_before_executor() {
        let mut reg = Registry::load_manifest(&manifest_two()).unwrap();
        let receipt = sliced(&mut reg, &[("read", SliceMode::Allow)]);
        let err = reg
            .invoke(&OkExec, "ghost", json!({}), &receipt)
            .unwrap_err();
        assert!(matches!(err, RegistryError::UnknownTool { .. }));
        let out = reg.invoke(&OkExec, "read", json!({}), &receipt).unwrap();
        assert_eq!(out, json!({"tool": "read"}));
    }

    #[test]
    fn ondemand_bypass_is_closed_at_the_seam() {
        let mut reg = Registry::load_manifest(&manifest_two()).unwrap();
        let receipt = sliced(&mut reg, &[("write", SliceMode::OnDemand)]);
        assert!(matches!(
            reg.invoke(&OkExec, "write", json!({}), &receipt)
                .unwrap_err(),
            RegistryError::SchemaNotOpened { .. }
        ));
        reg.tool_open("write", &receipt).unwrap();
        let out = reg.invoke(&OkExec, "write", json!({}), &receipt).unwrap();
        assert_eq!(out, json!({"tool": "write"}));
        // Allow-mode tools never need the open.
        let mut reg = Registry::load_manifest(&manifest_two()).unwrap();
        let receipt = sliced(&mut reg, &[("read", SliceMode::Allow)]);
        assert!(reg.invoke(&OkExec, "read", json!({}), &receipt).is_ok());
    }

    #[test]
    fn deny_listed_is_refused_at_the_seam_typed() {
        let mut reg = Registry::load_manifest(&manifest_two()).unwrap();
        let receipt = sliced(&mut reg, &[("write", SliceMode::Deny)]);
        // Covered (so NotSliced does NOT fire) but deny-listed.
        assert!(matches!(
            reg.invoke(&OkExec, "write", json!({}), &receipt)
                .unwrap_err(),
            RegistryError::DenyListed { .. }
        ));
        // Deny holds even after tool_open (open is not an un-deny).
        reg.tool_open("write", &receipt).unwrap();
        assert!(matches!(
            reg.invoke(&OkExec, "write", json!({}), &receipt)
                .unwrap_err(),
            RegistryError::DenyListed { .. }
        ));
        // Non-denied but unsliced tools still refuse NotSliced (closed).
        assert!(matches!(
            reg.invoke(&OkExec, "read", json!({}), &receipt)
                .unwrap_err(),
            RegistryError::NotSliced { .. }
        ));
    }

    #[test]
    fn reslice_is_last_wins_no_row_seam_desync() {
        let mut reg = Registry::load_manifest(&manifest_two()).unwrap();
        let r1 = sliced(&mut reg, &[("write", SliceMode::OnDemand)]);
        assert!(matches!(
            reg.invoke(&OkExec, "write", json!({}), &r1).unwrap_err(),
            RegistryError::SchemaNotOpened { .. }
        ));
        let r2 = sliced(&mut reg, &[("write", SliceMode::Allow)]);
        assert!(reg.invoke(&OkExec, "write", json!({}), &r2).is_ok());
        // And Deny-after-Allow binds the deny (last wins both ways).
        let r3 = sliced(&mut reg, &[("write", SliceMode::Deny)]);
        assert!(matches!(
            reg.invoke(&OkExec, "write", json!({}), &r3).unwrap_err(),
            RegistryError::DenyListed { .. }
        ));
        // Allow-after-Deny releases it again.
        let r4 = sliced(&mut reg, &[("write", SliceMode::Allow)]);
        assert!(reg.invoke(&OkExec, "write", json!({}), &r4).is_ok());
    }

    #[test]
    fn held_lens_disabled_is_uncallable_no_forged_param() {
        // V03-A1: invoke takes NO lens param (compile-time: forged lens
        // cannot be passed). The HELD lens gates the seam; rotating to a
        // permissive lens is the ONLY way to allow, and it audits.
        let mut reg = Registry::load_manifest(&manifest_two()).unwrap();
        reg.rotate_lens(
            crate::mcp::lens::ToolLens {
                disabled_keys: vec!["write".into()],
                ..Default::default()
            },
            "test restrictive".into(),
        )
        .unwrap();
        assert_eq!(reg.lens_audit().len(), 1);
        let receipt = sliced(&mut reg, &[("write", SliceMode::Allow)]);
        // Held restrictive lens denies even though slice allows.
        assert!(matches!(
            reg.invoke(&OkExec, "write", json!({}), &receipt)
                .unwrap_err(),
            RegistryError::LensDisabled { .. }
        ));
        // An attacker-held permissive lens value exists but has NO path to
        // the seam (no param) — the seam still denies.
        let _forged = crate::mcp::lens::ToolLens::default();
        assert!(matches!(
            reg.invoke(&OkExec, "write", json!({}), &receipt)
                .unwrap_err(),
            RegistryError::LensDisabled { .. }
        ));
        // Rotate to permissive (audited) re-allows.
        reg.rotate_lens(Default::default(), "re-allow".into())
            .unwrap();
        assert_eq!(reg.lens_audit().len(), 2);
        assert!(reg.invoke(&OkExec, "write", json!({}), &receipt).is_ok());
        // Invalid rotation refuses typed and changes nothing.
        let bad = crate::mcp::lens::ToolLens {
            disabled_keys: vec!["*read".into()],
            ..Default::default()
        };
        assert!(matches!(
            reg.rotate_lens(bad, "bad".into()).unwrap_err(),
            RegistryError::Denied { .. }
        ));
        assert_eq!(reg.lens_audit().len(), 2);
    }

    #[test]
    fn stale_receipt_and_handle_refuse_typed_after_reload() {
        // V03-C: reload invalidates old receipts/handles.
        let mut reg = Registry::load_manifest(&manifest_two()).unwrap();
        let receipt = sliced(
            &mut reg,
            &[("read", SliceMode::Allow), ("write", SliceMode::Allow)],
        );
        let handle = reg.catalog_handle();
        assert!(reg.invoke(&OkExec, "read", json!({}), &receipt).is_ok());
        reg.reload(&manifest_two()).unwrap();
        assert_eq!(reg.generation(), 2);
        // Old receipt stale at both invoke and tool_open.
        assert!(matches!(
            reg.invoke(&OkExec, "read", json!({}), &receipt)
                .unwrap_err(),
            RegistryError::StaleHandle { .. }
        ));
        assert!(matches!(
            reg.tool_open("read", &receipt).unwrap_err(),
            RegistryError::StaleHandle { .. }
        ));
        // Old catalog handle stale at submit.
        let gate = crate::mcp::bundles::BundleGate::parse_yaml(
            "review_required: false\nbundles:\n  b:\n    - read\n",
        )
        .unwrap();
        assert!(matches!(
            reg.submit_bundle(&gate, "b", false, &handle).unwrap_err(),
            crate::mcp::bundles::BundleError::StaleHandle { .. }
        ));
        // Fresh slice/handle work again (closed-by-default re-sliced).
        let receipt2 = sliced(&mut reg, &[("read", SliceMode::Allow)]);
        assert!(reg.invoke(&OkExec, "read", json!({}), &receipt2).is_ok());
        let handle2 = reg.catalog_handle();
        assert!(reg.submit_bundle(&gate, "b", false, &handle2).is_ok());
    }

    #[test]
    fn seam_order_unknown_coverage_deny_ondemand_lens_tier_executor() {
        // V03-D: unknown → coverage → deny → ondemand → lens →
        // tier/object/write-intent → executor. Each adjacent pair pinned:
        // the earlier stage wins.
        let mut reg = Registry::load_manifest(&manifest_two()).unwrap();
        // Unknown beats stale-coverage: ghost with a stale receipt is
        // UnknownTool, not StaleHandle.
        let mut reg2 = Registry::load_manifest(&manifest_two()).unwrap();
        let stale = sliced(&mut reg2, &[("read", SliceMode::Allow)]);
        reg2.reload(&manifest_two()).unwrap();
        assert!(matches!(
            reg2.invoke(&OkExec, "ghost", json!({}), &stale)
                .unwrap_err(),
            RegistryError::UnknownTool { .. }
        ));
        // Coverage beats deny-less Ok: unsliced read is NotSliced, not Ok.
        let r_empty = sliced(&mut reg, &[]);
        assert!(matches!(
            reg.invoke(&OkExec, "read", json!({}), &r_empty)
                .unwrap_err(),
            RegistryError::NotSliced { .. }
        ));
        // Deny beats ondemand-unopened AND lens: slice write Deny (last-wins
        // would clear ondemand, so force both via two slices where deny is
        // last — deny must win over lens-disabled too).
        let mut reg = Registry::load_manifest(&manifest_two()).unwrap();
        reg.rotate_lens(
            crate::mcp::lens::ToolLens {
                disabled_keys: vec!["write".into()],
                ..Default::default()
            },
            "disable write".into(),
        )
        .unwrap();
        let r = sliced(&mut reg, &[("write", SliceMode::Deny)]);
        // DenyListed wins over LensDisabled (deny is earlier).
        assert!(matches!(
            reg.invoke(&OkExec, "write", json!({}), &r).unwrap_err(),
            RegistryError::DenyListed { .. }
        ));
        // Ondemand beats lens: unopened ondemand + lens-disabled → opened
        // check first.
        let mut reg = Registry::load_manifest(&manifest_two()).unwrap();
        reg.rotate_lens(
            crate::mcp::lens::ToolLens {
                disabled_keys: vec!["write".into()],
                ..Default::default()
            },
            "disable".into(),
        )
        .unwrap();
        let r = sliced(&mut reg, &[("write", SliceMode::OnDemand)]);
        assert!(matches!(
            reg.invoke(&OkExec, "write", json!({}), &r).unwrap_err(),
            RegistryError::SchemaNotOpened { .. }
        ));
        // Lens beats tier: lens-disabled read with escape-shaped target
        // (which would also be TierDenied/ContainmentDenied) → LensDisabled.
        let mut reg = Registry::load_manifest(&manifest_two()).unwrap();
        let dir = tempfile::tempdir().unwrap();
        reg.bind_root(dir.path()).unwrap();
        reg.rotate_lens(
            crate::mcp::lens::ToolLens {
                disabled_keys: vec!["read".into()],
                ..Default::default()
            },
            "disable read".into(),
        )
        .unwrap();
        let r = sliced(&mut reg, &[("read", SliceMode::Allow)]);
        assert!(matches!(
            reg.invoke(&OkExec, "read", json!({"path": "../evil"}), &r)
                .unwrap_err(),
            RegistryError::LensDisabled { .. }
        ));
        // Tier (containment) beats executor: escape with permissive lens →
        // ContainmentDenied, even though executor would Ok.
        let mut reg = Registry::load_manifest(&manifest_two()).unwrap();
        let dir = tempfile::tempdir().unwrap();
        reg.bind_root(dir.path()).unwrap();
        let r = sliced(&mut reg, &[("read", SliceMode::Allow)]);
        assert!(matches!(
            reg.invoke(&OkExec, "read", json!({"path": "../evil"}), &r)
                .unwrap_err(),
            RegistryError::TierDenied { .. } | RegistryError::ContainmentDenied { .. }
        ));
    }

    #[test]
    fn containment_matrix_canonical_not_substring() {
        // V03-B matrix over a bound temp root.
        let mut reg = Registry::load_manifest(&manifest_two()).unwrap();
        let dir = tempfile::tempdir().unwrap();
        reg.bind_root(dir.path()).unwrap();
        let root = reg.root().to_path_buf();
        // Inside (relative) allows at containment level.
        assert!(reg.check_containment("read", Some("docs/guide.md")).is_ok());
        // Normalized-inside (a/./b/../c) allows — proves canonical fold,
        // not substring `..` refusal.
        assert!(reg.check_containment("read", Some("a/./b/../c")).is_ok());
        // Escape via .. denies.
        assert!(matches!(
            reg.check_containment("read", Some("../outside"))
                .unwrap_err(),
            RegistryError::ContainmentDenied { .. }
        ));
        // Absolute outside denies.
        assert!(matches!(
            reg.check_containment("read", Some("/etc/passwd"))
                .unwrap_err(),
            RegistryError::ContainmentDenied { .. }
        ));
        // Prefix-sibling denies: <root>-evil shares a substring prefix but
        // is NOT component-wise within root — proves starts_with, not
        // contains.
        let sibling = format!("{}-evil/file", root.display());
        assert!(matches!(
            reg.check_containment("read", Some(&sibling)).unwrap_err(),
            RegistryError::ContainmentDenied { .. }
        ));
        // Symlink inside denies (symlink refusal, not just prefix).
        #[cfg(unix)]
        {
            let real = dir.path().join("real.txt");
            std::fs::write(&real, "x").unwrap();
            let link = dir.path().join("link.txt");
            std::os::unix::fs::symlink(&real, &link).unwrap();
            assert!(matches!(
                reg.check_containment("read", Some("link.txt")).unwrap_err(),
                RegistryError::ContainmentDenied { .. }
            ));
        }
        // Symlinked root bind refuses.
        #[cfg(unix)]
        {
            let victim = tempfile::tempdir().unwrap();
            let linkroot = dir.path().join("linkroot");
            std::os::unix::fs::symlink(victim.path(), &linkroot).unwrap();
            let mut reg2 = Registry::load_manifest(&manifest_two()).unwrap();
            assert!(matches!(
                reg2.bind_root(&linkroot).unwrap_err(),
                RegistryError::ContainmentDenied { .. }
            ));
        }
    }

    #[test]
    fn containment_symlink_fold_hidden_traversal_denies() {
        // F1 (qa-b): lexical fold-then-walk hid `link` behind `/../`.
        // Walk-then-fold must deny all three hidden forms; direct and
        // benign folds keep their matrix verdicts.
        #[cfg(unix)]
        {
            let mut reg = Registry::load_manifest(&manifest_two()).unwrap();
            let dir = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            std::fs::write(outside.path().join("secret.txt"), "s").unwrap();
            reg.bind_root(dir.path()).unwrap();
            let link = dir.path().join("link");
            std::os::unix::fs::symlink(outside.path(), &link).unwrap();
            // Direct through symlink stays DENY.
            assert!(matches!(
                reg.check_containment("read", Some("link/secret.txt"))
                    .unwrap_err(),
                RegistryError::ContainmentDenied { .. }
            ));
            // Hidden via `..` folds lexically to `root/evil` but traverses
            // `link` on the real FS — must DENY (was ALLOW bypass).
            assert!(
                matches!(
                    reg.check_containment("read", Some("link/../evil"))
                        .unwrap_err(),
                    RegistryError::ContainmentDenied { .. }
                ),
                "link/../evil must deny"
            );
            assert!(
                matches!(
                    reg.check_containment("read", Some("a/../link/../evil2"))
                        .unwrap_err(),
                    RegistryError::ContainmentDenied { .. }
                ),
                "a/../link/../evil2 must deny"
            );
            let abs_hidden = format!("{}/link/../evil3", reg.root().display());
            assert!(
                matches!(
                    reg.check_containment("read", Some(&abs_hidden))
                        .unwrap_err(),
                    RegistryError::ContainmentDenied { .. }
                ),
                "abs <root>/link/../evil3 must deny"
            );
            // Benign folds keep ALLOW (no symlink).
            assert!(reg.check_containment("read", Some("a/./b/../c")).is_ok());
            assert!(reg.check_containment("read", Some("docs/guide.md")).is_ok());
        }
    }

    #[test]
    fn invoke_err_path_redacts_secret_same_as_ok() {
        // F2 (qa-b): executor `Err` strings were echoed verbatim into
        // `Denied.reason`. Both paths now redact; raw `sk-test` never
        // appears in the typed reason.
        struct LeakyErr;
        impl ToolExecutor for LeakyErr {
            fn execute(
                &self,
                _t: &str,
                _a: serde_json::Value,
            ) -> Result<serde_json::Value, String> {
                Err("boom sk-test-0123456789abcdef leaked via err".into())
            }
        }
        struct LeakyOk;
        impl ToolExecutor for LeakyOk {
            fn execute(
                &self,
                _t: &str,
                _a: serde_json::Value,
            ) -> Result<serde_json::Value, String> {
                Ok(json!({"leak": "sk-test-0123456789abcdef"}))
            }
        }
        let mut reg = Registry::load_manifest(&manifest_two()).unwrap();
        let receipt = sliced(&mut reg, &[("read", SliceMode::Allow)]);
        let err = reg
            .invoke(&LeakyErr, "read", json!({}), &receipt)
            .unwrap_err();
        assert!(matches!(err, RegistryError::Denied { .. }), "{err:?}");
        assert!(
            !err.to_string().contains("sk-test"),
            "Err-path leak: {err:?}"
        );
        let err = reg
            .invoke(&LeakyOk, "read", json!({}), &receipt)
            .unwrap_err();
        assert!(matches!(err, RegistryError::Denied { .. }), "{err:?}");
        assert!(
            !err.to_string().contains("sk-test"),
            "Ok-path leak: {err:?}"
        );
    }

    #[test]
    fn receipt_snapshot_bound_to_live_slice_no_desync() {
        // F3 (qa-b): receipt `covered`/`rows` bind the live slice.
        // Old read-only receipt never authorizes a later-sliced tool;
        // old Deny receipt never authorizes after Allow; stale `opened`
        // never survives Deny→OnDemand without a fresh `tool_open`.
        let mut reg = Registry::load_manifest(&manifest_two()).unwrap();
        let r_read = sliced(&mut reg, &[("read", SliceMode::Allow)]);
        assert_eq!(r_read.covered(), &["read".to_string()]);
        let _r2 = sliced(&mut reg, &[("write", SliceMode::Allow)]);
        // OLD read-only receipt invoking write must NOT be Ok.
        assert!(
            reg.invoke(&OkExec, "write", json!({}), &r_read).is_err(),
            "old read-only receipt must not invoke write Ok"
        );
        assert!(matches!(
            reg.invoke(&OkExec, "write", json!({}), &r_read)
                .unwrap_err(),
            RegistryError::NotSliced { .. } | RegistryError::Denied { .. }
        ));
        // tool_open desync: OLD receipt must NOT open a later-sliced tool.
        assert!(matches!(
            reg.tool_open("write", &r_read).unwrap_err(),
            RegistryError::NotSliced { .. }
        ));

        // Deny→Allow: OLD Deny receipt (no row) must NOT invoke Ok after
        // the live slice became Allow.
        let mut reg = Registry::load_manifest(&manifest_two()).unwrap();
        let r_deny = sliced(&mut reg, &[("write", SliceMode::Deny)]);
        assert!(r_deny.rows().is_empty());
        let _r_allow = sliced(&mut reg, &[("write", SliceMode::Allow)]);
        assert!(
            reg.invoke(&OkExec, "write", json!({}), &r_deny).is_err(),
            "old Deny receipt must not invoke Ok after Allow"
        );

        // OnDemand opened persistence: open, Deny, OnDemand (no reopen) must
        // demand a fresh `tool_open` (SchemaNotOpened), not Ok via stale
        // `opened`.
        let mut reg = Registry::load_manifest(&manifest_two()).unwrap();
        let r_od = sliced(&mut reg, &[("write", SliceMode::OnDemand)]);
        reg.tool_open("write", &r_od).unwrap();
        assert!(reg.is_opened("write"));
        let _ = sliced(&mut reg, &[("write", SliceMode::Deny)]);
        assert!(!reg.is_opened("write"), "slice to Deny must revoke opened");
        let r_od2 = sliced(&mut reg, &[("write", SliceMode::OnDemand)]);
        assert!(
            !reg.is_opened("write"),
            "re-slice to OnDemand must demand fresh open"
        );
        assert!(matches!(
            reg.invoke(&OkExec, "write", json!({}), &r_od2).unwrap_err(),
            RegistryError::SchemaNotOpened { .. }
        ));
        // Fresh open re-allows.
        reg.tool_open("write", &r_od2).unwrap();
        assert!(reg.invoke(&OkExec, "write", json!({}), &r_od2).is_ok());
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
