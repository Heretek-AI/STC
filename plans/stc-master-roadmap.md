# STC — Master Plan & Roadmap

## Context

STC (codename **Studio**) is a greenfield, local-first autonomous AI engineering platform — a **Virtual Software Agency** — whose explicit goal is to exceed **Paseo, iPolloWork, and Orkas**. Three AI systems each audited the 12 reference codebases in `/home/john/Projects/STC/review/` and recorded findings/plans in `/home/john/Projects/STC/plans/` (Claude Code → `claude/`, Antigravity → `antigravity/`, opencode → `opencode/`), plus a domain model at `scratchpads/brainstorm_studio_arch/domain_model.json`. This master plan synthesizes those reports into one decision-complete build roadmap, resolving their architectural conflicts.

The product spans four pillars the roadmap must deliver end-to-end:
1. **Manager → DAG → dispatch** — a human-facing Manager negotiates scope into a typed, machine-executed task DAG, which dispatches Researchers / Coders / Reviewers.
2. **Deterministic git-worktree isolation** — parallel coders in isolated worktrees with crash-safe, idempotent create / stash / teardown / merge.
3. **Dynamic least-privilege MCP scoping** — role-sliced tool schemas to stop context-flood.
4. **High-density cockpit** — fleet board, live streams, diff-gate, MCP capability matrix.

**Resolved spine (decided for this plan):** a **Rust (Tokio) core** daemon/engine; **worktree-first isolation** with the semantic write-intent moat deferred behind a spike gate; **spike-first validation** before committing the build. This is a reliability-first posture: a robust single-binary daemon, proven isolation primitives, and cheap validation of the differentiating bets before they are load-bearing.

**Epistemic note:** reference-source anchors below (`repo/path`, symbols) are as cited in the agents' reports, which tagged them `[VERIFIED path:line]`. Line numbers drift; the implementer MUST re-locate each anchor by **symbol name** in the reference repo before porting. The three reports cross-corroborate the same patterns, so the patterns are reliable even where exact line anchors need re-checking.

---

## Approach — phased roadmap

Order is chosen so the tree builds and each phase's exit criteria are provable before the next begins. Phase 0 is the validation gate; its results decide whether the Phase 5 moat is built. Phases 1–4 are the four pillars. Phase 5 is the differentiator + expansion track. New module layout lives in a Rust workspace `studio-core/` (daemon, worktree engine, state, MCP gateway, scheduler) with a `cockpit/` (Tauri + React/Tailwind) and `tui/` (Ratatui) shell, and `adapters/` (ACP + CLI harness shims).

### Phase 0 — Validation spikes (gate the build; ~1 day each, kill criteria decide the roadmap)

Three bounded prototypes, each a throwaway that answers one question. Run before committing Phases 1–5.

- **H1 — Write-intent fabric (decides the Phase 5 moat).** Build a minimal Rust tree-sitter scope extractor → `scope_hash(file|fn|symbol)` + an SQLite claim broker; run 5 scripted parallel edits against **one shared tree** and check for collisions. **Kill criterion:** any conflict on non-overlapping claims, or task-success drop > 5% vs. a worktree baseline → **then Phase 5 stays worktree-only and the write-intent moat is dropped** (see Assumptions).
- **H2 — Zero-token coordination (validates Pillar 1).** Implement the scheduler + claim broker + merge queue as a pure state machine replaying a 20-task DAG, vs. a manager-agent dispatch baseline. **Kill criterion:** completion drop > 5% vs. baseline, or any deadlock the baseline avoided → then allow a manager model back into the dispatch loop for that step (see Assumptions).
- **H3 — Tool Lens (validates Pillar 3).** Build a schema-slicing MCP proxy and A/B it on a 50/100/200-tool suite. **Kill criterion:** fails to approach the measured **~35×** schema-token cut (118 tools ≈ 80K tokens → 8 script tools ≈ 2.2K tokens), or completion drop > 5% → then ship the **scripts-only** tool mode (the measured path) instead of progressive disclosure.

Each spike writes a one-page result (pass/fail vs. kill criterion) to `plans/spikes/`. No product code is merged in Phase 0.

### Phase 1 — Core engine (Rust/Tokio): worktree isolation + DAG dispatch (Pillars 1 & 2)

Create `studio-core` Rust workspace (Tokio runtime, `tracing`, `clap`). Reuse, do not reinvent:
- **Process/session supervision & event log** — adapt `agent-of-empires` (`src/process/`, `src/events/`, `src/git/worktree/`): lease/epoch supervisor with RAII resume guard, event retention split, worktree `lock --reason` + `prune` fence, mutations bounded (30s) / observations bounded (5s).
- **Ports-and-adapters + CDC + worktree lifecycle** — port `agent-orchestrator` (`backend/internal/ports/`, `cdc/`, `session_manager/`, `lifecycle/`) into Rust: `Workspace` port with `Create / Destroy(refuses dirty) / StashUncommitted→refs/preserved/<sess> / ApplyPreserved(cherry-pick --no-commit) / ForceDestroy(only after stash)`; `change_log(seq,table,row,op,old,new)` triggers → 100ms CDC poller (batch 512) → SSE fan-out; durable-facts-only, display derived at read.
- **Deferred delete + create discipline** — trash-rename then async bounded sweep (orca `worktree-trash.ts`: `git worktree remove` of a multi-GB checkout blocks the thread 8–35s; rename-to-trash is metadata-only); create via `git worktree add --detach --no-checkout` → `reset --hard` → `lock --reason`.
- **Hash-chain plan ledger** — `.swarm/plan-ledger.jsonl` authoritative (`seq`/hash-chain/source), `plan.json` projection (opencode-swarm `src/plan/ledger.ts`).

Concrete behavior:
- **`WorktreeManager`** with a per-repo `WorktreeOperationLock`, hashed path layout `<root>/<hash>/<slug>`, and setup/teardown hooks. State machine `provisioning→active→dirty-check→stash→rebase/merge→verify→teardown`; removal is **fail-closed** (dirty/blocked surfaces, never force-delete without captured stash + waiver). Crash recovery: `git worktree list --porcelain` ∖ ledger = orphans → `prune`. Identity is `locator(repoId::path)` ≠ `occupant(wt2:host:instance)`.
- **`CONFLICT_RETAINED` merge outcome** (antigravity ADR-003): on merge conflict the worktree is kept intact and a structured diagnostic goes to the Lead/Auditor — never a destructive `git reset`/delete on parallel-coder work.
- **Typed DAG artifact + scheduler.** Manager emits a typed DAG (`WorkflowDefinition{nodes[]{id,type,config,next,inputs:upstream-only,inputSchema,retry}}`; single entry; every node reachable). Give `depends_on` **referential integrity** (the exact gap in agent-swarm, whose `dependsOn` IDs are arbitrary strings). Scheduler + claim broker + lease + heartbeat dispatch **with no model in the loop** (validated by H2). Roles: `Manager`, `Researcher`, `Coder`, `Reviewer`.
- **State store** — event-sourced SQLite (rusqlite, WAL, `BEGIN IMMEDIATE`, `afterCommit`): tables `tasks(id,parent_id,kind,intent_hash,scope_hash,status,depends_on)`, `claims`, `events`, `receipts`, `token_ledger`, `merges`. Token accounting is an append-only **ledger, not a counter** (a cumulative-counter reset once hid 59% of lifetime spend — munder-difflin `costLifetime.ts`); exclude cache-read from budget arms.
- **Session bridge** — ACP v1 (JSON-RPC 2.0 over NDJSON; use the Rust ACP schema crate) for Claude/Codex/OpenCode/pi; a thin `LocalCliCapabilities` shim (resume/instruction/ingress/permission — Orkas `local_agents/registry.ts`) for non-ACP CLIs. Bind each session to `task_id` + role manifest.
- **CLI surface** — `studio status`, `studio logs --follow` (no GUI yet).

**Exit:** N parallel coders in isolated worktrees produce commits with **0 `.git/index.lock` collisions**; create/stash/teardown/merge are idempotent and crash-safe (no disk leaks after a `kill -9`); a DAG drives dispatch with no model call in the loop.

### Phase 2 — Dynamic MCP gateway / Tool Lens (Pillar 3)

Reuse: opencode-swarm derived `TOOL_METADATA → AGENT_TOOL_MAP` + opt-in overlays (`agents/index.ts`), `scope-guard.ts`, `shell-write-detect.ts`; oh-my-pi `xd://` progressive disclosure (`ToolLoadMode essential|discoverable`); agent-swarm overflow-KV + tool-summarizer; Orkas `tool_load` lazy groups.

Concrete behavior:
- **Role manifests + progressive disclosure.** Per role `{allow, on-demand, deny}` (Manager / Researcher / Coder / Reviewer). Agent sees a compact index (name · 8-word desc · ~20 tok/tool); `tool_open(name)` returns the full schema; the gateway checks the manifest at resolution time. Enforce twice: gateway deny + ACP `session/request_permission` structured refusal.
- **Derived tool maps + parity.** Tool schemas `{id,version,agents[],scopes,featureFlag?,sandbox,prWorkflow?}`; derive `AGENT_TOOL_MAP` by inversion with compile-checked `defineHandlers` so a missing/stray registration is a build error; run registration-parity tests + `doctor tools` (prevents the phantom-tool class that shipped 6 unregistered tools in opencode-swarm v6.48.0).
- **Write safety.** Single `WRITE_TOOL_NAMES` list; `resolveWriteTargets` (caps 1M/2M); `shell-write-detect` (8 categories: `redirect|here_doc|builtin_write|inplace_edit|interpreter_eval|network_download|archive_extract|git_destructive`, bash-AST, fail-closed on unresolvable `$VAR`); `runProcess` chokepoint (`ProcessSpec{program,args,cwd,env,timeoutMs=30s,maxOutputBytes=8MiB}`, `shell:false`, bounded-output sink, `proc.kill()` in `finally`).
- **Anti-flood.** Overflow pointer (wire-limit 10KB → 24h `mcp:overflow:<agentId>` KV, same pointer both channels, `kv-get` exempt); tool-summarizer with `retrieve_*` floor; context-budget prune-outgoing-only; per-session search dedup. **Scoreboard:** Registry shows per-role resolved-schema token cost; target ≈ the 35× cut (validated by H3).

**Exit:** Coder/Researcher/Reviewer see only their role schemas; per-role schema token cost is visible and approaches the 35× target; out-of-scope writes are denied (`SCOPE_NOT_DECLARED` / `SCOPE_ROOT_ESCAPE` / `SCOPE_VIOLATION`); overflow degrades to a pointer, not a flood.

### Phase 3 — Verification gate & merge queue (Completion discipline for Pillars 1 & 2)

Reuse: agent-orchestrator head-SHA-fenced feedback (`review.go`); opencode-swarm `write-authority.ts` one-shot approval facts + spec-hash staleness + gate-evidence; munder-difflin `Stop` hook `{"decision":"block"}`; Orkas loop guards / progress governor.

Concrete behavior:
- **Reviewer/Auditor gate.** Receipts compare **declared write-intent vs. actual file delta**; checks `syntax_check` (tree-sitter), `placeholder_scan`, `sast`, `sbom`, `quality_budget`; evidence in a content-addressed store (sha256 blobs referenced from receipts).
- **Completion Contract (anti-tap-out).** Machine-checkable DoD declared at DAG-commit (a task without one is not dispatchable); `done` is a typed receipt requiring evidence (proposing future work is a `blocked` receipt); deterministic tap-out detector; scope-completion-ratio gate (2 of 7 planned files = 0.29 → refuse); tamper guard (workers may not shrink the assertion set); adversarial refute-done. Only one mechanism uses a model.
- **Merge queue.** Bors-style turnstile (one merge at a time, full tests) + file-disjoint fast path; conflict → typed `{conflictFiles}` → `CONFLICT_RETAINED` diagnostic → coder loop; `prune` before `branch -D`; feedback delivered **head-SHA-fenced** (stale verdicts never re-injected).
- **Anti-stall.** Heartbeat lease (missed renewal → reap → `lease_expired` receipt); 3-class stall taxonomy (generic / no-session / stale-heartbeat) + bounded resume budget; doom-loop detector (no file delta / repeated tool signature / burn-with-no-commit) → `blocked` receipt + wipe-and-rerun operator action; per-role tool-loop budgets (terminal receipt on exhaustion); circuit breaker. Every counter is unit-tested to increment (munder-difflin's `HOP_CAP` was checked against a `hops` field that never incremented).

**Exit:** nothing lands without reviewer + green tests; `done` is evidence-backed (prose "done" is rejected); parallel merges reconcile deterministically; stalls resolve without human babysitting.

### Phase 4 — High-density cockpit (Pillar 4)

Reuse: the three reports' design systems (claude §5 "Precision Instrument", antigravity §5 "Industrial Monochrome", opencode `03-cockpit.md`); Paseo binary terminal frames + coalescers; AoE `structured_view`; Orchestrator `WorkspaceDiffView`; orca diff tokens.

Concrete behavior:
- **Shell.** Tauri (Rust core as the Tauri backend) hosting a React/Tailwind frontend; a Ratatui TUI as the terminal-first/low-bandwidth surface. Adopt one token set (oklch dark-first surfaces/borders/text; single-source status colors; diff added/modified/deleted/renamed tokens; mono + `tabular-nums`).
- **Four views.** (1) **War Room** — fleet board with lanes `building→validating→in-review→ready`, DAG visualizer, token-burn velocity; (2) **Agent Stream** — full-bleed terminal + collapsible event spine per turn; (3) **Audit & Gatekeeper** — unified diff with the **evidence receipt first** (declared intent vs. delta), inline hunk critique → queued back to the coder head-SHA-fenced; (4) **MCP Registry** — role × capability matrix + per-server schema token cost + permission-decision log.
- **Perf/discipline.** Coalescers (terminal 5ms, agent 60ms), frame-commit + paced reveal, virtualized diff (`useVirtualizer`), pooled terminals that survive tab switches; every view stamps `source: studio.db · updated Nms ago` and shows a stale badge rather than owning state (the UI never owns state — if GUI and `studio.db` disagree, the GUI is wrong).

**Exit:** an operator runs a full epic (scope → DAG → dispatch → review → merge) from the cockpit; all four pillars are visible and controllable.

### Phase 5 — Moat & expansion (each self-contained; gated where noted)

- **Write-intent fabric moat (gated on H1).** Semantic scope claims (file/fn/symbol via the Phase 1 tree-sitter extractor) become the default isolation; deterministic overlap rules reject/queue colliding intents; worktree becomes the fallback for scopes declared `isolation: worktree|container`. If H1 failed, this item is dropped and Phase 1 worktree+merge-queue remains the isolation story.
- **Artifact-first creative lanes** (3D / image / video / desktop-web): source-of-record first (HTML/CSS/SVG, Remotion timeline, Blender Python, accessibility-tree), deterministic render, never text-merge binaries — the merge queue takes the winning source-of-record hash and re-renders. `Asset3D`/`VideoStudio`/`ImageStudio` specialist workers in disposable worktrees.
- **Distributed topology:** local lanes default; remote GPU nodes + cloud sandboxes (E2B/Modal) for heavy/untrusted work over an E2E relay (NaCl box, consent-gated); never share one checkout across hosts (`repoId::path`, `wt2:host:inst`).
- **Memory Ladder (structured-first):** L0 git+receipts, L1 `studio.db` facts + FTS5, L2 markdown project memory, L3 lifecycle memory rows (veracity/supersession/trust), L4 **opt-in** semantic index with a mandatory reranker. Reuse the AST/FTS5 "triad" (antigravity §5). Retention + reapers from day one (a never-delete bug once wrote 100GB in 8h).
- **Agent templates + identity:** versioned `RolePack`s (manager/researcher/coder/auditor/creative-*/qa-*/docs/release) with skill lockfiles + registration-parity tests; multi-profile isolation via `CLAUDE_CONFIG_DIR` / `CODEX_HOME` / `OPENCODE_CONFIG_DIR` per lane + a capability-token secret broker (workers never see raw creds; one-shot approval facts for OAuth'd writes).

---

## Critical files & anchors

Reference sources to port from (re-locate by symbol before editing; greenfield `studio-core/` layout is specified in the Approach):

- `review/agent-of-empires/src/{process,events,git/worktree}/` — Rust process/session supervision, event log, worktree lock/prune/trash. **Why:** the only Rust-native model for the daemon role the user selected; anchors Phase 1 supervision + worktree discipline.
- `review/agent-orchestrator/backend/internal/{ports,cdc}/` — `Workspace` port (`Create/Destroy/Stash/Apply`), `change_log`→CDC→SSE. **Why:** canonical ports-and-adapters + durable-facts pattern Phase 1 ports to Rust.
- `review/orca/src/main/worktree-trash.ts` — trash-rename deferred delete + create `--detach --no-checkout`→`reset --hard`→`lock`. **Why:** the load-bearing disk-hygiene + create discipline for Phase 1 teardown.
- `review/opencode-swarm/src/{hooks/shell-write-detect.ts,security/write-authority.ts,plan/ledger.ts,agents/index.ts}` — shell-write AST, one-shot approval facts, hash-chain ledger, derived tool map. **Why:** the enforcement + ledger + tool-slicing primitives for Phases 2–3.
- `review/oh-my-pi/packages/coding-agent/src/tools/xdev.ts` — `xd://` progressive disclosure (`ToolLoadMode essential|discoverable`). **Why:** the proven Tool Lens pattern Phase 2 generalizes.

New entry points the implementer creates (names fixed so callers conform): `studio-core/src/{worktree,state,dag,scheduler,mcp,session}/mod.rs`, `cockpit/` (Tauri), `tui/` (Ratatui), `adapters/{acp,cli}/`. No equivalent exists in the corpus as one Rust workspace, so these are new; each module's behavior is specified in the Approach.

---

## Verification

Working directory `/home/john/Projects/STC`. The roadmap is proven by its phase exit criteria; the following are the concrete, runnable checks (each tied to a phase).

- **Phase 0 spikes (new-behavior checks, throwaway harnesses in `plans/spikes/`):**
  - **H1:** run 5 scripted parallel edits on one shared tree via the claim broker → **expect 0 collisions on non-overlapping claims** and success rate within 5% of a worktree baseline. Fail → Phase 5 write-intent moat is dropped.
  - **H2:** replay a 20-task DAG through the pure-state-machine scheduler → **expect completion within 5% of the manager-agent baseline and no deadlock**. Fail → manager model re-enters that dispatch step.
  - **H3:** A/B the schema-slicing proxy on a 50/100/200-tool suite → **expect resolved-schema tokens to approach the ~35× cut** with completion within 5%. Fail → ship scripts-only mode.
  Each writes pass/fail vs. its kill criterion to `plans/spikes/`.
- **Phase 1 exit (Pillars 1 & 2):** `studio-core` stress test — spawn 10 concurrent Coder agents on 10 independent tasks in isolated worktrees → **expect 0 `.git/index.lock` collisions and 100% clean isolation**; then `kill -9` the daemon and re-run teardown → **expect no orphaned worktrees/dirs** (crash-safe, idempotent). A DAG dispatch run shows no model call in the dispatch loop.
- **Phase 2 exit (Pillar 3):** invoke the gateway as Coder/Researcher/Reviewer → **expect only role-schema tools visible** and per-role schema-token cost reported near the 35× target; issue an out-of-scope write → **expect a typed denial** (`SCOPE_*`); send a 1MB tool result → **expect an overflow pointer, not a flood**.
- **Phase 3 exit:** feed a task that stops at 80% with "here's what's next" → **expect a `blocked`/tap-out rewind, not `done`**; submit a conflicting parallel merge → **expect `CONFLICT_RETAINED` + an actionable diagnostic, no lost work**; run the full merge queue → **expect green-checks-gated landing**.
- **Phase 4 exit (Pillar 4):** in the cockpit, run one epic scope→DAG→dispatch→review→merge → **expect all four views to reflect `studio.db`** (War Room, Agent Stream, Audit+evidence receipt, MCP matrix) with stale badges on lag.

Prereqs: Rust toolchain (Tokio/rusqlite/tree-sitter), a git repo with ≥2 branches for worktree tests, one ACP harness (Claude Code) for Phase 1, Node/Bun only for the Tauri/TUI frontend. The corpus reference repos under `review/` must be present to port patterns.

---

## Assumptions & contingencies

Resolved spine (overridable): **Rust/Tokio core**; **worktree-first, write-intent moat deferred behind the H1 gate**; **spike-first**. The cockpit is web-based (Tauri + Ratatui TUI) regardless of core language.

Pre-decided fallbacks (so the implementer never stalls):
- **H1 fails** (any conflict on non-overlapping claims, or success drop > 5%): drop the write-intent moat entirely; Phase 1 worktree + merge-queue (`CONFLICT_RETAINED`) remains the complete isolation story. Do not attempt to "fix" the claim granularity.
- **H2 fails** (completion drop > 5%, or a deadlock the baseline avoided): allow the Manager model back into the dispatch step that failed; keep the rest of the plane zero-token.
- **H3 fails** (schema cut well short of 35×, or completion drop > 5%): ship the **scripts-only** tool mode (the measured path) instead of progressive disclosure; keep role manifests + least-privilege enforcement.
- **Rust ACP schema crate is immature:** build against ACP v1 stable and isolate protocol types behind one module (`adapters/acp/`); non-ACP CLIs go through the `LocalCliCapabilities` shim.
- **Tree-sitter grammar gaps (13+ language profiles):** start with TS/Python/Go/Rust; unknown grammars fall back to **file-granularity** scope claims (never fail the write for an unknown grammar).
- **SQLite single-writer blocks multi-machine sync:** keep the ledger per-workspace in Phases 1–4; multi-machine sync is a Phase 5 item, not a Phase 1 blocker.

Scope boundary (inline, not a section): v1 ships the four pillars + the Phase 5 moat; it does **not** ship a marketplace, cloud hosting, mobile companion, or voice. The corpus shows those are where products drown before the engineering core is trustworthy.
