# Studio — Findings & Report

**Project:** Studio — multi-agent autonomous software engineering studio and orchestration engine
**Date:** 2026-09-26
**Author:** Claude (Principal Systems Architect pass)
**Status:** Research complete. Findings only — no application code generated.
**Design/plan companion:** `plans/claude/studio-architecture-report.md`
**Raw research:** `.research/brainstorm_20260926T203716Z.md` (12-repo audit + dialectic I), `.research/brainstorm_20260926T211427Z.md` (open questions + dialectic II)

Epistemic convention: `[VERIFIED path]` = read in corpus or named source · `[INFERRED: parents]` = synthesis from named evidence · `[HYPOTHESIS: <test>]` = falsifiable speculation with a kill criterion.

---

## 1. Scope of this document

Two research passes, consolidated:

- **Pass 1 — Forensic audit.** 12 repositories in `/home/john/Projects/STC/review` read at source level (not README-only) across four parallel audits, plus the 300-entry ecosystem catalog. Purpose: find what exists, what works, what is broken.
- **Pass 2 — Open questions.** Community sentiment, multimodal/creative work, computer use, long-term memory, the find↔return loop, completion discipline, agent templates, OAuth, multi-profile identity. Purpose: answer the questions the corpus does not cover.

This document is **findings**. Decisions, ADRs and roadmap live in the companion architecture report.

---

## 2. Hard measurements (the numbers that should drive design)

These are the facts that most constrain the architecture. Each is verified to a named source.

| # | Measurement | Source | Design consequence |
|---|---|---|---|
| 1 | **118 tools ≈ 311 KB ≈ 80K tokens vs. 8 script tools ≈ 8.8 KB ≈ 2.2K tokens — a ~35× cut** | `agent-swarm/thoughts/shared/research/2026-07-11-scripts-only-mcp-experiment.md` `[VERIFIED]` | Progressive tool disclosure is the highest-ROI item in the product |
| 2 | Only Claude has ToolSearch; "pi/codex/opencode have no tool-search, so for them the full schema really does enter every session" | same `[VERIFIED]` | Cannot rely on the harness to fix tool bloat — the gateway must |
| 3 | **Three subagents ≈ 4× the token spend of a single-thread session** (each runs its own API requests in a fresh window) | youcanbuildthings.substack.com `[VERIFIED]` | Naive fan-out is expensive; coordination must not itself use models |
| 4 | `git worktree remove` of a multi-GB checkout **blocks the IPC thread 8–35s** | `orca/src/main/worktree-trash.ts:1-5` `[VERIFIED]` | Teardown must be rename-then-async-sweep, never inline delete |
| 5 | A never-delete memory bug wrote **100GB in 8 hours** | `munder-difflin/src/main/palaceReap.ts:1-30` `[VERIFIED]` | Memory stores need retention + reapers from day one |
| 6 | A cumulative-counter reset **hid 59% of real lifetime spend** | `munder-difflin/src/main/costLifetime.ts:1-40` `[VERIFIED]` | Token accounting must be an append-only ledger with segment-peak fold |
| 7 | 6 tools were listed in config but never registered, and shipped | `opencode-swarm/docs/engineering-invariants.md` v6.48.0 `[VERIFIED]` | Tool lists must be derived from a compile-checked manifest + parity tests |
| 8 | Synchronous `git commit` per routed message on the Electron main thread | `munder-difflin/src/main/hive.ts:1549-1553, 2683-2686` `[VERIFIED]` | Never put git or I/O coordination on the hot path |
| 9 | `HOP_CAP = 12` is checked against `hops` — which is **never incremented** | `munder-difflin/src/main/hive.ts:65, 1531, 1557` `[VERIFIED]` | Every counter needs a test proving it increments |
| 10 | Anthropic's own postmortem: caching bug + prompt changes → **~3% quality drop**, felt as "less capable, more repetitive" | aiforanything.io / shattered.io roundups `[VERIFIED]` | Quality is unstable across versions; pin harness + model versions |
| 11 | Prepending a ~150–250 token citation instruction on **every** Edit/Write/Bash | `claude-smart/plugin/src/claude_smart/context_inject.py:186-196` `[VERIFIED]` | Budget every injected byte; fixed per-turn taxes compound |

---

## 3. Corpus findings

### 3.1 What the landscape actually is

300+ projects catalogued in `awesome-agent-orchestrators`, organized by **human supervision modality** (parallel terminal · parallel desktop/web · multi-agent swarms · autonomous loop runners · autonomous task runners · infrastructure & primitives · personal assistants). 12 audited at source: agent-of-empires, agent-orchestrator, agent-swarm, claude-smart, iPolloWork, munder-difflin, oh-my-pi, opencode-swarm, orca, Orkas, paseo, awesome-agent-orchestrators.

They split into three tribes:

| Tribe | Members | Strong at | Weak at |
|---|---|---|---|
| Session managers | AoE, orca, paseo, AO | worktree lifecycle, harness wrapping, multi-surface UI | daemon owns all truth; worktree-per-agent disk explosion |
| Coordination & gates | opencode-swarm, agent-swarm | gated review, scope enforcement, write detection | LLM-heavy coordination burns the budget the work needed |
| Runtimes & learning | oh-my-pi, claude-smart, munder-difflin, iPolloWork, Orkas | tool/IDE depth, skill learning, runtime pluggability | weak isolation; unbounded context growth |

### 3.2 Four structural findings

**F1 — Decorative orchestration is the corpus's central failure mode.**
iPolloWork declares `relations: dependency | parallel` and **never walks the graph** `[VERIFIED packages/types/src/project-workspace.ts:88-110]`. agent-orchestrator has **no dependency concept at all** in its domain layer. Orkas **deleted** its plan DAG in favor of prompt-driven dispatch (`plan_executor.ts:1-16`, "G8b replaced static plans"). Only agent-swarm has a real `dependsOn` queue — and it has **zero referential integrity** (`dependsOn` IDs are arbitrary strings, `JSON.parse`d on read).
*Consequence:* a DAG that no deterministic scheduler executes is worse than no DAG. Studio's DAG must be machine-executed or not shipped.

**F2 — Progressive tool disclosure already exists in production.**
`oh-my-pi`'s `xd://` virtual devices demote rarely-used tools out of the request schema and expand on demand: `read xd://` (listing), `read xd://<tool>` (docs + schema), `write xd://<tool>` (execute), with `ToolLoadMode = "essential" | "discoverable"` and a pinned never-demote set `[VERIFIED packages/coding-agent/src/tools/xdev.ts:1-49, packages/agent/src/types.ts:984-1091]`. `Orkas` independently ships lazy `tool_load` group activation with `ownerAgent` isolation and "loading never grants" permission `[VERIFIED src/main/model/core-agent/tool-catalog.ts:11-237]`.
*Consequence:* Studio's Tool Lens generalizes a proven pattern; it does not invent one. Its novelty is doing it fleet-wide with role manifests and an ACP permission seam.

**F3 — Nobody solves write conflicts at the semantic layer.**
Everyone ships worktrees, then a merge queue to undo the damage. The three primitives to do better exist **separately**: `wit` (Tree-sitter function locks), `foremerge` (declare intent + semantic scope before writing), `Zaivern` (line-level ownership) `[VERIFIED awesome-agent-orchestrators/README.md:304, 229, 129]`. In-repo, `opencode-swarm` has scope leases with TTL + **shell-write detection as static analysis** (bash-parser AST covering `redirect | here_doc | builtin_write | inplace_edit | interpreter_eval | network_download | archive_extract | git_destructive`) `[VERIFIED src/hooks/shell-write-detect.ts:35-43]` — and admits it is "a shared bounded tripwire, never a sandbox."
*Consequence:* this is Studio's defensible gap. Semantic write-intent claims are the bet.

**F4 — Prompt-text scoping is not enforcement.**
iPolloWork renders `skillIds`/`pluginIds` into a system prompt ("Assigned plugins: …") and filters nothing `[VERIFIED packages/types/src/work-items.ts:109-127]`. Paseo's tool policy is a flat `disabledTools` string list. claude-smart scopes by hook matcher only.
*Consequence:* capability must be enforced at a gateway (resolution time), not advertised in a prompt.

### 3.3 What each project is actually best at

| Project | Best thing it does | Steal this |
|---|---|---|
| **oh-my-pi** | Context economics | `xd://` tool mounting; `StablePrefix`/`AppendOnlyLog` cache stability; `CompactionEntry` + tool-protection matchers; native per-family tokenizers; `MemoryRow` lifecycle |
| **orca** | Worktree lifecycle | trash-rename deferred delete; `--detach --no-checkout` → `reset --hard` → `lock`; mutation-receipt dedupe; nested-depth gate with actionable refusal |
| **agent-of-empires** | Process/session hygiene | lease/epoch supervisor with RAII resume guard; three-tier env-deny matrix; worktree `lock --reason` prune fence; event retention with substantive/non-substantive split |
| **opencode-swarm** | Gate machinery | shell-write-detect; `WriteApprovalFactV1` one-shot approval ledger; spec-hash staleness; derived `TOOL_METADATA` + parity tests; `engineering-invariants.md` practice |
| **agent-orchestrator** | Review correctness | **head-SHA-fenced feedback delivery**; fact-derived Kanban (never persisted); speculative worktree prepare/claim tokens; content-addressed reviewer manifest |
| **agent-swarm** | Stall/dag handling | 3-class stall taxonomy + bounded resume budget; dependency-aware queue-stall SQL; offer/claim + stale-offer release; measured scripts-only MCP |
| **munder-difflin** | Messaging + cache discipline | speech-act message schema; prompt-cache-stable prefix rule; capability-token secret broker; `workTokensOf` vs `tokensOf` |
| **Orkas** | Delegation contract | `hand_off_to` / `dispatch_to` / `run_worker`; `LocalCliCapabilities`; model-unwritable task board |
| **paseo** | Extensibility hygiene | zod-validated plugin before-hooks; import-graph boundary tests; capability-token MCP injection; 3-layer context-flood stack |
| **iPolloWork** | Plugin portability | manifest v2 (`requires`/`provides`, auth methods, engine bindings); engine adapter with `ready/partial/unsupported` matrix |
| **claude-smart** | Attribution | idempotent publish batching (uuid5 request ids); citation registry with counterfactual cite-bar |

### 3.4 Anti-patterns, each verified in source

1. Prompt-text scoping instead of enforcement — iPolloWork
2. Decorative DAGs — iPolloWork, AO, Orkas
3. Fixed per-turn injection tax — claude-smart
4. Coordination on the hot path — munder-difflin
5. Un-incremented loop guards — munder-difflin `HOP_CAP`
6. Unbounded append-only files — munder-difflin `cost-ledger.jsonl`
7. Enumerated lists drifting from registrations — opencode-swarm
8. God modules — Orkas `bus.ts` (10,751 lines), paseo `agent-manager.ts` (5,363), opencode-swarm `index.ts` (6,271)
9. Best-effort disk hygiene — AoE ("cosmetic leak"), AO (platform-forked removal repair)
10. String-matched control flow — agent-swarm `failureReason?.includes("Blocked dependency")`

---

## 4. Community findings

Directional, not statistical. Sources are roundups, postmortems and tooling ecosystems rather than a controlled survey.

**Liked:** worktrees as relief from parallel-agent chaos; code quality; the MCP ecosystem; the `CLAUDE.md`/skills culture.

**Disliked (ranked by recurrence):** token cost compounding · subagent fan-out burn · quality instability · profile/setup friction.

**Missing everywhere** (the whitespace):
- semantic write-conflict prevention
- zero-token coordination
- **completion discipline** — nobody solves the "agent stops at 80% and suggests a next step" failure
- pre-declared token budgets as contracts (vs. meters you look at after)
- creative/multimodal lanes as first-class (they're bolted on: Orkas' nine specialists, iPolloWork's six output modalities)

---

## 5. Open-question findings

### 5.1 Multimodal and creative work — the artifact-first rule

The unifying answer to image / video / 3D / desktop / web is: **never let a creative agent return a finished binary. Return the source of record; render deterministically.**

| Capability | Works today | Still unreliable | Pattern |
|---|---|---|---|
| Images | OpenAI/Gemini image APIs, Replicate, fal.ai, Ideogram, Flux, ComfyUI | iterating a raster | **HTML/CSS/SVG-first**; raster only as export (matches iPolloWork "editable production loop", Orkas ImageStudio) |
| Video | FFmpeg, Remotion (React→video), TTS narration, caption/dub pipelines | long-form generative | **Coded timeline as source of record**; generative shots are inserts |
| 3D | Blender `bpy`, OpenSCAD, CAD APIs, text-to-3D (Meshy/Tripo/TRELLIS/Hunyuan3D) | text-to-3D gives meshes, not editable topology | **Parametric/Blender-script first**; text-to-3D is an *asset import* |
| Web apps | Playwright MCP (accessibility snapshots), Stagehand, browser-use | screenshot-vision is slow + token-hungry | **Structured automation first, pixels second** |
| Desktop apps | AX-tree MCP (Windows-MCP, Peekaboo), Anthropic computer use, Cua Driver | pure pixel control is flaky | **Accessibility tree primary** (AXUI/UIA/AT-SPI), pixels for canvas/legacy |

The 2026 consensus on browser/desktop is explicit: "the most reliable architecture is usually structured browser automation first, computer use second — use Playwright MCP when the application exposes stable semantics, and computer-use when a workflow depends on pixels, canvas, legacy interfaces." `[VERIFIED himat.tech]` and "accessibility-tree-based servers… remain the gold standard." `[VERIFIED dev.to]`

**Additional finding:** creative iteration is a hill-climb. Five parallel video drafts produce five mediocre videos; one video with five parallel *reviewers* produces one good video. `[INFERRED: I7 antithesis]` Serialize authoring, parallelize critique.

**Merge consequence:** binary artifacts must never text-merge. The merge queue takes the artifact whose source-of-record hash is on the winning side and re-renders.

### 5.2 Long-term memory — is a vector DB a good idea?

**Finding: as a retrieval component, yes. As an organizing architecture, no.**

The community evidence is unusually one-directional:
- "A vector database does one thing well: approximate nearest-neighbor search over embeddings… most agent-memory queries don't decompose to pure semantic similarity." `[VERIFIED hindsight.vectorize.io]`
- "For specific facts and shared state, a structured key-value approach is often more reliable and easier to maintain." `[VERIFIED n1n.ai]`
- "Vector search keeps failing teams who deploy it as a complete solution." `[VERIFIED mindstudio.ai]`
- "Vector search and knowledge graphs solve different halves of the memory problem… the half they are missing is the half that breaks in production." `[VERIFIED hindsight.vectorize.io]`

The corpus agrees, and the *best* memory system in it is **not** vector-first:
- `oh-my-pi` `MemoryRow` carries a statement-lifetime model — `veracity` (stated|inferred|tool|imported|unknown), `valid_until`, `superseded_by`, `trust_tier`, `recall_count`, `scope` — with retrieval across **four voices** (vector/graph/fact/temporal) combined into `combined_score` `[VERIFIED packages/mnemopi/src/types.ts:9-28, core/polyphonic-recall.ts:8-33]`
- `claude-smart` has no in-process vector store at all; it keeps a **citation registry** proving whether a memory was used
- `munder-difflin` needed a quarantine reaper after 100GB/8hrs

**Recommended shape — the Memory Ladder:**

| Layer | Contents | Retrieval | Cost |
|---|---|---|---|
| L0 | git + receipts + content-addressed blobs | `git log`, blob refs | ~0 |
| L1 | `studio.db` facts (decisions, budgets, claims, typed outcomes) | SQL + FTS5 | ~0 |
| L2 | markdown project memory (`MEMORY.md`, AGENTS.md, scope notes) | grep / FTS | ~0 |
| L3 | lifecycle memory rows (veracity/supersession/trust) | keyed + recency | low |
| L4 | **optional** semantic index for narrative prose | embeddings + **mandatory reranker** | opt-in per project |

Rules: L4 is opt-in and scoped · embeddings are versioned + re-derivable (`content_hash` + `embed_model`) · never store what git stores · reranker is mandatory if L4 ships.

### 5.3 The find↔return loop — Evidence Packets

**The failure mode:** a researcher reads 40 files, returns 800 words, and the implementer works from a paraphrase. Nothing can check grounding.

**Finding:** return an **EvidencePacket**, not a report — claims with verbatim quotes + locators, content-addressed `blob_ref`s readable via `evidence://`, `confidence` per claim, `superseded_by` retractions, and `open_questions`. Six rules: cite-or-don't-claim · immutable blobs · schema-first return · **disjoint role tools** (researchers never write, implementers never browse) · return-then-close is mandatory · `needs_evidence` is a first-class round-trip task.

**Key structural point:** the **coordination plane** moves packets. It is zero-token and structurally incapable of summarizing them away. Only the producing and consuming models touch content.

### 5.4 Completion discipline — the anti-tap-out problem

**Finding: this is the most under-addressed failure in the entire space.** CI solved it in 2010 (a PR merges on green checks; "here's what I'd do next" is not a pipeline state) and agent tooling regressed to chat. Partial pieces exist — `no_human` (second model told to refute "done" + tamper guard against net test reductions), `opencode-swarm` (evidence-backed completion gates, spec-hash staleness), ralph-* exit detection — but nobody composes them.

**The Completion Contract (7 mechanisms, only one uses a model):**
1. DoD is declared at DAG-commit and **machine-checkable** — a task without one is not dispatchable
2. `done` is a typed receipt requiring evidence; proposing future work is a **`blocked`** receipt
3. **Deterministic tap-out detector** — *future-work prose* + *no assertions satisfied* + *delta smaller than declared scope* → auto-rewind to `in_progress`
4. **Scope-completion ratio** gate (2 of 7 planned files = 0.29 → refuse)
5. **Tamper guard** — workers may not shrink the assertion set
6. **Adversarial refute-done** — a fresh model explicitly instructed to refute the claim of done
7. Budgets make early exit cost *more* (the task requeues under the same budget)

**One line:** *done is a property of evidence, not of language.*

### 5.5 Identity, OAuth, and multi-profile

**Finding: multi-profile is solved and the mechanism is `CLAUDE_CONFIG_DIR`.** Each profile is a complete isolated config directory — settings, credentials, MCP servers, history. A real ecosystem exists (`claude-code-profiles`, `ccm`, direnv-per-folder patterns). Out of the box, "switching means logging out and back in, and everything else stays mixed." `[VERIFIED github.com/CarlosTheory/claude-multi-account]` Codex uses `auth.json`. Orkas abstracts the difference behind `LocalCliCapabilities` (`resume: native|session-id|none`, `LocalCliPermissionPolicy`).

Notably, the corpus already treats it as *the* isolation knob: `agent-of-empires` pins `CLAUDE_CONFIG_DIR` in its spawn env-deny matrix so a child cannot migrate identity `[VERIFIED src/acp/acp_client/spawn.rs:86-90]`.

**Gotchas:** rate limits are **per-account**, so budgets must be per-account not per-agent · lock files in shared config dirs · transcripts keyed by cwd (munder-difflin seeds transcripts per target cwd to work around this).

**OAuth — two distinct problems:**
- *Studio as client:* OAuth 2.0 + PKCE with loopback redirect (RFC 8252); device-code flow for headless; OS keychain storage.
- *Agents acting for the user:* **never hand an agent a raw token.** Use a capability-token broker — munder-difflin's pattern is the strong one: "workers call `127.0.0.1/i/<id>/<path>` with a per-worker handle; the broker injects the real credential upstream; the worker never sees it." `[VERIFIED src/main/integrationBroker.ts]` iPolloWork's manifest v2 auth taxonomy (`secret-form | oauth-pkce | device-code | hosted-browser`) and capability-bound plugin authorization are the right registry shape. Every OAuth'd write consumes a **one-shot approval fact**.

### 5.6 Agent templates

**Finding: yes, ship them.** Free model tiers (OpenCode Go) make the runtime nearly free; the bottleneck is authoring configuration. pi/oh-my-pi's configurability is a wall for new users. Ship versioned `RolePack`s (role, tool manifest, `tool_load_mode`, harness profiles, `returns:` type, budget) with **skill lockfiles** and **registration parity tests** — the phantom-tool bug class is the reason.

Recommended packs: `manager` · `researcher` · `coder` · `auditor` · `creative-image` · `creative-video` · `creative-3d` · `qa-web` · `qa-desktop` · `docs` · `release`.

### 5.7 Human feedback from UI/UX

**Finding: you don't need general human feedback. You need three decisions — scope, budget, taste.** Everything else is a machine-checkable assertion.

- **Decision prompts** (rare, high friction) → ACP `session/request_permission`, consolidated into one **approval inbox** (Calyx/octomux pattern) so the operator isn't tab-hopping.
- **Taste/design feedback** (cheap, continuous) → findings anchored to artifact regions (SVG node, video timeline range, slide), routed back to the originating session **head-SHA-fenced**. Preferences are *separate* durable rows from findings, with scope + supersession.
- **Structured forms** → steal Orkas' `input_channel: 'form' | 'prose'` for scope negotiation, budget confirmation, and design briefs.
- **Voice caution:** destructive verbs need a distinct-token confirm — "NEVER a bare `yes`, so ambient speech can't authorize a kill." `[VERIFIED munder-difflin realtimeActions.ts:12-122]`

---

## 6. Consolidated recommendations

Ranked by leverage and by how much verified evidence supports them:

| # | Recommendation | Evidence base | Risk |
|---|---|---|---|
| 1 | **Progressive tool disclosure (Tool Lens)** with role manifests + ACP permission seam | 35× measured; `xd://` + `tool_load` both exist | Low — proven pattern |
| 2 | **Zero-token coordination plane** (no model in dispatch/claim/merge) | 3-subagent 4× burn; `bernstein` ecosystem precedent | Medium — must prove completion rate holds |
| 3 | **Write-intent fabric** (semantic claims) with worktree fallback | ecosystem primitives exist uncombined | Medium-High — the central bet |
| 4 | **Completion Contract** (evidence-bearing `done`, tap-out detector, refute-done) | nobody solves it; CI precedent | Low-Medium |
| 5 | **Memory Ladder** (structured-first, vector opt-in + reranker) | community consensus + corpus best practice | Low |
| 6 | **Evidence Packets** for find↔return | cite-bar proven; content-addressed blobs already planned | Low |
| 7 | **Profile-as-Code** via `CLAUDE_CONFIG_DIR` + capability broker | mechanism verified; AoE already pins it | Low |
| 8 | **Artifact-first creative lanes** + render farm | iPolloWork/Orkas convergent; merge semantics clear | Medium |
| 9 | **Accessibility-tree first** for desktop/web, pixels second | 2026 consensus | Low |
| 10 | **`RolePack` templates** with lockfiles + parity tests | free-tier economics make authoring the bottleneck | Low |

---

## 7. Open loops

| # | Question | Resolve by |
|---|---|---|
| 1 | Does the Completion Contract cut tap-out from >40% to <10% at <20% added cost? | Spike (kill if <15pt reduction or >40% cost) |
| 2 | Do EvidencePackets double grounding rate vs. prose summaries? | Spike (kill if <1.3× or researcher time doubles) |
| 3 | Does Memory Ladder L0–L3 answer >70% of queries with no embeddings? | Spike (kill if <50% → build L4 now) |
| 4 | Does function-level write-intent locking beat worktrees on cost *and* success? | Spike (kill on any conflict rate >0 for non-overlapping claims) |
| 5 | Does zero-token coordination match manager-agent dispatch on completion + deadlock? | Spike (kill if completion drops >5%) |
| 6 | Does progressive disclosure beat full-schema at 50+ tools? | Spike (must approach the 35× floor) |
| 7 | Profile-as-Code: zero credential cross-talk across 5 agents / 2 accounts, setup <2 min | Spike |
| 8 | Which formats are genuinely source-of-record for this team? | Format spike (SVG / PPTX / Remotion / OpenSCAD) |
| 9 | MCP authorization spec maturity for desktop clients | Research before Phase 1 |
| 10 | Bun process/PTY at 20 concurrent agents (p99 < 50ms) | ADR-001 revisit trigger |

---

## 8. Bottom line

Everyone in this space is solving "run agents in parallel and review diffs." The verified evidence points to three things nobody combines, and they are all cheaper than what the field is currently doing:

1. **Prevent the conflicts** rather than merge around them (semantic write-intent).
2. **Take models out of coordination** rather than spend tokens on dispatch (zero-token plane).
3. **Make "done" an evidence property** rather than a sentence (Completion Contract).

Add the two proven-but-uncomposed context tools — progressive tool disclosure (35× measured) and structured-first memory — and Studio has a defensible core that is simultaneously *simpler* and *cheaper* than the incumbents.

**Next action: run the six spikes in §7 before committing any roadmap.** Each is under a day; each has an explicit kill criterion. If they fail, the plan bends — not the schedule.
