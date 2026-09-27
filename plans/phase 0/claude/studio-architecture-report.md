# Studio — Architecture Report & Implementation Plan

**Project:** Studio — multi-agent autonomous software engineering studio and orchestration engine
**Date:** 2026-09-26
**Status:** Research and architecture complete. No application code generated. Awaiting spike approval.
**Source corpus:** `/home/john/Projects/STC/review` — 12 repositories, source-level forensic audit
**Companion artifacts:** `plans/claude/STUDIO_FINDINGS_AND_REPORT.md` (consolidated findings + measurements — read first), `.research/brainstorm_20260926T203716Z.md` (audit + dialectic I), `.research/brainstorm_20260926T211427Z.md` (open questions + dialectic II), `scratchpads/brainstorm_studio_arch/domain_model.json` (domain model, closed loops)

Epistemic convention used throughout: `[VERIFIED path]` = claim read from source; `[INFERRED]` = synthesis from named evidence; `[HYPOTHESIS: …]` = falsifiable speculation with a kill criterion.

---

## 1. Executive Summary

Studio is a greenfield build. The corpus it must exceed splits into three tribes:

| Tribe | Representatives | Strong at | Weak at |
|---|---|---|---|
| Session managers | agent-of-empires, orca, paseo, agent-orchestrator | worktree lifecycle, harness wrapping, multi-surface UI | daemon owns all truth; worktree-per-agent disk explosion |
| Coordination & gates | opencode-swarm, agent-swarm | gated review pipelines, scope enforcement, write detection | LLM-heavy coordination burns the budget the work needed |
| Runtimes & learning | oh-my-pi, claude-smart, munder-difflin, iPolloWork, Orkas | tool/IDE depth, durable skill learning, runtime pluggability | weak isolation; unbounded context growth |

**Four findings that drive the design:**

1. **Tool-schema bloat is measured, not felt.** agent-swarm recorded **118 tools ≈ 311 KB ≈ 80K tokens → 8 script tools ≈ 8.8 KB ≈ 2.2K tokens (~35×)**, and noted only Claude has ToolSearch — "pi/codex/opencode have no tool-search, so for them the full schema really does enter every session." `[VERIFIED thoughts/shared/research/2026-07-11-scripts-only-mcp-experiment.md]`
2. **Progressive tool disclosure already exists in production.** oh-my-pi's `xd://` virtual devices demote rarely-used tools out of the schema and expand on demand (`ToolLoadMode essential|discoverable`). `[VERIFIED packages/coding-agent/src/tools/xdev.ts]`
3. **Decorative orchestration is the corpus's central failure.** iPolloWork declares a `dependency|parallel` graph and never walks it; agent-orchestrator has no dependency concept; Orkas deleted its plan DAG. Only agent-swarm has a real `dependsOn` queue — and it has zero referential integrity.
4. **No one has solved write conflicts at the semantic layer.** Everyone ships worktrees, then a merge queue to undo the damage.

**The thesis:** Studio is not another daemon with worktrees and a dashboard. Its defensible core is a **zero-token coordination plane** (deterministic, no model in the dispatch loop) over a **write-intent fabric** (semantic scope claims instead of default worktree cloning), with a **Tool Lens** MCP proxy (progressive schema disclosure + role manifests — the 35× lever), and a session bridge that is **ACP**, an existing open standard. The cockpit is a projection of that plane, never a second source of truth.

---

## 2. Corpus Audit — What We Learned

Full per-repo evidence in `.research/brainstorm_20260926T203716Z.md` §2. Condensed:

### 2.1 Comparative matrix

| | AoE | orca | paseo | AO | agent-swarm | opencode-swarm | oh-my-pi | claude-smart | munder-difflin | iPolloWork | Orkas |
|---|---|---|---|---|---|---|---|---|---|---|---|
| Orchestration | supervisor + lease/epoch | coordinator poll 2s | daemon + run-state | direct spawn, **no DAG** | queue + offer/claim + workflows | gated phase pipeline | task workpool | n/a (hooks) | god + hive routing | config DAG **decorative** | commander-in-the-loop |
| DAG real? | n/a | partial | n/a | **no** | **yes** | phase gates | n/a | n/a | tasks.json | **no** | **no** (deleted) |
| Isolation unit | worktree + Docker | worktree | worktree | worktree | **container** | worktree/lane | n/a | n/a | PTY + flags | n/a | tool groups |
| Tool scoping | capability taxonomy | skill selectors | flat disabledTools | harness perms | **scripts-only 35×** | derived TOOL_MAP + budget | **`xd://` progressive** | hook matchers | flag allowlist | **prompt text only** | tool groups + `tool_load` |
| Write safety | — | — | — | — | — | **shell-write-detect + approval facts** | approval tiers | — | PreToolUse deny | — | permission policy |
| Review gate | — | — | — | **head-SHA fenced** | task follow-up (string-matched) | **gate-evidence + spec-hash** | — | — | — | — | — |
| Stall handling | respawn budget | circuit break | heartbeat grace | no-signal facts | **3-class stall + resume budget** | fail-closed | — | — | wake watchdog | lease claim | host budgets |
| Teardown | best-effort | **trash-rename** | half-removed fallback | discard pile | container | **fail-closed** | — | — | Closing Time | — | — |
| Context hygiene | — | — | **3-layer flood stack** | — | **35× measured** | prompt budget, 2-point | **StablePrefix + compaction** | token leak (anti-pattern) | cache-stable prefix | — | roster token budget |

### 2.2 Tier-1 cherry-picks (build these first)

| # | Pattern | Source | Applies to |
|---|---|---|---|
| 1 | `xd://` discoverable tool mounting + `essential`/`discoverable` load modes | `oh-my-pi/.../tools/xdev.ts` | Tool Lens |
| 2 | Scripts-only / code-mode MCP + measured 35× schema cut | `agent-swarm/.../scripts-only-mcp-experiment.md` | Tool Lens budget |
| 3 | Shell write detection as static analysis (8 `WriteCategory`s, bash-parser AST) | `opencode-swarm/src/hooks/shell-write-detect.ts` | Write safety |
| 4 | One-shot write-approval fact ledger (session/action/content-bound, consumed-once) | `opencode-swarm/src/security/write-authority.ts` | Human-in-the-loop gates |
| 5 | Head-SHA-fenced feedback delivery (stale verdicts never re-injected) | `agent-orchestrator/.../review.go:779-793` | Auditor → coder loop |
| 6 | Trash-rename deferred worktree deletion (8–35s IPC block → metadata rename) | `orca/src/main/worktree-trash.ts` | Disk hygiene |
| 7 | Speculative worktree prepare/claim/cancel tokens | `agent-orchestrator/.../delegation.go:48-62` | Worktree state machine |
| 8 | Stall taxonomy (3 classes) + bounded resume budget | `agent-swarm/src/heartbeat/heartbeat.ts:104-162` | Deadlock handling |
| 9 | `LocalCliCapabilities` resume/instruction/ingress contract | `Orkas/.../local_agents/registry.ts:37-80` | Session bridge |
| 10 | Prompt-cache-stable injection (volatile-free prefix + hook channel) | `munder-difflin/src/main/hive.ts:1409-1418` | Cache economics |

A full ranked list of 49 stealable artifacts is in the companion brainstorm §2.13.

### 2.3 Anti-patterns to design against

Each verified in source:

1. **Prompt-text scoping instead of enforcement** (iPolloWork `skillIds`/`pluginIds` rendered into a system prompt, never filtered) → gate at the MCP gateway.
2. **Decorative DAGs** (iPolloWork `relations` validated and drawn, never executed) → a DAG Studio does not schedule does not ship.
3. **Fixed per-turn injection tax** (claude-smart re-appends a ~150–250 token citation instruction on every Edit/Write/Bash) → budget every injected byte.
4. **Coordination on the hot path** (munder-difflin runs synchronous `git commit` per routed message) → the plane writes ledger rows; commits are deliberate.
5. **Un-incremented loop guards** (munder-difflin `HOP_CAP` checked but `hops` never incremented) → unit-test that every counter increments.
6. **Unbounded append-only files** (`cost-ledger.jsonl`, `.sent/` kept forever) → SQLite with retention; content-addressed receipts.
7. **Enumerated lists drifting from registrations** (opencode-swarm v6.48.0 shipped six tools that were listed but never registered) → derive tool maps from a compile-checked manifest; run parity tests.
8. **God modules** (Orkas `bus.ts` 10,751 lines; paseo `agent-manager.ts` 5,363; opencode-swarm `index.ts` 6,271) → small modules with documented lock order.
9. **Best-effort disk hygiene** (AoE comments a leftover dir is "a cosmetic leak"; AO ships platform-forked removal repair) → fail-closed teardown + trash-rename + ledger-reconciled orphans.
10. **String-matched control flow** (agent-swarm `failureReason?.includes("Blocked dependency")`) → typed outcome enums.

---

## 3. Target Architecture

### 3.1 Process & concurrency — the loop that is not a model

```
                    ┌──────────────────────────────────────────┐
 human ⇄ MANAGER ──▶│  DAG ARTIFACT (typed, hash-addressed)     │
 (scope only)       └───────────────┬──────────────────────────┘
                                    ▼
 ┌────────────────────────────────────────────────────────────────┐
 │ COORDINATION PLANE  (no model calls — pure state machine)      │
 │  Scheduler ──▶ Claim Broker ──▶ Lease ──▶ Worker               │
 │      ▲              │              │            │              │
 │      │         overlap rules   TTL+renew    heartbeat          │
 │      │              ▼              ▼            ▼              │
 │  Merge Queue ◀── Receipt Check ◀─ Reap ◀── Watchdog            │
 └────────────────────────────────────────────────────────────────┘
        │                                    │
        ▼                                    ▼
  staging branch                     studio.db (event log)
```

**Supervision tree** — each node owns its children and has a defined death policy:

| Node | Owns | On child death |
|---|---|---|
| `studio-root` | `studio.db`, ledger, watchdogs | restart plane; reconcile from ledger |
| `plane` | scheduler, claim broker, merge queue | restart; replay event log |
| `harness-relay` (per agent) | one ACP session | reap lease, mark task `failed`/`requeued`, free claims |
| `mcp-gateway` | tool proxy, schema cache | restart; sessions reconnect |
| `fs-sandbox` (per task) | Seatbelt/Docker/none | kill process group; release claims |

**Stall and deadlock handling** is deterministic, never "ask the model":
- **Heartbeat lease** — each worker renews a lease; missed renewal → watchdog reaps → task returns to queue with a `lease_expired` receipt.
- **Three-class stall taxonomy** (agent-swarm) — generic stall, no-session (worker clearly dead), stale-heartbeat. Plus a **bounded resume budget** so resume loops cannot thrash.
- **Doom-loop detector** — trips on (a) no file delta across K turns, (b) repeated identical tool-call signature, (c) token burn over budget with zero commits. Emits a `blocked` receipt and surfaces **wipe & re-run** as an operator action.
- **Per-role tool-loop budgets** (Orkas `actor-budgets.ts`) — hard caps; exhaustion is a terminal receipt, not a silent stop.
- **Queue-stall alarm that respects dependencies** — only measures the claimable denominator, so unmet-DAG tasks never cry wolf.

### 3.2 Write-Intent Fabric (the central bet)

Default isolation is **semantic scope claims**, not filesystem clones.

- Before writing a byte, an agent files a **Write Intent**: semantic scope at file → function → symbol granularity (Tree-sitter sidecar), into a claim table.
- Deterministic overlap rules reject or queue colliding intents. N writers, one tree, zero merges for non-overlapping scopes.
- `git worktree` is reserved for scopes declared `isolation: worktree | container` (destructive/experimental), enforced at claim time.

**Worktree state machine** (fallback path only):

```
NONE ──add -b──▶ ACTIVE ──commit──▶ DIVERGED ──merge-queue──▶ MERGING ──▶ MERGED ──remove──▶ NONE
  │                │                    │                        │
  │                └──stale(lease lost)─┴──conflict──────────────┴──▶ QUARANTINED ──operator──▶ NONE
  └──crash recovery: `git worktree list --porcelain` ∖ ledger = orphans → prune
```

Lifecycle discipline taken from the corpus:
- create: `git worktree add --detach --no-checkout` → `reset --hard` → `lock --reason` (orca + AoE)
- write a ledger row *before* the git command; re-running remove is idempotent success
- delete via **trash-rename** (metadata op), sweep asynchronously in a bounded loop (orca)
- removal is **fail-closed** — dirty/blocked worktrees surface, never force-delete (opencode-swarm)

### 3.3 Tool Lens & MCP Proxy Gateway

Separate **catalog** (cheap, always present) from **schema** (expensive, on demand), and enforce capability at *resolution* time rather than enumeration time.

```
 agent session
      │  1. sees COMPACT INDEX:  name · 8-word desc · role-flag   (~20 tok/tool)
      │  2. calls `tool_open(name)`  ──▶ returns full JSON schema
      │  3. calls the tool             ──▶ gateway checks role manifest
      ▼
 ┌─────────────────────────────────────────────────────────┐
 │ MCP GATEWAY                                              │
 │  schema cache (content-addressed)                        │
 │  role capability manifest  → allow / on-demand / deny    │
 │  ACP session/request_permission hook  → allow / ask      │
 │  audit: every resolution + denial is a ledger event      │
 └─────────────────────────────────────────────────────────┘
      │
      ├── shared MCP servers   (search, LSP, git, AST)  — long-lived, pooled
      └── ephemeral MCP servers (browser, sandbox FS)    — per-task, destroyed
```

**Role capability manifests** — least privilege as *resolution policy*:

| Role | `allow` | `on-demand` | `deny` |
|---|---|---|---|
| manager | dag.read, task.assign, budget.read, plan.write | — | fs.write, shell |
| researcher | search.semantic, search.grep, doc.fetch, browser | git.read, dep.graph | fs.write, shell.exec |
| coder | fs.edit, fs.read, lsp.*, terminal.exec, git.write, test.run | dep.graph, doc.fetch | browser, secrets |
| auditor | git.diff, ast.inspect, lint.*, test.run, sast, secrets.scan | fs.read | fs.write, mutating shell |

Enforcement happens twice: at the gateway (deny at resolution) and via ACP `session/request_permission`, so the agent gets a structured refusal rather than a mystery missing tool.

**Supporting machinery worth copying:**
- shell write detection (static analysis of redirects, builtins, in-place editors, network downloads, archive extraction, git destructive ops) — the missing layer under "agent has bash"
- write-approval fact ledger for human-required mutations
- derived `TOOL_METADATA` + registration parity tests + a `doctor tools` command
- prompt-composition budget measured at factory exit **and** after user-controlled substitution

**The scoreboard:** the Registry view shows, per role, the token cost of the resolved schema set. That number is the anti-flooding metric made visible. Target: approach the measured 35×.

### 3.4 Data & state persistence

Event-sourced SQLite (`studio.db`, WAL) is the single source of truth. The plane rebuilds in-memory state from the log on restart; the UI renders ledger + log tail and never owns state.

| Store | Contents | Notes |
|---|---|---|
| `tasks` | `id, parent_id, kind, intent_hash, scope_hash, status, depends_on` | hash-addressed; enables task cache |
| `claims` | `task_id, scope, gran(file\|fn\|symbol), owner, lease_expires_at` | the write-intent fabric |
| `events` | append-only: dispatch, lease, tool-resolve, denial, commit, verdict | replay source |
| `receipts` | `task_id, prompt_hash, model, tokens_in/out, file_delta, verdict, signature` | aGiTrack / YYLO lineage |
| `token_ledger` | per-agent / per-task / per-turn spend | rollups are views |
| `merges` | merge-queue entries, conflict findings, outcome | |

- Token accounting is a **ledger, not a counter** — every turn writes a row; budgets are enforced by the plane reading the ledger, never by agent self-report. Split display/cost tokens from budget-arm tokens (exclude cache-read from budget arms).
- Evidence store: content-addressed blobs (diffs, tool payloads, test output) keyed by `sha256`, referenced from receipts. Makes replay possible.
- Board-shaped state uses whole-file snapshot writes (Orkas discipline), not row-level mutation.
- `dependsOn` gets **referential integrity** — the exact gap in agent-swarm.

### 3.5 Session bridge — ACP is a dependency, not a workstream

ACP v1 is a stable JSON-RPC 2.0 contract over NDJSON on stdio (or TCP): `initialize`, `session/new`, `session/prompt`, `session/update`, `session/request_permission`, cancellation, session restoration. Shipped adapters exist for Claude Agent, Codex CLI, Gemini CLI, Cursor, Copilot, Docker `cagent`. Four of the twelve audited repos already integrate it (AoE `acp-worker/`, paseo `generic-acp-agent.ts`, agent-swarm `acp-adapter`, Orkas diagnostics).

| Concern | ACP primitive | Studio extension |
|---|---|---|
| session create | `session/new` | binds `task_id` + role manifest |
| turn | `session/prompt` | emits ledger events |
| streaming | `session/update` | one timeline (tool calls + file deltas) |
| permission | `session/request_permission` | **gateway decides from role manifest; `ask` escalates** |
| resume | session restore / session-id | `resume: native \| session-id \| none` |
| cancel | cancellation | frees claims, writes receipt |
| non-ACP CLI | — | thin PTY/stream-json shim with the same capability table |

The shim layer is the only bespoke work. A new harness attaches without Studio changes.

### 3.6 Manager ↔ human negotiation

The Manager is the only model in the coordination path, and only at the boundary:

1. Human states an epic. Manager asks the minimum questions, then emits a **typed DAG artifact** (stories → tasks, each with `intent`, `scope`, `acceptance`, `budget`, `depends_on`).
2. The DAG is committed to the ledger and is *the* contract. Subsequent negotiation is an edit to that artifact — diffable, reviewable — not a chat transcript to re-interpret.
3. Everything after commit is mechanical: claim, execute, receipt, audit, merge.
4. Failures return to the human as **receipts + a proposed DAG patch**, not free-form chatter.

This makes negotiation resumable and auditable without keeping an LLM in the dispatch loop.

---

## 4. Architecture Decision Records

### ADR-001 — Core runtime: TypeScript on Bun, with a Rust sidecar for Tree-sitter
**Status:** Proposed. **Decision:** TS/Bun for plane + gateway + cockpit; a small Rust stdio sidecar for Tree-sitter scope extraction.

**Rationale:** (1) ACP adapters are TS-first — that is the largest schedule risk and it evaporates; (2) the cockpit is web/Electron regardless, so one language, one test runner, one type system; (3) the plane is I/O-bound on PTYs, git, and harnesses — Rust's advantage is small, its cost is schedule; (4) Tree-sitter is the one genuinely CPU-sensitive, parser-correctness-critical piece — isolate it as a sidecar speaking stdio JSON-RPC.

**Corpus signal:** 6 of 12 are TS/Bun; opencode-swarm ships 6,000+ tests in Bun.
**Consequences:** fastest iteration, one hiring profile, direct pattern reuse. Negative: a second toolchain to release.
**Revisit if:** scheduling latency p99 > 50ms at 20 concurrent agents, or the sidecar boundary dominates integration cost.

### ADR-002 — IPC / transport: ACP + SQLite WAL event log
**Status:** Proposed. **Decision:** two planes, two transports — control plane = ACP over stdio; state plane = append-only event log in `studio.db`, UI subscribes to the log tail.

**Rationale:** ACP is an open standard; building a bespoke session protocol is pure loss. SQLite WAL beats gRPC/Redis/NATS for a local-first studio: crash-safe, zero ops, auditable, works offline. **Corpus signal:** foremerge ("one Rust binary over local SQLite"), iPolloWork (`runtime.sqlite` + lease claims), opencode-swarm (`node:sqlite`) all chose SQLite.
**Rejected:** gRPC (heavy for local IPC), custom Unix-socket RPC (reinvents ACP), WebSockets as the state bus (loses durability).

### ADR-003 — Merge conflicts: write-intent fabric first, worktree as fallback
**Status:** Proposed — this is the platform's central bet. **Decision:** default isolation is semantic claims on a shared tree; worktrees only for declared destructive scopes.

**Rejected:** worktree-only (N× disk + merge queue); file-locking (too coarse); "let the merge queue sort it out" (serialization tax).
**Risk accepted:** ambient side effects escape claim granularity → destructive scopes must declare `isolation: worktree | container`, enforced at claim time.

### ADR-004 — Session bridge: ACP + `LocalCliCapabilities` shim
Non-ACP CLIs get a thin PTY/stream-json shim exposing the same capability table (resume mode, instruction channel, active-run ingress, permission policy).

### ADR-005 — State: event-sourced SQLite; UI as pure projection
`studio.db` is the only source of truth. Board-shaped state uses whole-file snapshots. Receipts are content-addressed blobs referenced from the log.

---

## 5. Design Language & UX

**Philosophy — "Precision Instrument" (precision brutalism).** An instrument panel for a fleet of writers: density is a feature, chroma is semantic only, and the UI is a projection of the coordination ledger. Four laws:

1. **The UI never owns state.** If the GUI and `studio.db` disagree, the GUI is wrong.
2. **Color is signal, never decoration.** Every hue maps to exactly one meaning.
3. **Numbers are tabular and comparable.** Mono + `tabular-nums` for every value a user might compare.
4. **One expressive object.** The scope-conflict DAG is the only place the design is allowed to be beautiful.

**Palette (dark-first):**

| Token | Value | Role |
|---|---|---|
| `--ground` / `--surface-1..3` | `#0B0D0E` / `#121517` `#191E21` `#22282C` | ground and elevation ladder |
| `--rule` / `--rule-strong` | `#2A3136` / `#3A434A` | hairlines, focus |
| `--ink` / `--ink-muted` / `--ink-faint` | `#E8ECEE` / `#9AA5AB` / `#5E686E` | text hierarchy |
| Roles | manager `#7C8CFF` · researcher `#3DBDA8` · coder `#D8A24A` · auditor `#B48CE0` · system `#6B757B` | agent identity |
| Status | idle `#6B757B` · running `#4EA1FF` · blocked `#D8A24A` · gated `#B48CE0` · failed `#E5534B` · merged `#3FB950` | state machine |
| Token heat | `#1B2A33` → `#2E5A66` → `#7A6A2E` → `#C47A22` → `#D1442F` | node fill, meters |
| `--conflict` | `#FF4D3D` | **DAG edges and claim-rejection toasts only** |

**Typography:** IBM Plex Sans (UI chrome) / IBM Plex Mono (telemetry, code, diffs) with `font-variant-numeric: tabular-nums`. Iconography: 16px grid, 1.5px stroke, unfilled; status is a 6px filled square, never an emoji; agent role is a 2-letter mono sigil. Grid: 4px base, 8px rhythm, 28px compact table rows; rail 240px / fluid stage / inspector 360px.

**Four views:**

1. **War Room** — live scope-conflict DAG (node fill = burn velocity, stroke = status, **edges flash `--conflict` on intent-lock overlap**), fleet rail, token weather per *scope region*, burn velocity in $/hr.
2. **Agent Detail / Stream** — full-bleed terminal with PTY attach; a collapsible **event spine** per turn (prompt → tool calls → results → commit/receipt); tool payloads collapsed to `name · args-shape · duration · tokens`, expandable inline.
3. **Audit & Gatekeeper** — unified diff, and as the *first* card the **evidence receipt**: declared write-intent vs. actual file delta (the immune check). Verdict checklist. One-click hunk finding → queued message back to the same coder session. Gate verbs: `request changes` / `approve & merge` / `reject scope`.
4. **MCP Registry** — role × capability matrix (`full` / `on-demand` / `denied`), MCP server inventory with **per-server schema token cost**, and an ACP permission-decision log tail.

Cross-cutting: doom-loop watchdog surfaces **wipe & re-run** as a first-class action; everything is keyboard-first (`g w` / `g a` / `g m`, `/` palette); every view stamps `source: studio.db · updated Nms ago` and shows a stale badge rather than inventing state.

---

## 6. Roadmap

### Phase 0 — MVP CLI harness (2–3 weeks) — *proves the two bets that kill the project if wrong*
1. Tree-sitter scope extractor sidecar (Rust) → `scope_hash(file|fn|symbol)`
2. Claim broker + lease + SQLite event log (`claims`, `events`, `tasks`)
3. One ACP harness bridge (Claude Code) + `session/request_permission` hook
4. Manager → typed DAG artifact + a **mechanical** scheduler that claims and dispatches
5. CLI status surface (`studio status`, `studio logs --follow`) — no GUI yet
6. **Gate: run spikes H1 and H2.**

### Phase 1 — Tool gating & MCP Router (2 weeks)
Schema cache + role manifests; Tool Lens (`tool_open` expansion); shared vs ephemeral MCP lifecycle; token ledger on every resolution/denial. **Gate: run spike H3.**

### Phase 2 — Auditor / Reviewer gate (2 weeks)
Auditor role manifests; receipt verification (declared intent vs. file delta); merge queue with `QUARANTINED` findings; one-click finding → coder session; content-addressed task cache.

### Phase 3 — GUI / TUI Cockpit (3 weeks)
Ledger tail → War Room; agent stream; audit gate; registry control center; TUI mode for compact-density users.

### Phase 4 — Scale & hardening (ongoing)
Worktree fallback + container sandbox; merge-queue automation into staging; multi-machine ledger sync; quota-aware harness rotation; runbook → SKILL compilation; skill lockfiles.

**Explicit v1 non-goals:** marketplace, cloud hosting, mobile companion, voice, video/design artifact editors. The corpus shows those are where products drown (iPolloWork's six output modalities, Orkas' nine specialists) before the engineering core is trustworthy.

---

## 7. Risks & Open Questions

| # | Risk / question | Mitigation |
|---|---|---|
| 1 | Write-intent granularity misses ambient side effects (`npm install`, test DB) | Destructive scopes declare `isolation: worktree \| container` at claim time; H1 spike measures the miss rate |
| 2 | Some harnesses cannot do dynamic tool sets | The shim expands the Tool Lens into a fixed set; degrade to scripts-only mode (the measured 35× path) |
| 3 | SQLite single-writer limits multi-machine | Ledger is per-workspace; sync is a Phase 4 problem |
| 4 | Bun process/PTY may not hold 20 agents at p99 < 50ms | ADR-001 revisit trigger; the plane is I/O-bound, but measure in Phase 0 |
| 5 | Tree-sitter grammar coverage across 13 language profiles | Start with TS/Python/Go/Rust; unknown grammars fall back to file-granularity claims |
| 6 | ACP v2 is still draft | Build against v1 stable; isolate protocol types behind one module |
| 7 | Coordination plane could become its own god module | Module boundaries + documented lock order (AoE's discipline); an `engineering-invariants.md` failure map from day one |

---

## 8. Recommended Next Action

Do not start the roadmap. Run the three spikes first — each is under a day, each has an explicit kill criterion:

| Spike | Test | Kill criterion |
|---|---|---|
| **H1 — Write-intent fabric** | Tree-sitter extractor + SQLite claim broker + 5 scripted parallel edits on one tree | any conflict on non-overlapping claims, or task success drop > 5% vs. worktree baseline |
| **H2 — Zero-token coordination** | scheduler + claim broker + merge queue as a pure state machine replaying a 20-task DAG vs. a manager-agent baseline | completion drop > 5%, or any deadlock the baseline avoided |
| **H3 — Tool Lens** | schema-slicing MCP proxy + A/B on a 50/100/200-tool suite | must approach the measured 35× schema cut; completion drop > 5% |

If H1 and H3 hold, Studio has two defensible advantages no competitor in this corpus combines. If H2 holds, the coordination overhead that kills every fleet product stops existing.

If a spike fails its kill criterion, that is the cheapest possible information — and the plan bends, not the schedule.
