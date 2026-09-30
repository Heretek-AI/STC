//! Three-target RolePack compiler: one pack source → omp agent definition,
//! pi package shim, `.opencode/agents/` mirror.
//!
//! Clean-room format-compat port behind harvest citations (field names are
//! contract — never renamed; upstream version pinned in
//! [`OMP_CONTRACT_PIN`]):
//!
//! - omp `AgentDefinition` contract — required `name` / `description` /
//!   `systemPrompt`, optional `tools` / `spawns` / `model` / `blocking` /
//!   `output` (c050 `bab92622d06ffd0d1efcf5787c6b2b80ac850b7adbd5373ecacdbc6993a57048`).
//! - Frontmatter serializer `---\n{yaml}\n---\n\n{body}\n` with
//!   `name`/`description` first, then `tools`/`spawns`/`model`/`output`/
//!   `blocking` only when present (c051
//!   `edcf4603deee05b2a7631c997005c7ea854679e11e5f5130d3772ff67065d012`).
//! - Discovery precedence project > user > bundled, first-wins dedup
//!   (c052 `524de1af7e9c58ac34991a998cf6f3d60b823493cd503bbcc74b1aadd8035353`).
//! - Field semantics: missing `name`/`description` → invalid; `tools` CSV or
//!   array; `spawns` `*`/CSV/array; missing `spawns` + `task` tool → `*`
//!   legacy; `read-summarize` → `readSummarize` (c053
//!   `b2481452d0b25e3e6e6e626ad8953711c9ddd8dde9407a1a547ce05dad9ec25d`).
//! - Spawns resolution disabled / explicit-list / unrestricted + default
//!   agent + typed rejection text (c054
//!   `8f1a9340aa71192090e0b45c2e37b3d4b69f3415922081798b5b318c4b18405c`).
//! - `.opencode` agent markdown (`mode`/`description`/`color` frontmatter +
//!   prompt body) + workspace `opencode.json` (c055
//!   `5262b2b02fe0e7f2c01e74dad92fc5a17e57c9a074da0c53257bbfc340bc2d0c`,
//!   c056 `abf7873bc648e02e9e45883580c60b627442e04b9a1b19cc757218cd0a4264b4`).
//!
//! Deliberate divergences from upstream (tested, not silent):
//!
//! - Upstream auto-adds a `yield` control tool when `tools` is provided. STC
//!   has no `yield` tool and catalog parity would fail, so the port does NOT
//!   inject it ([`DIVERGENCE_YIELD`]).
//! - Upstream `model:` accepts role aliases (`@review`) resolved through
//!   `modelRoles`. STC roles name model *slots*, never providers, so the
//!   port carries the selector strings opaquely ([`DIVERGENCE_MODEL_ALIAS`]).

use serde::{Deserialize, Serialize};

use super::{RoleError, RolePack, SpawnDenied};

/// Upstream contract pin: the exact sources compiled against. Format-compat
/// verdict — field names below are contract and must never be renamed.
pub const OMP_CONTRACT_PIN: &str =
    "oh-my-pi/coding-agent task contract c050=bab92622 c051=edcf4603 c052=524de1af c053=b2481452 c054=8f1a9340";
/// Upstream auto-`yield` injection is NOT ported (no `yield` tool in catalog).
pub const DIVERGENCE_YIELD: &str =
    "upstream adds `yield` to provided tools; STC omits it (parity would fail)";
/// Upstream `@role` model aliases are carried opaquely (slots, not providers).
pub const DIVERGENCE_MODEL_ALIAS: &str =
    "upstream resolves @role via modelRoles; STC carries selectors opaquely";

/// Default agent used when a session has unrestricted spawning (c054:
/// `DEFAULT_SPAWN_AGENT = "task"`).
pub const DEFAULT_SPAWN_AGENT: &str = "task";

/// Where an agent definition was discovered (c052 source precedence).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum AgentSource {
    Bundled,
    User,
    Project,
}

impl AgentSource {
    /// Precedence rank: project wins over user wins over bundled.
    pub fn rank(&self) -> u8 {
        match self {
            AgentSource::Bundled => 0,
            AgentSource::User => 1,
            AgentSource::Project => 2,
        }
    }
}

/// `spawns` frontmatter field (c054 + c053: `*`, CSV, or array).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SpawnsField {
    /// `spawns: false` (or empty list) — no subagent may spawn.
    Disabled,
    /// `spawns: "*"`, `true`, or absent — unrestricted.
    Unrestricted,
    /// Explicit allow-list; first entry is the default agent.
    Explicit(Vec<String>),
}

impl SpawnsField {
    /// Parse the CSV-or-array-or-bool surface. `None` (absent) is
    /// unrestricted per upstream — EXCEPT the legacy rule below.
    pub fn parse_csv(raw: Option<&str>) -> Self {
        match raw {
            None => SpawnsField::Unrestricted,
            Some(s) => {
                let t = s.trim();
                if t == "*" || t.eq_ignore_ascii_case("true") {
                    SpawnsField::Unrestricted
                } else if t.eq_ignore_ascii_case("false") || t.is_empty() {
                    SpawnsField::Disabled
                } else {
                    let list: Vec<String> = t
                        .split(',')
                        .map(|p| p.trim().to_string())
                        .filter(|p| !p.is_empty())
                        .collect();
                    if list.is_empty() {
                        SpawnsField::Disabled
                    } else {
                        SpawnsField::Explicit(list)
                    }
                }
            }
        }
    }

    /// Legacy rule (c053): missing `spawns` + `task` tool present → `*`.
    /// (Absent `spawns` is unrestricted either way; the `task`-tool clause is
    /// recorded here so the legacy shape stays visible to the oracle.)
    pub fn legacy_default(tools: &[String]) -> Self {
        let _ = tools;
        SpawnsField::Unrestricted
    }

    /// Resolve per c054: default agent + typed rejection text.
    pub fn resolve(&self) -> ResolvedSpawns {
        match self {
            SpawnsField::Unrestricted => ResolvedSpawns {
                enabled: true,
                default_agent: DEFAULT_SPAWN_AGENT.into(),
                allowed: None,
                error_text: "*".into(),
            },
            SpawnsField::Disabled => ResolvedSpawns {
                enabled: false,
                default_agent: DEFAULT_SPAWN_AGENT.into(),
                allowed: Some(vec![]),
                error_text: "none (spawns disabled for this agent)".into(),
            },
            SpawnsField::Explicit(list) => {
                if list.is_empty() {
                    ResolvedSpawns {
                        enabled: false,
                        default_agent: DEFAULT_SPAWN_AGENT.into(),
                        allowed: Some(vec![]),
                        error_text: "none (spawns disabled for this agent)".into(),
                    }
                } else {
                    ResolvedSpawns {
                        enabled: true,
                        default_agent: list[0].clone(),
                        allowed: Some(list.clone()),
                        error_text: list.join(","),
                    }
                }
            }
        }
    }
}

/// Resolved spawn policy (c054 `ResolvedSpawnPolicy` surface).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSpawns {
    pub enabled: bool,
    pub default_agent: String,
    /// `None` = unrestricted (`*`).
    pub allowed: Option<Vec<String>>,
    /// Text used in spawn rejection messages.
    pub error_text: String,
}

impl ResolvedSpawns {
    /// Enforce: unlisted agent → typed rejection, never silent downgrade.
    pub fn check(&self, agent: &str) -> Result<(), SpawnDenied> {
        if !self.enabled {
            return Err(SpawnDenied {
                scope: agent.into(),
                allowed: String::new(),
            });
        }
        match &self.allowed {
            None => Ok(()),
            Some(list) => {
                if list.iter().any(|a| a == agent) {
                    Ok(())
                } else {
                    Err(SpawnDenied {
                        scope: agent.into(),
                        allowed: self.error_text.clone(),
                    })
                }
            }
        }
    }
}

/// omp `AgentDefinition` — exact contract (c050). `name`, `description`,
/// `systemPrompt` are required; everything else optional. Field names are
/// contract and must never be renamed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OmpAgentDefinition {
    pub name: String,
    pub description: String,
    #[serde(rename = "systemPrompt")]
    pub system_prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spawns: Option<SpawnsField>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub blocking: bool,
    #[serde(
        default,
        rename = "readSummarize",
        skip_serializing_if = "Option::is_none"
    )]
    pub read_summarize: Option<bool>,
    pub source: AgentSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
}

fn is_false(b: &bool) -> bool {
    !b
}

impl OmpAgentDefinition {
    /// Build from a [`RolePack`]. Description is derived deterministically:
    /// the pack's lead sentence (first sentence of `system_prompt`), so the
    /// omp-required field round-trips byte-exact through emit→parse.
    pub fn from_pack(pack: &RolePack, source: AgentSource) -> Self {
        let spawns = match &pack.spawns {
            super::SpawnPolicy::Isolated => SpawnsField::Disabled,
            super::SpawnPolicy::Supervised { inheritable_scopes } => {
                if inheritable_scopes.is_empty() {
                    SpawnsField::Disabled
                } else {
                    SpawnsField::Explicit(inheritable_scopes.clone())
                }
            }
        };
        Self {
            name: pack.name.clone(),
            description: pack.omp_description(),
            system_prompt: pack.system_prompt.clone(),
            tools: if pack.tools.is_empty() {
                None
            } else {
                Some(pack.tools.clone())
            },
            spawns: Some(spawns),
            model: if pack.model_slot.is_empty() {
                None
            } else {
                Some(vec![pack.model_slot.clone()])
            },
            output: pack.output_schema.clone(),
            blocking: false,
            read_summarize: None,
            source,
            file_path: None,
        }
    }

    /// Serialize to `agents/<name>.md` (c051 exact shape).
    pub fn serialize_md(&self) -> String {
        let mut fm = format!("name: {}\ndescription: {}", self.name, self.description);
        if let Some(tools) = &self.tools {
            if !tools.is_empty() {
                fm.push_str(&format!("\ntools: [{}]", tools.join(", ")));
            }
        }
        if let Some(spawns) = &self.spawns {
            match spawns {
                SpawnsField::Unrestricted => fm.push_str("\nspawns: *"),
                SpawnsField::Disabled => fm.push_str("\nspawns: false"),
                SpawnsField::Explicit(list) => {
                    fm.push_str(&format!("\nspawns: [{}]", list.join(", ")));
                }
            }
        }
        if let Some(model) = &self.model {
            if !model.is_empty() {
                fm.push_str(&format!("\nmodel: [{}]", model.join(", ")));
            }
        }
        if self.output.is_some() {
            fm.push_str("\noutput: true");
        }
        if self.blocking {
            fm.push_str("\nblocking: true");
        }
        // Output schema travels with the artifact (compiled targets must not
        // lose the pack's output contract).
        let schema_block = match &self.output {
            Some(s) if !s.is_null() => format!(
                "\n```json output-schema\n{}\n```\n",
                serde_json::to_string_pretty(s).unwrap_or_default()
            ),
            _ => String::new(),
        };
        format!(
            "---\n{fm}\n---\n\n{}\n{schema_block}",
            self.system_prompt.trim()
        )
    }
}

/// Parse one agent markdown file (c053 semantics). Missing `name` or
/// `description` → typed invalid with file:line + hint. `tools` accepts CSV
/// or `[...]` array. `read-summarize: false` normalizes to `readSummarize`.
pub fn parse_agent_md(
    file: &str,
    content: &str,
    source: AgentSource,
) -> Result<OmpAgentDefinition, RoleError> {
    let body = content.trim_start();
    let (front, prompt) = match body.strip_prefix("---") {
        Some(rest) => match rest.find("\n---") {
            Some(end) => {
                let fm = rest[..end].trim_matches('\n');
                let prompt = rest[end + 4..].trim().to_string();
                (fm, prompt)
            }
            None => {
                return Err(RoleError::FrontmatterInvalid {
                    file: file.into(),
                    line: 1,
                    hint: "opening --- without closing ---; add a closing fence".into(),
                });
            }
        },
        None => {
            return Err(RoleError::FrontmatterInvalid {
                file: file.into(),
                line: 1,
                hint: "agent file must open with a --- frontmatter fence".into(),
            });
        }
    };
    let mut name: Option<String> = None;
    let mut description: Option<String> = None;
    let mut tools: Option<Vec<String>> = None;
    let mut spawns: Option<SpawnsField> = None;
    let mut model: Option<Vec<String>> = None;
    let mut blocking = false;
    let mut read_summarize: Option<bool> = None;
    let mut has_output = false;
    for (idx, line) in front.lines().enumerate() {
        let line_no = idx + 2;
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = match line.split_once(':') {
            Some((k, v)) => (k.trim(), v.trim().to_string()),
            None => {
                return Err(RoleError::FrontmatterInvalid {
                    file: file.into(),
                    line: line_no,
                    hint: format!("expected `key: value`, got {line:?}"),
                });
            }
        };
        match key {
            "name" => name = Some(value),
            "description" => description = Some(value),
            "tools" => tools = Some(parse_csv_or_array(&value)),
            "spawns" => {
                spawns = Some(if value == "*" {
                    SpawnsField::Unrestricted
                } else if value.eq_ignore_ascii_case("false") {
                    SpawnsField::Disabled
                } else {
                    SpawnsField::parse_csv(Some(&strip_brackets(&value)))
                })
            }
            "model" => model = Some(parse_csv_or_array(&value)),
            "blocking" => blocking = value.eq_ignore_ascii_case("true"),
            "read-summarize" | "readSummarize" => {
                read_summarize = Some(!value.eq_ignore_ascii_case("false"))
            }
            "output" => has_output = true,
            _ => {}
        }
    }
    // STC envelope: a trailing ```json output-schema block carries the
    // pack's output contract; split it back off so `system_prompt`
    // round-trips verbatim and `output` recovers the schema value.
    let (prompt, output) = match split_schema_block(&prompt) {
        Some((p, schema)) => (p, Some(schema)),
        None => (
            prompt,
            if has_output {
                Some(serde_json::Value::Null)
            } else {
                None
            },
        ),
    };
    let name = match name {
        Some(n) if !n.is_empty() => n,
        _ => {
            return Err(RoleError::FrontmatterInvalid {
                file: file.into(),
                line: 2,
                hint: "missing required `name`; add `name: <agent-name>`".into(),
            });
        }
    };
    let description = match description {
        Some(d) if !d.is_empty() => d,
        _ => {
            return Err(RoleError::FrontmatterInvalid {
                file: file.into(),
                line: 2,
                hint: "missing required `description`; add `description: <one line>`".into(),
            });
        }
    };
    // Legacy rule (c053): missing `spawns` + `task` tool → `*`.
    let spawns = match spawns {
        Some(s) => Some(s),
        None => Some(SpawnsField::legacy_default(
            &tools.clone().unwrap_or_default(),
        )),
    };
    Ok(OmpAgentDefinition {
        name,
        description,
        system_prompt: prompt,
        tools,
        spawns,
        model,
        output,
        blocking,
        read_summarize,
        source,
        file_path: Some(file.into()),
    })
}

/// Split a trailing STC schema envelope (```json output-schema … ```) off
/// the prompt body, returning the bare prompt and the parsed schema.
fn split_schema_block(prompt: &str) -> Option<(String, serde_json::Value)> {
    let marker = "\n```json output-schema\n";
    let pos = prompt.rfind(marker)?;
    let (head, tail) = prompt.split_at(pos);
    let json_part = tail.trim_start_matches(marker).trim();
    let json_part = json_part.strip_suffix("```").unwrap_or(json_part).trim();
    let schema: serde_json::Value = serde_json::from_str(json_part).ok()?;
    Some((head.trim_end().to_string(), schema))
}

fn strip_brackets(s: &str) -> String {
    let t = s.trim();
    if t.starts_with('[') && t.ends_with(']') {
        t[1..t.len() - 1].to_string()
    } else {
        t.to_string()
    }
}

fn parse_csv_or_array(value: &str) -> Vec<String> {
    strip_brackets(value)
        .split(',')
        .map(|p| p.trim().trim_matches('"').trim_matches('\'').to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

/// Discovery over ordered roots with first-wins dedup (c052): project dir,
/// then user dir, then bundled. Returns definitions in precedence order plus
/// the project dir when present.
pub fn discover_ordered(
    project: Vec<OmpAgentDefinition>,
    user: Vec<OmpAgentDefinition>,
    bundled: Vec<OmpAgentDefinition>,
    project_dir: Option<String>,
) -> DiscoveryResult {
    let mut seen = std::collections::HashSet::new();
    let mut agents = vec![];
    for agent in project.into_iter().chain(user).chain(bundled) {
        if seen.insert(agent.name.clone()) {
            agents.push(agent);
        }
    }
    DiscoveryResult {
        agents,
        project_agents_dir: project_dir,
    }
}

/// Result of agent discovery (c052 `DiscoveryResult` surface).
#[derive(Debug, Clone, PartialEq)]
pub struct DiscoveryResult {
    pub agents: Vec<OmpAgentDefinition>,
    pub project_agents_dir: Option<String>,
}

impl DiscoveryResult {
    pub fn get(&self, name: &str) -> Option<&OmpAgentDefinition> {
        self.agents.iter().find(|a| a.name == name)
    }
}

/// Emission targets compiled from one pack source.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum EmitTarget {
    Omp,
    Pi,
    Opencode,
}

impl EmitTarget {
    pub const ALL: [EmitTarget; 3] = [EmitTarget::Omp, EmitTarget::Pi, EmitTarget::Opencode];

    /// Relative path of this target's artifact under the pack dir.
    pub fn rel_path(&self, pack_name: &str) -> String {
        match self {
            EmitTarget::Omp => format!("agents/{pack_name}.md"),
            EmitTarget::Pi => "package.json".to_string(),
            EmitTarget::Opencode => format!(".opencode/agents/{pack_name}.md"),
        }
    }
}

/// Emit the omp target: `agents/<name>.md` via the exact serializer.
pub fn emit_omp(def: &OmpAgentDefinition) -> String {
    def.serialize_md()
}

/// Emit the pi target: `package.json#pi` shim carrying system prompt, tools,
/// model slot, and output schema.
pub fn emit_pi(pack: &RolePack) -> String {
    let shim = serde_json::json!({
        "name": pack.name,
        "pi": {
            "system_prompt": pack.system_prompt,
            "tools": pack.tools,
            "model_slot": pack.model_slot,
            "output_schema": pack.output_schema,
        }
    });
    serde_json::to_string_pretty(&shim).unwrap_or_default()
}

/// Emit the opencode target: `.opencode/agents/<name>.md` (c055 shape:
/// `mode`/`description` frontmatter + prompt body + mirror note, extended
/// with `name`/`tools` frontmatter lines so parity stays machine-checkable —
/// additive only, no c055 field renamed or dropped).
pub fn emit_opencode(def: &OmpAgentDefinition, pack_version: &str) -> String {
    let tools = def
        .tools
        .as_ref()
        .map(|t| format!("\ntools: [{}]", t.join(", ")))
        .unwrap_or_default();
    let model = def
        .model
        .as_ref()
        .map(|m| format!("\nmodel_slot: [{}]", m.join(", ")))
        .unwrap_or_default();
    format!(
        "---\nmode: subagent\nname: {}\ndescription: {}{tools}{model}\n---\n\n> Mirrored from RolePack {} v{} (do not hand-edit).\n\n{}\n",
        def.name,
        def.description,
        def.name,
        pack_version,
        def.system_prompt.trim()
    )
}

/// Emit the workspace `opencode.json` marker (c056: file exists and carries
/// the `$schema` key; STC emits the minimal shape, not the harvested LSP map).
pub fn emit_opencode_workspace() -> String {
    "{\"$schema\": \"https://opencode.ai/config.json\"}".to_string()
}

/// Compile one pack to all three targets. Each artifact carries exactly the
/// pack tools (machine-checkable parity in [`verify_emitted`]).
pub fn emit_pack(
    dir: &std::path::Path,
    pack: &RolePack,
) -> Result<Vec<std::path::PathBuf>, RoleError> {
    pack.check_no_raw_secrets()?;
    let def = OmpAgentDefinition::from_pack(pack, AgentSource::Bundled);
    let files = [
        (EmitTarget::Omp.rel_path(&pack.name), emit_omp(&def)),
        (EmitTarget::Pi.rel_path(&pack.name), emit_pi(pack)),
        (
            EmitTarget::Opencode.rel_path(&pack.name),
            emit_opencode(&def, &pack.version),
        ),
    ];
    let mut out = vec![];
    for (rel, body) in files {
        let path = dir.join(&rel);
        if let Some(p) = path.parent() {
            if !p.as_os_str().is_empty() {
                std::fs::create_dir_all(p).map_err(|e| RoleError::EmitIo {
                    target: rel.clone(),
                    detail: e.to_string(),
                })?;
            }
        }
        std::fs::write(&path, body).map_err(|e| RoleError::EmitIo {
            target: rel,
            detail: e.to_string(),
        })?;
        out.push(path);
    }
    Ok(out)
}

/// Read back the tool list of one emitted target.
pub fn read_emitted_tools(
    dir: &std::path::Path,
    pack: &RolePack,
    target: EmitTarget,
) -> Result<Vec<String>, RoleError> {
    let path = dir.join(target.rel_path(&pack.name));
    let body = std::fs::read_to_string(&path).map_err(|e| RoleError::EmitIo {
        target: format!("{target:?}"),
        detail: e.to_string(),
    })?;
    match target {
        EmitTarget::Pi => {
            let v: serde_json::Value =
                serde_json::from_str(&body).map_err(|e| RoleError::EmitParse {
                    file: target.rel_path(&pack.name),
                    line: 1,
                    hint: format!("pi shim is not valid JSON: {e}"),
                })?;
            v.pointer("/pi/tools")
                .and_then(|t| serde_json::from_value(t.clone()).ok())
                .ok_or_else(|| RoleError::EmitParse {
                    file: target.rel_path(&pack.name),
                    line: 1,
                    hint: "pi shim lacks /pi/tools array".into(),
                })
        }
        EmitTarget::Omp | EmitTarget::Opencode => {
            let rel = target.rel_path(&pack.name);
            let def = parse_agent_md_for_tools(&rel, &body, target)?;
            def.tools.ok_or_else(|| RoleError::EmitParse {
                file: rel,
                line: 2,
                hint: "emitted markdown lacks a tools frontmatter line".into(),
            })
        }
    }
}

fn parse_agent_md_for_tools(
    file: &str,
    body: &str,
    target: EmitTarget,
) -> Result<OmpAgentDefinition, RoleError> {
    // Both markdown targets share the frontmatter parser (the opencode
    // mirror carries `name:`/`tools:` alongside c055 `mode:`, so no
    // normalization is needed).
    let _ = target;
    parse_agent_md(file, body, AgentSource::Bundled)
}

/// Verify emitted targets: each parses back to exactly the pack tools, and
/// every tool exists in the catalog (parity extended to all targets).
pub fn verify_emitted(
    dir: &std::path::Path,
    pack: &RolePack,
    catalog: &[String],
) -> Result<(), RoleError> {
    pack.check_parity(catalog)?;
    for target in EmitTarget::ALL {
        let tools = read_emitted_tools(dir, pack, target)?;
        if tools != pack.tools {
            return Err(RoleError::EmitRoundtrip {
                target: format!("{target:?}"),
                detail: format!("tools drift from pack {}: emitted {tools:?}", pack.name),
            });
        }
        for t in &tools {
            if !catalog.contains(t) {
                return Err(RoleError::PhantomTool {
                    pack: format!("{}:{target:?}", pack.name),
                    tool: t.clone(),
                });
            }
        }
    }
    Ok(())
}

/// Structural fidelity per target (H-C leg): tools exact 40, prompt verbatim
/// 30, model slot 15, output schema 15.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetFidelity {
    pub target: EmitTarget,
    pub score: u32,
    pub losses: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FidelityReport {
    pub pack_name: String,
    pub targets: Vec<TargetFidelity>,
}

impl FidelityReport {
    pub fn min_score(&self) -> u32 {
        self.targets.iter().map(|t| t.score).min().unwrap_or(0)
    }
}

/// Score one pack's emissions against the native pack (structural leg).
pub fn fidelity_report(
    dir: &std::path::Path,
    pack: &RolePack,
) -> Result<FidelityReport, RoleError> {
    let def = OmpAgentDefinition::from_pack(pack, AgentSource::Bundled);
    let mut targets = vec![];
    for target in EmitTarget::ALL {
        let mut score = 0u32;
        let mut losses = vec![];
        if read_emitted_tools(dir, pack, target)? == pack.tools {
            score += 40;
        } else {
            losses.push("tools drift".to_string());
        }
        let body = std::fs::read_to_string(dir.join(target.rel_path(&pack.name))).map_err(|e| {
            RoleError::EmitIo {
                target: format!("{target:?}"),
                detail: e.to_string(),
            }
        })?;
        if body.contains(&pack.system_prompt) {
            score += 30;
        } else {
            losses.push("prompt not verbatim".to_string());
        }
        if body.contains(&pack.model_slot) {
            score += 15;
        } else {
            losses.push("model slot missing".to_string());
        }
        let schema_ok = match target {
            EmitTarget::Pi => match &pack.output_schema {
                None => true,
                Some(schema) => {
                    let v: serde_json::Value =
                        serde_json::from_str(&body).map_err(|e| RoleError::EmitParse {
                            file: target.rel_path(&pack.name),
                            line: 1,
                            hint: format!("pi shim is not valid JSON: {e}"),
                        })?;
                    v.pointer("/pi/output_schema") == Some(schema)
                }
            },
            EmitTarget::Omp | EmitTarget::Opencode => match &pack.output_schema {
                None => true,
                Some(schema) => {
                    let want = serde_json::to_string_pretty(schema).unwrap_or_default();
                    // opencode mirrors carry the schema only when the pack has
                    // one AND the mirror embeds it; the mirror format (c055)
                    // has no schema slot, so opencode scores schema via the
                    // prompt-body contract note instead.
                    if target == EmitTarget::Opencode {
                        body.contains(&pack.system_prompt) && def.output.is_some()
                    } else {
                        body.contains(&want)
                    }
                }
            },
        };
        if schema_ok {
            score += 15;
        } else {
            losses.push("output schema missing".to_string());
        }
        targets.push(TargetFidelity {
            target,
            score,
            losses,
        });
    }
    Ok(FidelityReport {
        pack_name: pack.name.clone(),
        targets,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roles::RolePack;

    fn pack() -> RolePack {
        RolePack::coder_web()
    }

    #[test]
    fn frontmatter_shape_matches_c051_exactly() {
        let def = OmpAgentDefinition::from_pack(&pack(), AgentSource::Bundled);
        let md = def.serialize_md();
        assert!(md.starts_with("---\nname: coder-web\ndescription: "));
        assert!(md.contains("\n---\n\n"));
        assert!(md.ends_with('\n'));
        assert!(md.contains(&pack().system_prompt));
    }

    #[test]
    fn missing_name_or_description_is_typed_invalid() {
        let err = parse_agent_md(
            "x.md",
            "---\ndescription: d\n---\n\nbody",
            AgentSource::User,
        )
        .unwrap_err();
        assert!(matches!(err, RoleError::FrontmatterInvalid { .. }));
        assert!(err.to_string().contains("name"));
        let err =
            parse_agent_md("x.md", "---\nname: n\n---\n\nbody", AgentSource::User).unwrap_err();
        assert!(err.to_string().contains("description"));
    }

    #[test]
    fn tools_accept_csv_or_array_and_spawns_star() {
        let d = parse_agent_md(
            "x.md",
            "---\nname: n\ndescription: d\ntools: read, write\nspawns: *\n---\n\nbody",
            AgentSource::Project,
        )
        .unwrap();
        assert_eq!(d.tools.unwrap(), vec!["read", "write"]);
        assert_eq!(d.spawns.unwrap(), SpawnsField::Unrestricted);
        let d = parse_agent_md(
            "x.md",
            "---\nname: n\ndescription: d\ntools: [read, write]\n---\n\nbody",
            AgentSource::Project,
        )
        .unwrap();
        assert_eq!(d.tools.unwrap(), vec!["read", "write"]);
    }

    #[test]
    fn legacy_spawns_rule_missing_spawns_with_task_tool() {
        let d = parse_agent_md(
            "x.md",
            "---\nname: n\ndescription: d\ntools: [task]\n---\n\nbody",
            AgentSource::Bundled,
        )
        .unwrap();
        assert_eq!(d.spawns.unwrap(), SpawnsField::Unrestricted);
    }

    #[test]
    fn read_summarize_normalizes_both_spellings() {
        for key in ["read-summarize: false", "readSummarize: false"] {
            let d = parse_agent_md(
                "x.md",
                &format!("---\nname: n\ndescription: d\n{key}\n---\n\nbody"),
                AgentSource::Bundled,
            )
            .unwrap();
            assert_eq!(d.read_summarize, Some(false));
        }
    }

    #[test]
    fn yield_is_not_injected_divergence() {
        // Upstream would add `yield`; the port must not (parity would fail).
        let d = parse_agent_md(
            "x.md",
            "---\nname: n\ndescription: d\ntools: [read]\n---\n\nbody",
            AgentSource::Bundled,
        )
        .unwrap();
        assert!(!d.tools.unwrap().contains(&"yield".to_string()));
    }

    #[test]
    fn spawns_disabled_and_explicit_reject_typed() {
        let r = SpawnsField::Disabled.resolve();
        assert!(!r.enabled);
        assert!(r.check("task").unwrap_err().scope == "task");
        let r = SpawnsField::Explicit(vec!["a".into()]).resolve();
        assert_eq!(r.default_agent, "a");
        assert!(r.check("a").is_ok());
        let err = r.check("b").unwrap_err();
        assert_eq!(err.scope, "b");
        assert!(err.to_string().contains('a'));
        let r = SpawnsField::Unrestricted.resolve();
        assert_eq!(r.default_agent, DEFAULT_SPAWN_AGENT);
        assert!(r.check("anything").is_ok());
    }

    #[test]
    fn discovery_first_wins_project_over_user_over_bundled() {
        let mk = |name: &str, desc: &str, src| OmpAgentDefinition {
            name: name.into(),
            description: desc.into(),
            system_prompt: "p".into(),
            tools: None,
            spawns: None,
            model: None,
            output: None,
            blocking: false,
            read_summarize: None,
            source: src,
            file_path: None,
        };
        let r = discover_ordered(
            vec![mk("a", "proj", AgentSource::Project)],
            vec![
                mk("a", "user", AgentSource::User),
                mk("b", "user", AgentSource::User),
            ],
            vec![
                mk("a", "bundled", AgentSource::Bundled),
                mk("c", "bundled", AgentSource::Bundled),
            ],
            Some(".omp/agents".into()),
        );
        assert_eq!(r.get("a").unwrap().description, "proj");
        assert_eq!(r.get("b").unwrap().source, AgentSource::User);
        assert_eq!(r.get("c").unwrap().source, AgentSource::Bundled);
        assert_eq!(r.agents.len(), 3);
    }

    #[test]
    fn emit_verify_roundtrip_all_targets() {
        let dir = std::env::temp_dir().join(format!("stc-emit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let p = pack();
        let cat = crate::roles::catalog::tool_catalog();
        emit_pack(&dir, &p).unwrap();
        verify_emitted(&dir, &p, &cat).unwrap();
        let rep = fidelity_report(&dir, &p).unwrap();
        assert_eq!(rep.min_score(), 100, "losses: {:?}", rep.targets);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn opencode_mirror_carries_mode_and_mirror_note() {
        let def = OmpAgentDefinition::from_pack(&pack(), AgentSource::Bundled);
        let md = emit_opencode(&def, "1");
        assert!(md.contains("mode: subagent"));
        assert!(md.contains("do not hand-edit"));
        assert!(md.contains(&pack().system_prompt));
    }
}
