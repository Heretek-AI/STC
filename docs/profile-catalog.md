# Pi Profile Catalog (issue #13)

Five task profiles as `RolePack`s (`RolePack::pi_library()` in
`studio-core/src/roles/mod.rs`). Authored once against the omp
`AgentDefinition` contract; emitted to 3 targets via `emit_pack`
(omp drop + base-pi shim + `.opencode` mirror); pinned via
`rolepack.yaml/.lock + verify_lock`.

Provenance: prompts adapted from `review/STC_ROLEPACK_SPECIFICATION.md`
(§0 shared conventions + §§1–5 role text, hand-reviewed templates).
Bodies are native STC authoring, not copied peer content — stub origins:
none beyond the spec; no unverified-template-origin material.

## Profiles

| Profile | Purpose | Model slot | Harness | Tier rationale |
|---|---|---|---|---|
| coder-web | Flagship web coding (implement declared task, evidence before claims) | `coder.primary` | omp | Flagship tier; auto-routing forbidden |
| reviewer-security | OWASP/STRIDE review, severity-rated file:line findings (§2.2) | `reviewer` | pi | Workhorse tier; auto-routing forbidden |
| researcher-docs | Source-of-record research + docs authoring (§4.5) | `researcher` | pi | Economy tier; pool fallback allowed |
| planner-spec | Specs/RFCs/interfaces, proposal-only, never implementation (§1.1) | `planner` | pi | Economy tier; pool fallback allowed |
| qa-tests | Failing-test-first synthesis, red→green demonstrated (§2.3) | `qa` | pi | Workhorse tier; pool fallback allowed |

## $/task routing note (pi-first justification)

Pi is the custom-agent substrate (most configurable) and the cheapest
adequate runner for bounded profiles: reviewer/researcher/planner/qa prompts
are narrow enough for economy/workhorse tiers, reserving flagship spend for
`coder.primary` execution. Measured $/task per profile is pending live-model
measurement (blocked:credentials) — when credentials exist, record routing
decisions from the token ledger (`billed_kind` per session) and revisit tiers.

## Verification

```bash
cargo test -p studio-core roles   # parity + manifest + secrets + roundtrip + emit/verify + lock per pack
```

`pi_library_packs_are_coherent` asserts all five packs pass
`check_parity + check_manifest + check_no_raw_secrets`, survive save/load
roundtrip, emit + verify across all 3 targets, and verify `--locked`.
`pi_library_spawns_never_escalate` asserts no write-class inherited scopes.

## #12 batch 1: engineering remainder (spec §1)

Same flagship bar via `RolePack::engineering_batch()` +
`engineering_batch_meets_flagship_bar` (coherence + no unregistered tools).

| Pack | Slot | Harness | Tool lens |
|---|---|---|---|
| systems-architect | `architect` | omp | read, code_search, tool_open, kv_get, plan_open, dag_commit, retrieve_docs |
| rust-systems-engineer | `coder.primary` | opencode | coder lens + syntax_check |
| fullstack-ts-engineer | `coder.primary` | opencode | coder lens + syntax_check |
| python-backend-engineer | `coder.primary` | pi | coder lens |
| go-services-engineer | `coder.primary` | pi | coder lens |
| firmware-embedded-engineer | `coder.primary` | pi | coder lens |
| mobile-engineer | `coder.primary` | pi | coder lens |
| compiler-tooling-engineer | `coder.primary` | pi | coder lens |
| dba-data-engineer | `coder.primary` | pi | coder lens |
| cloud-k8s-architect | `architect` | omp | coder lens + retrieve_docs |

Coder lens = read, write, edit, patch, runProcess, code_search, tool_open,
kv_get (Coder manifest + search/syntax OnDemand, coder_web precedent).
Provenance: prompts authored natively from the spec role text; no new catalog
tools (H-C gate respected); systems-architect Supervised[read, plan_open],
all others Isolated.

## #12 batch 2: QA/security remainder (spec §2)

Same flagship bar via `RolePack::qa_batch()` +
`qa_batch_meets_flagship_bar`.

| Pack | Slot | Harness | Tool lens |
|---|---|---|---|
| qa-director | `reviewer` | omp | read, code_search, tool_open, kv_get, syntax_check, sast, sbom (reviewer lens, Supervised[read, code_search]) |
| security-auditor | `reviewer` | pi | read, code_search, tool_open, kv_get, sast, syntax_check (reviewer lens, Isolated) |
| test-synthesizer | `coder.primary` | pi | coder lens (Isolated) |
| perf-qa-engineer | `reviewer` | pi | read, runProcess, code_search, tool_open, kv_get (Researcher manifest, Isolated) |
| accessibility-auditor | `reviewer` | pi | read, code_search, tool_open, kv_get, web_search (Researcher manifest, Isolated) |

Reviewer lens = Reviewer manifest + code_search OnDemand. Provenance: prompts
authored natively from the spec role text; no new catalog tools.

## #12 batch 3: creative + management + operations (spec §3–§5)

Same flagship bar via `RolePack::creative_management_ops_batch()` +
`creative_management_ops_batch_meets_flagship_bar`. Shared spec constraints
(source-of-record/asset budgets for creative; owner + acceptance +
deadline-tick directives for management; approval objects for ops mutations)
live in the prompts.

| Pack | Slot | Harness |
|---|---|---|
| blender-tech-artist, shader-specialist, godot-gameplay-programmer, procedural-mesh-generator, audio-designer, tooling-asset-pipeline-engineer | `creative` | pi |
| product-manager, scrum-dispatcher | `manager` | omp |
| pr-gatekeeper | `reviewer` | omp |
| release-engineer, tech-writer, devops-sre, token-economist, knowledge-curator | `manager`/`researcher` | pi |
| incident-commander | `manager` | omp |

Tool lenses: creative on coder lens; management on Manager manifest
(release-engineer +runProcess, devops-sre +runProcess/code_search via the
`extended()` helper); tech-writer/knowledge-curator on Researcher +write/edit.
Provenance: prompts authored natively from the spec role text; no new catalog
tools (H-C gate respected).
