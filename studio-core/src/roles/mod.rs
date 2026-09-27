//! Agent templates + identity (Phase 5).
//! Versioned `RolePack`s with skill lockfiles + registration-parity checks;
//! multi-profile isolation via per-lane config dirs; capability-token secret broker
//! (workers never see raw creds; one-shot approval facts for OAuth'd writes).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RolePack {
    pub name: String,
    pub version: String,
    pub tools: Vec<String>,
    /// sha256 of the skill lockfile content
    pub skill_lockfile_hash: String,
    /// Authored system prompt (omp `AgentDefinition` contract).
    #[serde(default)]
    pub system_prompt: String,
    /// Least-privilege tool manifest; every `tools` entry must pass it.
    #[serde(default = "default_coder_manifest")]
    pub mcp_manifest: crate::mcp::RoleManifest,
    /// Model slot name (roles name slots, never providers).
    #[serde(default)]
    pub model_slot: String,
    #[serde(default)]
    pub harness_profile: HarnessProfile,
    #[serde(default)]
    pub spawns: SpawnPolicy,
    /// Optional JSON Schema for structured agent output.
    #[serde(default)]
    pub output_schema: Option<serde_json::Value>,
}

fn default_coder_manifest() -> crate::mcp::RoleManifest {
    crate::mcp::default_manifest(crate::mcp::AgentRole::Coder)
}

/// Which harness a pack targets (authoring contract: omp `AgentDefinition`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HarnessProfile {
    pub harness: String,
    pub version_req: String,
}

impl Default for HarnessProfile {
    fn default() -> Self {
        Self {
            harness: "omp".into(),
            version_req: ">=1".into(),
        }
    }
}

/// Spawn policy: children never escalate above the parent. A request above
/// the parent is rejected with a typed error, never silently downgraded
/// (happier pattern).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum SpawnPolicy {
    #[default]
    Isolated,
    Supervised {
        inheritable_scopes: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnDenied {
    pub reason: String,
}

impl SpawnPolicy {
    /// Child scopes must be a subset of the parent's. Anything else is a
    /// typed rejection (fail-closed, no downgrade).
    pub fn request(
        &self,
        child_scopes: &[String],
        parent_scopes: &[String],
    ) -> Result<(), SpawnDenied> {
        let allowed: Vec<&String> = match self {
            SpawnPolicy::Isolated => vec![],
            SpawnPolicy::Supervised { inheritable_scopes } => inheritable_scopes
                .iter()
                .filter(|s| parent_scopes.contains(s))
                .collect(),
        };
        for scope in child_scopes {
            if !allowed.contains(&scope) {
                return Err(SpawnDenied {
                    reason: format!("child scope {scope} exceeds inheritable authority"),
                });
            }
        }
        Ok(())
    }
}

/// Parts for one engineering-batch pack (spec §1, #12 batch 1).
struct BatchParts {
    name: &'static str,
    tools: Vec<String>,
    prompt: String,
    manifest: crate::mcp::RoleManifest,
    model_slot: &'static str,
    harness: &'static str,
    spawns: SpawnPolicy,
    output_schema: Option<serde_json::Value>,
}

impl RolePack {
    pub fn lockfile_hash(content: &str) -> String {
        let mut h = Sha256::new();
        h.update(content.as_bytes());
        hex::encode(h.finalize())
    }

    /// Parity: every tool must exist in the MCP catalog; no extras unregistered.
    pub fn check_parity(&self, catalog: &[String]) -> Result<(), String> {
        for t in &self.tools {
            if !catalog.contains(t) {
                return Err(format!("phantom tool in RolePack {}: {t}", self.name));
            }
        }
        Ok(())
    }

    /// Manifest coherence: every pack tool must pass the role manifest
    /// (double enforcement with the gateway at resolution time).
    pub fn check_manifest(&self) -> Result<(), String> {
        for t in &self.tools {
            self.mcp_manifest
                .check(t)
                .map_err(|e| format!("pack tool {t} fails manifest: {e}"))?;
        }
        Ok(())
    }

    /// Refuse raw secret material anywhere in the pack (placeholders only).
    pub fn check_no_raw_secrets(&self) -> Result<(), String> {
        reject_raw_secrets("system_prompt", &self.system_prompt)
    }

    /// Serialize to `rolepack.yaml` in `dir` (after secret + parity checks).
    pub fn save_pack(
        &self,
        dir: &std::path::Path,
        catalog: &[String],
    ) -> Result<std::path::PathBuf, String> {
        self.check_parity(catalog)?;
        self.check_manifest()?;
        self.check_no_raw_secrets()?;
        let body = serde_yaml::to_string(self).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let path = dir.join("rolepack.yaml");
        std::fs::write(&path, body).map_err(|e| e.to_string())?;
        Ok(path)
    }

    pub fn load_pack(dir: &std::path::Path) -> Result<Self, String> {
        let body = std::fs::read_to_string(dir.join("rolepack.yaml")).map_err(|e| e.to_string())?;
        serde_yaml::from_str(&body).map_err(|e| e.to_string())
    }

    /// The one flagship pack. Scope deliberately narrow: web coding tasks,
    /// coder model slot, omp harness profile.
    pub fn coder_web() -> Self {
        let mut manifest = default_coder_manifest();
        // A web coder needs search + syntax on demand (least privilege, not deny).
        use crate::mcp::Access;
        manifest
            .rules
            .insert("code_search".into(), Access::OnDemand);
        manifest
            .rules
            .insert("syntax_check".into(), Access::OnDemand);
        Self {
            name: "coder-web".into(),
            version: "1".into(),
            tools: vec![
                "read".into(),
                "write".into(),
                "edit".into(),
                "patch".into(),
                "runProcess".into(),
                "code_search".into(),
                "syntax_check".into(),
                "tool_open".into(),
                "kv_get".into(),
            ],
            skill_lockfile_hash: Self::lockfile_hash("coder-web-skills-v1"),
            system_prompt: "You are a web-coding specialist. Implement exactly the declared task; \
                request scope expansion instead of improvising. Evidence before claims: every \
                completed file must be verified by tests or checks before reporting done."
                .into(),
            mcp_manifest: manifest,
            model_slot: "coder.primary".into(),
            harness_profile: HarnessProfile::default(),
            spawns: SpawnPolicy::Supervised {
                inheritable_scopes: vec!["read".into(), "code_search".into()],
            },
            output_schema: Some(serde_json::json!({
                "type": "object",
                "required": ["summary", "files_changed", "tests"],
                "properties": {
                    "summary": {"type": "string"},
                    "files_changed": {"type": "array", "items": {"type": "string"}},
                    "tests": {"type": "string"}
                }
            })),
        }
    }

    fn pi_harness() -> HarnessProfile {
        HarnessProfile {
            harness: "pi".into(),
            version_req: ">=1".into(),
        }
    }

    /// Pi profile: reviewer-security (≈ spec §2.2 security-auditor).
    /// OWASP/STRIDE findings with file:line evidence; never prose-only verdicts.
    /// Slot `reviewer` (auto-routing forbidden), pi harness, Isolated.
    pub fn reviewer_security() -> Self {
        let mut manifest = crate::mcp::default_manifest(crate::mcp::AgentRole::Reviewer);
        // Security review needs semantic search on demand (read-only, least privilege).
        manifest
            .rules
            .insert("code_search".into(), crate::mcp::Access::OnDemand);
        Self {
            name: "reviewer-security".into(),
            version: "1".into(),
            tools: vec![
                "read".into(),
                "code_search".into(),
                "tool_open".into(),
                "kv_get".into(),
                "sast".into(),
                "syntax_check".into(),
            ],
            skill_lockfile_hash: Self::lockfile_hash("reviewer-security-skills-v1"),
            system_prompt: "You are a security-review specialist. Hold the production-ready bar: \
                never untested, never pseudo-code, no hallucinated APIs, no unrelated deletions, \
                no secrets in evidence, no unvetted dependencies. Declare your verification strategy \
                before acting (sast scan scope, syntax pass, manual OWASP Top 10 + STRIDE checklist); \
                verify-and-correct after. Report severity-rated findings with file:line:rule evidence; \
                zero critical/high unfixed or a CODEOWNERS waiver object. Escalate blockers to the PM, \
                breaking changes to the architect, completed reviews to QA with test commands."
                .into(),
            mcp_manifest: manifest,
            model_slot: "reviewer".into(),
            harness_profile: Self::pi_harness(),
            spawns: SpawnPolicy::Isolated,
            output_schema: Some(serde_json::json!({
                "type": "object",
                "required": ["summary", "findings", "verdict"],
                "properties": {
                    "summary": {"type": "string"},
                    "findings": {"type": "array", "items": {"type": "string"}},
                    "verdict": {"type": "string"}
                }
            })),
        }
    }

    /// Pi profile: researcher-docs (≈ spec §4.5 tech-writer).
    /// Docs accuracy + freshness + link checking; build-clean deltas.
    /// Slot `researcher`, pi harness, Isolated.
    pub fn researcher_docs() -> Self {
        let mut manifest = crate::mcp::default_manifest(crate::mcp::AgentRole::Researcher);
        // Docs authoring needs scoped writes on demand (docs dir only by convention).
        manifest
            .rules
            .insert("write".into(), crate::mcp::Access::OnDemand);
        manifest
            .rules
            .insert("edit".into(), crate::mcp::Access::OnDemand);
        Self {
            name: "researcher-docs".into(),
            version: "1".into(),
            tools: vec![
                "read".into(),
                "write".into(),
                "edit".into(),
                "code_search".into(),
                "tool_open".into(),
                "kv_get".into(),
                "web_search".into(),
            ],
            skill_lockfile_hash: Self::lockfile_hash("researcher-docs-skills-v1"),
            system_prompt: "You are a researcher and documentation specialist. Research from \
                source of record first (code, docs, upstream); never present generated stubs as \
                reviewed. Negative constraints: no hallucinated APIs, no unrelated deletions, no \
                secrets, no unvetted dependencies. Declare your verification strategy before acting \
                (sources to consult, freshness checks, link check, docs build); verify-and-correct \
                after. Every claim carries its source; docs deltas must build clean. Escalate \
                blocked research to the PM and completed docs to QA with build commands."
                .into(),
            mcp_manifest: manifest,
            model_slot: "researcher".into(),
            harness_profile: Self::pi_harness(),
            spawns: SpawnPolicy::Isolated,
            output_schema: Some(serde_json::json!({
                "type": "object",
                "required": ["summary", "sources", "files_changed"],
                "properties": {
                    "summary": {"type": "string"},
                    "sources": {"type": "array", "items": {"type": "string"}},
                    "files_changed": {"type": "array", "items": {"type": "string"}}
                }
            })),
        }
    }

    /// Pi profile: planner-spec (≈ spec §1.1 systems-architect, proposal-only).
    /// End-to-end specs with module boundaries and interfaces; never writes
    /// implementation (no write/edit/patch tools at all).
    /// Named `planner` slot (never a provider), pi harness,
    /// Supervised with narrow inheritable scopes.
    pub fn planner_spec() -> Self {
        Self {
            name: "planner-spec".into(),
            version: "1".into(),
            tools: vec![
                "read".into(),
                "code_search".into(),
                "tool_open".into(),
                "kv_get".into(),
                "plan_open".into(),
                "retrieve_docs".into(),
            ],
            skill_lockfile_hash: Self::lockfile_hash("planner-spec-skills-v1"),
            system_prompt: "You are a planning specialist. Produce specs, RFCs, interface lists, \
                and boundary maps — never implementation. Outputs are documents and decisions: every \
                directive carries owner, acceptance criteria, and deadline-tick. Negative constraints: \
                no hallucinated APIs, no scope improvisation (request expansion instead), no secrets. \
                Declare your verification strategy before acting (interfaces to confirm, risks to \
                retire); verify-and-correct after. Escalate blocked planning to the PM and completed \
                specs to the dispatcher with acceptance criteria."
                .into(),
            mcp_manifest: crate::mcp::default_manifest(crate::mcp::AgentRole::Manager),
            model_slot: "planner".into(),
            harness_profile: Self::pi_harness(),
            spawns: SpawnPolicy::Supervised {
                inheritable_scopes: vec!["read".into(), "plan_open".into()],
            },
            output_schema: Some(serde_json::json!({
                "type": "object",
                "required": ["summary", "decisions", "interfaces", "risks"],
                "properties": {
                    "summary": {"type": "string"},
                    "decisions": {"type": "array", "items": {"type": "string"}},
                    "interfaces": {"type": "array", "items": {"type": "string"}},
                    "risks": {"type": "array", "items": {"type": "string"}}
                }
            })),
        }
    }

    /// Pi profile: qa-tests (≈ spec §2.3 test-synthesizer).
    /// Failing test first, minimal passing code; red→green demonstrated.
    /// Named `qa` slot (never a provider), pi harness, Isolated.
    pub fn qa_tests() -> Self {
        let mut manifest = crate::mcp::default_manifest(crate::mcp::AgentRole::Coder);
        // QA needs search + syntax on demand (least privilege, not deny).
        use crate::mcp::Access;
        manifest
            .rules
            .insert("code_search".into(), Access::OnDemand);
        manifest
            .rules
            .insert("syntax_check".into(), Access::OnDemand);
        Self {
            name: "qa-tests".into(),
            version: "1".into(),
            tools: vec![
                "read".into(),
                "write".into(),
                "edit".into(),
                "patch".into(),
                "runProcess".into(),
                "code_search".into(),
                "tool_open".into(),
                "kv_get".into(),
            ],
            skill_lockfile_hash: Self::lockfile_hash("qa-tests-skills-v1"),
            system_prompt: "You are a test-synthesis specialist. Failing test first, minimal \
                passing code; every requirement clause gets positive, negative, and edge cases in \
                Given-When-Then form. Hold the production-ready bar: red→green demonstrated in the \
                log, no flaky landings (re-run where the harness supports it). Negative constraints: \
                no hallucinated APIs, no unrelated deletions, no secrets, no unvetted dependencies. \
                Escalate blocked tests to the PM and completed suites to QA with test commands."
                .into(),
            mcp_manifest: manifest,
            model_slot: "qa".into(),
            harness_profile: Self::pi_harness(),
            spawns: SpawnPolicy::Isolated,
            output_schema: Some(serde_json::json!({
                "type": "object",
                "required": ["summary", "files_changed", "tests"],
                "properties": {
                    "summary": {"type": "string"},
                    "files_changed": {"type": "array", "items": {"type": "string"}},
                    "tests": {"type": "string"}
                }
            })),
        }
    }

    /// The five Pi-profile packs (#13): coder-web flagship + four new profiles.
    pub fn pi_library() -> Vec<Self> {
        vec![
            Self::coder_web(),
            Self::reviewer_security(),
            Self::researcher_docs(),
            Self::planner_spec(),
            Self::qa_tests(),
        ]
    }

    fn schema_code() -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "type": "object",
            "required": ["summary", "files_changed", "tests"],
            "properties": {
                "summary": {"type": "string"},
                "files_changed": {"type": "array", "items": {"type": "string"}},
                "tests": {"type": "string"}
            }
        }))
    }

    fn schema_spec() -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "type": "object",
            "required": ["summary", "decisions", "interfaces", "risks"],
            "properties": {
                "summary": {"type": "string"},
                "decisions": {"type": "array", "items": {"type": "string"}},
                "interfaces": {"type": "array", "items": {"type": "string"}},
                "risks": {"type": "array", "items": {"type": "string"}}
            }
        }))
    }

    /// Coder-lens manifest shared by implementation roles (coder_web precedent:
    /// Coder base plus search + syntax on demand, least privilege not deny).
    fn coder_lens() -> crate::mcp::RoleManifest {
        let mut manifest = crate::mcp::default_manifest(crate::mcp::AgentRole::Coder);
        manifest
            .rules
            .insert("code_search".into(), crate::mcp::Access::OnDemand);
        manifest
            .rules
            .insert("syntax_check".into(), crate::mcp::Access::OnDemand);
        manifest
    }

    fn harness_named(harness: &str) -> HarnessProfile {
        HarnessProfile {
            harness: harness.into(),
            version_req: ">=1".into(),
        }
    }

    /// One engineering-batch pack from spec §1 (#12 batch 1). Tools are always
    /// a subset of `tools` passing `manifest`; prompts carry identity,
    /// constraints, DoD, and handoff per the spec role text.
    fn batch_pack(parts: BatchParts) -> Self {
        Self {
            name: parts.name.into(),
            version: "1".into(),
            tools: parts.tools,
            skill_lockfile_hash: Self::lockfile_hash(&format!("{}-skills-v1", parts.name)),
            system_prompt: parts.prompt,
            mcp_manifest: parts.manifest,
            model_slot: parts.model_slot.into(),
            harness_profile: Self::harness_named(parts.harness),
            spawns: parts.spawns,
            output_schema: parts.output_schema,
        }
    }

    fn coder_tools(with_syntax: bool) -> Vec<String> {
        let mut t = vec![
            "read".into(),
            "write".into(),
            "edit".into(),
            "patch".into(),
            "runProcess".into(),
            "code_search".into(),
            "tool_open".into(),
            "kv_get".into(),
        ];
        if with_syntax {
            t.push("syntax_check".into());
        }
        t
    }

    /// #12 batch 1: engineering remainder, spec §1.1–§1.10 (systems-architect,
    /// rust/fullstack-ts/python/go/firmware/mobile/compiler/dba/cloud-k8s).
    /// Harness + slot + DoD follow the spec role text; no catalog additions.
    pub fn engineering_batch() -> Vec<Self> {
        let iso = SpawnPolicy::Isolated;
        vec![
            Self::batch_pack(BatchParts {
                name: "systems-architect",
                tools: vec![
                    "read".into(),
                    "code_search".into(),
                    "tool_open".into(),
                    "kv_get".into(),
                    "plan_open".into(),
                    "dag_commit".into(),
                    "retrieve_docs".into(),
                ],
                prompt: "You are a systems architect. Own end-to-end architecture: module boundaries, \
                protocols, RFCs with interface lists and boundary maps — never implementation. \
                Negative constraints: no hallucinated APIs, no scope improvisation, no secrets. \
                Declare your verification strategy before acting (boundaries to confirm, risks to \
                retire); verify-and-correct after. DoD: RFC artifact + interface list + boundary map, \
                validated via dag_commit. Handoff to the scrum-dispatcher {rfc, interfaces}, and to QA \
                on breaking changes."
                    .into(),
                manifest: crate::mcp::default_manifest(crate::mcp::AgentRole::Manager),
                model_slot: "architect",
                harness: "omp",
                spawns: SpawnPolicy::Supervised {
                    inheritable_scopes: vec!["read".into(), "plan_open".into()],
                },
                output_schema: Self::schema_spec(),
            }),
            Self::batch_pack(BatchParts {
                name: "rust-systems-engineer",
                tools: Self::coder_tools(true),
                prompt: "You are a Rust systems engineer. Memory-safe zero-cost Rust: borrow-check-clean \
                or it does not land; no unwrap on new paths without a justification object. \
                Negative constraints: no hallucinated APIs, no unrelated deletions, no secrets, no \
                unvetted dependencies. Declare your verification strategy before acting (clippy, tests, \
                tree-sitter parse); verify-and-correct after. DoD: cargo clippy -D warnings + cargo test \
                green + tree-sitter parse. Handoff to the test-synthesizer {files, test-cmd}, then QA."
                    .into(),
                manifest: Self::coder_lens(),
                model_slot: "coder.primary",
                harness: "opencode",
                spawns: iso.clone(),
                output_schema: Self::schema_code(),
            }),
            Self::batch_pack(BatchParts {
                name: "fullstack-ts-engineer",
                tools: Self::coder_tools(true),
                prompt: "You are a fullstack TypeScript engineer. Type-safe TS/React/Node with non-any \
                annotations; fallow-clean deltas. Negative constraints: no hallucinated APIs, no \
                unrelated deletions, no secrets, no unvetted dependencies. Declare your verification \
                strategy before acting (tsc, tests, fallow audit); verify-and-correct after. DoD: tsc + \
                fallow audit new-findings-zero + tests green. Handoff to the test-synthesizer, then QA."
                    .into(),
                manifest: Self::coder_lens(),
                model_slot: "coder.primary",
                harness: "opencode",
                spawns: iso.clone(),
                output_schema: Self::schema_code(),
            }),
            Self::batch_pack(BatchParts {
                name: "python-backend-engineer",
                tools: Self::coder_tools(false),
                prompt: "You are a Python backend engineer. Strict-mypy Python with Pydantic runtime \
                validation at API boundaries. Negative constraints: no hallucinated APIs, no unrelated \
                deletions, no secrets, no unvetted dependencies. Declare your verification strategy \
                before acting (mypy strict, ruff, pytest); verify-and-correct after. DoD: mypy strict + \
                ruff + pytest green. Handoff to the test-synthesizer, then QA."
                    .into(),
                manifest: Self::coder_lens(),
                model_slot: "coder.primary",
                harness: "pi",
                spawns: iso.clone(),
                output_schema: Self::schema_code(),
            }),
            Self::batch_pack(BatchParts {
                name: "go-services-engineer",
                tools: Self::coder_tools(false),
                prompt: "You are a Go services engineer. Idiomatic Go services; go vet clean, race detector \
                on concurrency work. Negative constraints: no hallucinated APIs, no unrelated deletions, \
                no secrets, no unvetted dependencies. Declare your verification strategy before acting \
                (vet, race-enabled tests); verify-and-correct after. DoD: go vet + go test -race green. \
                Handoff to the test-synthesizer, then QA."
                    .into(),
                manifest: Self::coder_lens(),
                model_slot: "coder.primary",
                harness: "pi",
                spawns: iso.clone(),
                output_schema: Self::schema_code(),
            }),
            Self::batch_pack(BatchParts {
                name: "firmware-embedded-engineer",
                tools: Self::coder_tools(false),
                prompt: "You are a firmware and embedded engineer (ARM Cortex, RISC-V, bare metal). No heap \
                without justification; hardware-in-loop notes where applicable. Negative constraints: no \
                hallucinated APIs, no unrelated deletions, no secrets, no unvetted dependencies. Declare \
                your verification strategy before acting (cross-compile, size report, target test or HIL \
                note); verify-and-correct after. DoD: cross-compile clean + size report + target test or \
                HIL note. Handoff to the test-synthesizer, then QA."
                    .into(),
                manifest: Self::coder_lens(),
                model_slot: "coder.primary",
                harness: "pi",
                spawns: iso.clone(),
                output_schema: Self::schema_code(),
            }),
            Self::batch_pack(BatchParts {
                name: "mobile-engineer",
                tools: Self::coder_tools(false),
                prompt: "You are a cross-platform mobile engineer (React Native, Flutter, Kotlin, Swift). \
                Offline-sync paths tested; no main-thread I/O. Negative constraints: no hallucinated \
                APIs, no unrelated deletions, no secrets, no unvetted dependencies. Declare your \
                verification strategy before acting (typecheck, unit tests, offline-path test or waiver); \
                verify-and-correct after. DoD: typecheck + unit tests + offline-path test or explicit \
                waiver. Handoff to the test-synthesizer, then QA."
                    .into(),
                manifest: Self::coder_lens(),
                model_slot: "coder.primary",
                harness: "pi",
                spawns: iso.clone(),
                output_schema: Self::schema_code(),
            }),
            Self::batch_pack(BatchParts {
                name: "compiler-tooling-engineer",
                tools: Self::coder_tools(true),
                prompt: "You are a compiler and tooling engineer (AST parsers, linters, codegen). Snapshot \
                and golden tests mandatory for every transform; fuzz-corpus entry for new parser rules. \
                Negative constraints: no hallucinated APIs, no unrelated deletions, no secrets, no \
                unhandled-grammar panics. Declare your verification strategy before acting (golden tests, \
                rule docs); verify-and-correct after. DoD: golden tests green + new-rule docs. Handoff to \
                the test-synthesizer, then QA."
                    .into(),
                manifest: Self::coder_lens(),
                model_slot: "coder.primary",
                harness: "pi",
                spawns: iso.clone(),
                output_schema: Self::schema_code(),
            }),
            Self::batch_pack(BatchParts {
                name: "dba-data-engineer",
                tools: Self::coder_tools(false),
                prompt: "You are a DBA and data engineer. Migrations are expand-migrate-contract: \
                backward-compatible, zero-downtime, with rollback scripts; N+1 joins over selects; \
                read-only production access (plan only, never apply). Negative constraints: no \
                hallucinated APIs, no secrets. Declare your verification strategy before acting \
                (migration + rollback + EXPLAIN note on new indexes); verify-and-correct after. DoD: \
                migration + rollback script + EXPLAIN note. Handoff to the test-synthesizer, then QA \
                and the release-engineer."
                    .into(),
                manifest: Self::coder_lens(),
                model_slot: "coder.primary",
                harness: "pi",
                spawns: iso.clone(),
                output_schema: Self::schema_code(),
            }),
            Self::batch_pack(BatchParts {
                name: "cloud-k8s-architect",
                tools: {
                    let mut t = Self::coder_tools(false);
                    t.push("retrieve_docs".into());
                    t
                },
                prompt: "You are a cloud and Kubernetes architect (Terraform, K8s, mesh). Attach terraform plan \
                output; apply is never executed (a human runs apply). Manifests follow a CIS checklist \
                (no root, no privileged). Negative constraints: no hallucinated APIs, no secrets. Declare \
                your verification strategy before acting (plan artifact, CIS checklist); verify-and-correct \
                after. DoD: plan artifact + CIS checklist. Handoff to devops-sre, then the release-engineer."
                    .into(),
                manifest: Self::coder_lens(),
                model_slot: "architect",
                harness: "omp",
                spawns: iso,
                output_schema: Self::schema_spec(),
            }),
        ]
    }

    fn schema_verdict() -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "type": "object",
            "required": ["summary", "verdict", "evidence_refs"],
            "properties": {
                "summary": {"type": "string"},
                "verdict": {"type": "string"},
                "evidence_refs": {"type": "array", "items": {"type": "string"}}
            }
        }))
    }

    /// Reviewer-lens manifest shared by QA/security roles (Reviewer base plus
    /// semantic search on demand, read-only, least privilege not deny).
    fn reviewer_lens() -> crate::mcp::RoleManifest {
        let mut manifest = crate::mcp::default_manifest(crate::mcp::AgentRole::Reviewer);
        manifest
            .rules
            .insert("code_search".into(), crate::mcp::Access::OnDemand);
        manifest
    }

    /// #12 batch 2: QA/security remainder, spec §2.1–§2.5 (qa-director,
    /// security-auditor, test-synthesizer, perf-qa-engineer,
    /// accessibility-auditor). No catalog additions.
    pub fn qa_batch() -> Vec<Self> {
        let researcher = crate::mcp::default_manifest(crate::mcp::AgentRole::Researcher);
        vec![
            Self::batch_pack(BatchParts {
                name: "qa-director",
                tools: vec![
                    "read".into(),
                    "code_search".into(),
                    "tool_open".into(),
                    "kv_get".into(),
                    "syntax_check".into(),
                    "sast".into(),
                    "sbom".into(),
                ],
                prompt: "You are the QA director. Define gates and own sign-off: block on red, \
                never self-approve. Coverage delta >= 0, zero new error findings, SBOM attached. \
                Negative constraints: no prose-only verdicts, no hallucinated APIs, no secrets. \
                Declare your verification strategy before acting (gates to check, evidence to demand); \
                verify-and-correct after. DoD: verdict object {approve|rewind, evidence_refs[], fix_tasks[]}. \
                Handoff to the pr-gatekeeper {verdict, evidence}, or rewind with fix-tasks."
                    .into(),
                manifest: Self::reviewer_lens(),
                model_slot: "reviewer",
                harness: "omp",
                spawns: SpawnPolicy::Supervised {
                    inheritable_scopes: vec!["read".into(), "code_search".into()],
                },
                output_schema: Self::schema_verdict(),
            }),
            Self::batch_pack(BatchParts {
                name: "security-auditor",
                tools: vec![
                    "read".into(),
                    "code_search".into(),
                    "tool_open".into(),
                    "kv_get".into(),
                    "sast".into(),
                    "syntax_check".into(),
                ],
                prompt: "You are a security auditor. Run OWASP Top 10 + STRIDE against component \
                diagrams; severity-rated findings with file:line:rule evidence, never prose-only \
                verdicts. Semgrep Guardian findings are accepted as input evidence. Negative \
                constraints: no hallucinated APIs, no secrets. Declare your verification strategy \
                before acting (sast scope, checklist coverage); verify-and-correct after. DoD: severity \
                report, zero critical/high unfixed or a CODEOWNERS waiver object. Handoff to the \
                qa-director."
                    .into(),
                manifest: Self::reviewer_lens(),
                model_slot: "reviewer",
                harness: "pi",
                spawns: SpawnPolicy::Isolated,
                output_schema: Self::schema_verdict(),
            }),
            Self::batch_pack(BatchParts {
                name: "test-synthesizer",
                tools: Self::coder_tools(false),
                prompt: "You are a test synthesizer. Failing test first, minimal passing code; \
                Given-When-Then; every requirement clause gets positive, negative, and edge cases. \
                Demonstrate red-to-green in the log; re-run shuffled 3x where the harness supports it \
                to catch flakes. Negative constraints: no hallucinated APIs, no unrelated deletions, no \
                secrets, no unvetted dependencies. Handoff to the qa-director."
                    .into(),
                manifest: Self::coder_lens(),
                model_slot: "coder.primary",
                harness: "pi",
                spawns: SpawnPolicy::Isolated,
                output_schema: Self::schema_code(),
            }),
            Self::batch_pack(BatchParts {
                name: "perf-qa-engineer",
                tools: vec![
                    "read".into(),
                    "runProcess".into(),
                    "code_search".into(),
                    "tool_open".into(),
                    "kv_get".into(),
                ],
                prompt: "You are a performance QA engineer. Benchmarks with baselines; p95 and latency \
                SLOs stated per change; no optimization without profiling evidence. Negative \
                constraints: no hallucinated numbers, no secrets. Declare your verification strategy \
                before acting (baseline, workload, metric); verify-and-correct after. DoD: benchmark \
                delta report vs baseline; regressions block with numbers. Handoff to the qa-director."
                    .into(),
                manifest: researcher.clone(),
                model_slot: "reviewer",
                harness: "pi",
                spawns: SpawnPolicy::Isolated,
                output_schema: Self::schema_verdict(),
            }),
            Self::batch_pack(BatchParts {
                name: "accessibility-auditor",
                tools: vec![
                    "read".into(),
                    "code_search".into(),
                    "tool_open".into(),
                    "kv_get".into(),
                    "web_search".into(),
                ],
                prompt: "You are an accessibility auditor. WCAG 2.1 AA checklist: ARIA, semantics, \
                keyboard paths, contrast; axe or pa11y findings where available. Negative constraints: \
                no checklist theater — every rule cites its violating nodes; no secrets. Declare your \
                verification strategy before acting (rules in scope, nodes sampled); verify-and-correct \
                after. DoD: pass/fail per rule with violating nodes. Handoff to the qa-director."
                    .into(),
                manifest: researcher,
                model_slot: "reviewer",
                harness: "pi",
                spawns: SpawnPolicy::Isolated,
                output_schema: Self::schema_verdict(),
            }),
        ]
    }
}

/// Raw secret patterns: never persisted in packs, locks, or emitted configs.
fn reject_raw_secrets(field: &str, text: &str) -> Result<(), String> {
    for tok in text.split_whitespace() {
        let t = tok.trim_matches(|c: char| c.is_ascii_punctuation());
        if (t.starts_with("sk-") && t.len() > 12)
            || (t.starts_with("AKIA") && t.len() == 20)
            || t.starts_with("xoxb-")
            || t.starts_with("xoxp-")
            || t.starts_with("ghp_")
        {
            return Err(format!("raw secret material in {field}"));
        }
    }
    Ok(())
}

/// Per-lane profile isolation: config dir per lane (never shared credentials).
pub fn profile_dir(base: &str, lane: &str) -> String {
    format!("{base}/profiles/{lane}")
}

/// Content-hash lock over the four asset kinds (skills, commands, MCPs,
/// instructions), extending the shipped `skills-lock.json` pattern.
/// `rolepack.lock` + `--locked` verification in CI and worktree provisioning.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RolePackLock {
    pub pack_name: String,
    pub pack_version: String,
    pub pack_hash: String,
    /// kind -> asset name -> sha256(content)
    pub assets: HashMap<String, HashMap<String, String>>,
}

pub fn hash_asset(content: &str) -> String {
    RolePack::lockfile_hash(content)
}

/// Canonical content hash: JSON with sorted keys, so `HashMap` iteration
/// order can never cause phantom drift between compute and verify.
fn canonical_hash<T: Serialize>(v: &T) -> Result<String, String> {
    let json: serde_json::Value = serde_json::to_value(v).map_err(|e| e.to_string())?;
    Ok(hash_asset(
        &serde_json::to_string(&json).map_err(|e| e.to_string())?,
    ))
}

/// Compute a lock for a pack over caller-supplied assets. Secrets must never
/// appear here: assets are rejected by the same raw-secret rule as packs.
pub fn compute_lock(
    pack: &RolePack,
    assets: &HashMap<String, HashMap<String, String>>,
    asset_contents: &HashMap<String, HashMap<String, String>>,
) -> Result<RolePackLock, String> {
    for (kind, files) in asset_contents {
        for (name, content) in files {
            reject_raw_secrets(&format!("asset {kind}/{name}"), content)?;
            let want = assets
                .get(kind)
                .and_then(|m| m.get(name))
                .ok_or_else(|| format!("asset {kind}/{name} missing from hash map"))?;
            if want != &hash_asset(content) {
                return Err(format!("asset {kind}/{name} hash mismatch"));
            }
        }
    }
    let pack_hash = canonical_hash(pack)?;
    Ok(RolePackLock {
        pack_name: pack.name.clone(),
        pack_version: pack.version.clone(),
        pack_hash,
        assets: assets.clone(),
    })
}

pub fn write_lock(
    dir: &std::path::Path,
    lock: &RolePackLock,
) -> Result<std::path::PathBuf, String> {
    let body = serde_yaml::to_string(lock).map_err(|e| e.to_string())?;
    let path = dir.join("rolepack.lock");
    std::fs::write(&path, body).map_err(|e| e.to_string())?;
    Ok(path)
}

/// `--locked` verification: recompute every hash from the asset contents and
/// compare against the lock. Any drift fails closed.
pub fn verify_lock(
    dir: &std::path::Path,
    asset_contents: &HashMap<String, HashMap<String, String>>,
) -> Result<(), String> {
    let pack = RolePack::load_pack(dir)?;
    let body = std::fs::read_to_string(dir.join("rolepack.lock")).map_err(|e| e.to_string())?;
    let lock: RolePackLock = serde_yaml::from_str(&body).map_err(|e| e.to_string())?;
    if lock.pack_name != pack.name || lock.pack_version != pack.version {
        return Err("lock names a different pack version".into());
    }
    let recomputed = compute_lock(&pack, &lock.assets, asset_contents)?;
    if recomputed.pack_hash != lock.pack_hash || recomputed.assets != lock.assets {
        return Err("rolepack.lock drift: asset hashes do not match".into());
    }
    Ok(())
}

/// Managed-block upsert for `CLAUDE.md` / `AGENTS.md`: replaces content
/// between `<!-- studio:<marker> start -->` / `<!-- studio:<marker> end -->`,
/// or appends the block. Never rewrites outside the block.
pub fn upsert_managed_block(
    path: &std::path::Path,
    marker: &str,
    block: &str,
) -> Result<(), String> {
    let start = format!("<!-- studio:{marker} start -->");
    let end = format!("<!-- studio:{marker} end -->");
    let current = std::fs::read_to_string(path).unwrap_or_default();
    let replacement = format!("{start}\n{block}\n{end}");
    let next = if let Some(s) = current.find(&start) {
        if let Some(e) = current.find(&end) {
            format!(
                "{}{replacement}{}",
                &current[..s],
                &current[e + end.len()..]
            )
        } else {
            return Err(format!(
                "dangling managed block {marker}: start without end"
            ));
        }
    } else {
        format!("{current}\n{replacement}\n")
    };
    if let Some(p) = path.parent() {
        if !p.as_os_str().is_empty() {
            std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
    }
    std::fs::write(path, next).map_err(|e| e.to_string())
}

/// Emission targets, compiled from one pack source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmitTarget {
    /// omp-native `agents/<name>.md` drop.
    Omp,
    /// base-pi `package.json#pi` shim.
    BasePi,
    /// `.opencode/agents/<name>.md` mirror.
    Opencode,
}

/// Compile a pack to all three targets. Each file carries a `tools:` front
/// matter listing exactly the pack tools (machine-checkable parity).
pub fn emit_pack(
    dir: &std::path::Path,
    pack: &RolePack,
) -> Result<Vec<std::path::PathBuf>, String> {
    pack.check_no_raw_secrets()?;
    let tools_line = pack.tools.join(", ");
    let omp = format!(
        "---\nname: {}\nmodel_slot: {}\ntools: [{}]\n---\n\n{}\n",
        pack.name, pack.model_slot, tools_line, pack.system_prompt
    );
    let shim = serde_json::json!({
        "name": pack.name,
        "pi": {
            "system_prompt": pack.system_prompt,
            "tools": pack.tools,
            "model_slot": pack.model_slot,
        }
    });
    let mirror = format!(
        "---\nname: {}\ntools: [{}]\n---\n\n> Mirrored from RolePack {} v{} (do not hand-edit).\n\n{}\n",
        pack.name, tools_line, pack.name, pack.version, pack.system_prompt
    );
    let files = [
        (format!("agents/{}.md", pack.name), omp),
        (
            "package.json".to_string(),
            serde_json::to_string_pretty(&shim).unwrap(),
        ),
        (format!(".opencode/agents/{}.md", pack.name), mirror),
    ];
    let mut out = vec![];
    for (rel, body) in files {
        let path = dir.join(rel);
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, body).map_err(|e| e.to_string())?;
        out.push(path);
    }
    Ok(out)
}

/// Verify emitted targets: each parses back to exactly the pack tools, and
/// every tool exists in the catalog (extends `check_parity` to all targets).
pub fn verify_emitted(
    dir: &std::path::Path,
    pack: &RolePack,
    catalog: &[String],
) -> Result<(), String> {
    pack.check_parity(catalog)?;
    for target in [EmitTarget::Omp, EmitTarget::BasePi, EmitTarget::Opencode] {
        let tools = read_emitted_tools(dir, pack, target)?;
        if tools != pack.tools {
            return Err(format!("{target:?} tools drift from pack {}", pack.name));
        }
        for t in &tools {
            if !catalog.contains(t) {
                return Err(format!("{target:?} references unregistered tool {t}"));
            }
        }
    }
    Ok(())
}

fn read_emitted_tools(
    dir: &std::path::Path,
    pack: &RolePack,
    target: EmitTarget,
) -> Result<Vec<String>, String> {
    match target {
        EmitTarget::BasePi => {
            let body =
                std::fs::read_to_string(dir.join("package.json")).map_err(|e| e.to_string())?;
            let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
            v.pointer("/pi/tools")
                .and_then(|t| t.as_array())
                .ok_or_else(|| "base-pi shim missing pi.tools".to_string())?
                .iter()
                .map(|t| {
                    t.as_str()
                        .map(|s| s.to_string())
                        .ok_or_else(|| "non-string tool in shim".to_string())
                })
                .collect()
        }
        EmitTarget::Omp => read_front_matter_tools(&dir.join(format!("agents/{}.md", pack.name))),
        EmitTarget::Opencode => {
            read_front_matter_tools(&dir.join(format!(".opencode/agents/{}.md", pack.name)))
        }
    }
}

fn read_front_matter_tools(path: &std::path::Path) -> Result<Vec<String>, String> {
    let body = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let fm = body
        .strip_prefix("---\n")
        .and_then(|b| b.split_once("\n---\n"))
        .map(|(f, _)| f)
        .ok_or_else(|| format!("{} missing front matter", path.display()))?;
    for line in fm.lines() {
        if let Some(list) = line.trim().strip_prefix("tools:") {
            let inner = list.trim().trim_start_matches('[').trim_end_matches(']');
            return Ok(inner
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect());
        }
    }
    Err("front matter missing tools:".into())
}

/// Capability-token secret broker: mint scoped one-shot tokens; workers present
/// tokens, never raw creds. Tokens carry a short TTL; `checkout` materializes a
/// 0600 token file under the lane's auth dir for harnesses that need a file.
#[derive(Debug)]
pub struct SecretBroker {
    tokens: HashMap<String, (String, bool, u64)>, // token -> (scope, consumed, expires_at_secs)
    base: String,
}

/// Short TTL for materialized capability handles (10 minutes).
pub const SHORT_TTL_SECS: u64 = 600;

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Default for SecretBroker {
    fn default() -> Self {
        Self {
            tokens: HashMap::new(),
            base: ".".into(),
        }
    }
}

impl SecretBroker {
    pub fn with_base(base: &str) -> Self {
        Self {
            tokens: HashMap::new(),
            base: base.into(),
        }
    }

    pub fn mint(&mut self, scope: &str) -> String {
        let tok = format!("cap-{}-{}", scope, self.tokens.len());
        self.tokens.insert(
            tok.clone(),
            (scope.into(), false, now_secs() + SHORT_TTL_SECS),
        );
        tok
    }

    /// One-shot redeem: second use fails closed; expired tokens fail closed.
    pub fn redeem(&mut self, token: &str, scope: &str) -> Result<(), String> {
        match self.tokens.get_mut(token) {
            Some((s, consumed, exp)) if s == scope && !*consumed && *exp >= now_secs() => {
                *consumed = true;
                Ok(())
            }
            _ => Err("invalid or consumed capability token".into()),
        }
    }

    /// Materialize a 0600 token file for a lane: `{profile_dir}/auth/<sha256(token)>`.
    /// The file carries no lifetime of its own — the broker TTL still governs.
    pub fn checkout(&mut self, token: &str, lane: &str) -> Result<std::path::PathBuf, String> {
        let entry = self
            .tokens
            .get(token)
            .ok_or_else(|| "invalid or consumed capability token".to_string())?;
        if entry.1 || entry.2 < now_secs() {
            return Err("invalid or consumed capability token".into());
        }
        let dir = std::path::Path::new(&self.base)
            .join("profiles")
            .join(lane)
            .join("auth");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let mut h = Sha256::new();
        h.update(token.as_bytes());
        let path = dir.join(hex::encode(h.finalize()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&path)
                .map_err(|e| e.to_string())?;
            use std::io::Write;
            f.write_all(token.as_bytes()).map_err(|e| e.to_string())?;
        }
        #[cfg(not(unix))]
        {
            std::fs::write(&path, token.as_bytes()).map_err(|e| e.to_string())?;
        }
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parity_catches_phantom() {
        let pack = RolePack {
            name: "coder".into(),
            version: "1".into(),
            tools: vec!["read".into(), "ghost".into()],
            skill_lockfile_hash: RolePack::lockfile_hash("skills"),
            system_prompt: String::new(),
            mcp_manifest: default_coder_manifest(),
            model_slot: "coder.primary".into(),
            harness_profile: HarnessProfile::default(),
            spawns: SpawnPolicy::Isolated,
            output_schema: None,
        };
        assert!(pack.check_parity(&["read".into()]).is_err());
        assert!(pack.check_parity(&["read".into(), "ghost".into()]).is_ok());
    }

    #[test]
    fn tokens_are_one_shot() {
        let mut b = SecretBroker::default();
        let t = b.mint("write:a.rs");
        assert!(b.redeem(&t, "write:a.rs").is_ok());
        assert!(b.redeem(&t, "write:a.rs").is_err());
    }

    fn catalog() -> Vec<String> {
        crate::mcp::catalog().into_iter().map(|t| t.id).collect()
    }

    #[test]
    fn flagship_pack_is_coherent() {
        let pack = RolePack::coder_web();
        let cat = catalog();
        pack.check_parity(&cat).unwrap();
        pack.check_manifest().unwrap();
        pack.check_no_raw_secrets().unwrap();
        assert_eq!(pack.model_slot, "coder.primary");
        let dir = tempfile::tempdir().unwrap();
        pack.save_pack(dir.path(), &cat).unwrap();
        let back = RolePack::load_pack(dir.path()).unwrap();
        assert_eq!(back.name, "coder-web");
        assert_eq!(back.tools, pack.tools);
    }

    #[test]
    fn pack_rejects_raw_secrets() {
        let mut pack = RolePack::coder_web();
        pack.system_prompt = "key sk-live-0123456789abcdef".into();
        assert!(pack.check_no_raw_secrets().is_err());
    }

    #[test]
    fn spawn_never_escalates() {
        let parent = vec!["read".to_string(), "code_search".to_string()];
        let pol = SpawnPolicy::Supervised {
            inheritable_scopes: vec!["read".to_string()],
        };
        assert!(pol.request(&["read".to_string()], &parent).is_ok());
        // write is neither inheritable nor held: typed rejection, not downgrade.
        let err = pol.request(&["write".to_string()], &parent).unwrap_err();
        assert!(err.reason.contains("write"));
        assert!(SpawnPolicy::Isolated
            .request(&["read".to_string()], &parent)
            .is_err());
    }

    #[test]
    fn lock_detects_drift() {
        let pack = RolePack::coder_web();
        let mut assets: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut contents: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut skills = HashMap::new();
        skills.insert("tdd".to_string(), hash_asset("test first"));
        assets.insert("skills".to_string(), skills.clone());
        let mut cskills = HashMap::new();
        cskills.insert("tdd".to_string(), "test first".to_string());
        contents.insert("skills".to_string(), cskills);
        let lock = compute_lock(&pack, &assets, &contents).unwrap();
        let dir = tempfile::tempdir().unwrap();
        pack.save_pack(dir.path(), &catalog()).unwrap();
        write_lock(dir.path(), &lock).unwrap();
        verify_lock(dir.path(), &contents).unwrap();
        // Drift the content: locked verification fails closed.
        contents
            .get_mut("skills")
            .unwrap()
            .insert("tdd".to_string(), "changed".to_string());
        assert!(verify_lock(dir.path(), &contents).is_err());
    }

    #[test]
    fn managed_block_preserves_outside_text() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("AGENTS.md");
        std::fs::write(&p, "# Project\nHuman notes here.\n").unwrap();
        upsert_managed_block(&p, "rolepack", "pack: coder-web").unwrap();
        upsert_managed_block(&p, "rolepack", "pack: coder-web v2").unwrap();
        let body = std::fs::read_to_string(&p).unwrap();
        assert!(body.contains("Human notes here."));
        assert!(body.contains("pack: coder-web v2"));
        assert!(!body.contains("pack: coder-web\n"));
        assert_eq!(body.matches("studio:rolepack start").count(), 1);
    }

    fn assert_pack_coherent(pack: &RolePack, cat: &[String]) {
        // Acceptance: parity + manifest + secret checks green per pack.
        pack.check_parity(cat).unwrap();
        pack.check_manifest().unwrap();
        pack.check_no_raw_secrets().unwrap();
        // Save/load roundtrip preserves the pack.
        let dir = tempfile::tempdir().unwrap();
        pack.save_pack(dir.path(), cat).unwrap();
        let back = RolePack::load_pack(dir.path()).unwrap();
        assert_eq!(back.name, pack.name);
        assert_eq!(back.tools, pack.tools);
        assert_eq!(back.model_slot, pack.model_slot);
        // Emit + verify across all 3 targets.
        emit_pack(dir.path(), pack).unwrap();
        verify_emitted(dir.path(), pack, cat).unwrap();
        // Lock pinned + drift-detecting.
        let mut assets: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut contents: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut skills = HashMap::new();
        skills.insert(
            "profile".to_string(),
            hash_asset(&format!("{}-skills", pack.name)),
        );
        assets.insert("skills".to_string(), skills.clone());
        let mut cskills = HashMap::new();
        cskills.insert("profile".to_string(), format!("{}-skills", pack.name));
        contents.insert("skills".to_string(), cskills);
        let lock = compute_lock(pack, &assets, &contents).unwrap();
        write_lock(dir.path(), &lock).unwrap();
        verify_lock(dir.path(), &contents).unwrap();
    }

    #[test]
    fn pi_library_packs_are_coherent() {
        let cat = catalog();
        let packs = RolePack::pi_library();
        assert_eq!(packs.len(), 5);
        let mut names: Vec<&str> = packs.iter().map(|p| p.name.as_str()).collect();
        names.sort();
        assert_eq!(
            names,
            vec![
                "coder-web",
                "planner-spec",
                "qa-tests",
                "researcher-docs",
                "reviewer-security"
            ]
        );
        for pack in &packs {
            assert_pack_coherent(pack, &cat);
        }
    }

    #[test]
    fn pi_library_spawns_never_escalate() {
        for pack in RolePack::pi_library() {
            // No pack may grant write-class scopes to children beyond Isolated,
            // and Supervised packs inherit read-only scopes only.
            match &pack.spawns {
                SpawnPolicy::Isolated => {}
                SpawnPolicy::Supervised { inheritable_scopes } => {
                    for s in inheritable_scopes {
                        assert!(
                            ["read", "code_search", "plan_open"].contains(&s.as_str()),
                            "{} inherits write-class scope {s}",
                            pack.name
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn engineering_batch_meets_flagship_bar() {
        let cat = catalog();
        let packs = RolePack::engineering_batch();
        assert_eq!(packs.len(), 10);
        let mut names: Vec<&str> = packs.iter().map(|p| p.name.as_str()).collect();
        names.sort();
        assert_eq!(
            names,
            vec![
                "cloud-k8s-architect",
                "compiler-tooling-engineer",
                "dba-data-engineer",
                "firmware-embedded-engineer",
                "fullstack-ts-engineer",
                "go-services-engineer",
                "mobile-engineer",
                "python-backend-engineer",
                "rust-systems-engineer",
                "systems-architect",
            ]
        );
        for pack in &packs {
            assert_pack_coherent(pack, &cat);
            // No new catalog tools: every tool must already be registered.
            for t in &pack.tools {
                assert!(cat.contains(t), "{} uses unregistered tool {t}", pack.name);
            }
        }
    }

    #[test]
    fn qa_batch_meets_flagship_bar() {
        let cat = catalog();
        let packs = RolePack::qa_batch();
        assert_eq!(packs.len(), 5);
        let mut names: Vec<&str> = packs.iter().map(|p| p.name.as_str()).collect();
        names.sort();
        assert_eq!(
            names,
            vec![
                "accessibility-auditor",
                "perf-qa-engineer",
                "qa-director",
                "security-auditor",
                "test-synthesizer",
            ]
        );
        for pack in &packs {
            assert_pack_coherent(pack, &cat);
            for t in &pack.tools {
                assert!(cat.contains(t), "{} uses unregistered tool {t}", pack.name);
            }
        }
    }

    #[test]
    fn emit_verify_all_targets() {
        let pack = RolePack::coder_web();
        let cat = catalog();
        let dir = tempfile::tempdir().unwrap();
        emit_pack(dir.path(), &pack).unwrap();
        verify_emitted(dir.path(), &pack, &cat).unwrap();
        // Drift one target: verification fails.
        let omp = dir.path().join("agents/coder-web.md");
        let mut body = std::fs::read_to_string(&omp).unwrap();
        body = body.replace("tool_open", "ghost-tool");
        std::fs::write(&omp, body).unwrap();
        assert!(verify_emitted(dir.path(), &pack, &cat).is_err());
    }
}
