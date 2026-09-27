# STC Phase 1 — Platform Research Report (Claude)

**Findings, Evidence, Reasoning & Recommendations**
Date: 2026-09-26 · Agent: Claude Code · Mode: `epistemic-swarm:brainstorming` (6-agent fan-out research → dialectic synthesis)

**Scope:** the ten next-phase platform questions (memory, topology, harness acquisition, persistence, hand-built agents, model management, pi ecosystem, cross-pollination, per-profile skills/MCP, coding standards).

**Method:** six parallel isolated research agents over primary sources, cross-checked against the local corpus (`review/`), the shipped codebase (`studio-core/`), and sibling reports in `plans/phase 1/{antigravity,opencode}/`. Full dialectic in `.research/brainstorm_20260926T224500Z.md`; domain model in `scratchpads/brainstorm_next_phase/domain_model.json`.

**Epistemic tagging:** `[VERIFIED: source]` read in a named primary source · `[INFERRED: parents]` synthesis leap · `[HYPOTHESIS: <test>]` falsifiable speculation.

---

## 0. Corrections to my own earlier research (read this first)

Two claims in my research fan-out were wrong or overstated. Both are corrected here rather than buried.

| Claim | Status | Correction |
|---|---|---|
| *"No tool named 'fallaw'/'fallow' exists; the user meant Flawfinder."* | **WRONG** | **Fallow is real.** `fallow-rs/fallow` — 4,900 stars, Rust binary, MIT, created 2026-03-17, pushed today. I verified it directly via the GitHub API and README after seeing the sibling reports cite it. It is **not** Flawfinder. See §10. |
| *"lefthook is broken in linked worktrees (issue #901)."* | **OVERSTATED** | The bug is real and reproducible (`lefthook install` → `exit 128` after `git worktree add`, lefthook 1.9.3) but issue #901 is **closed** as of this write. Treat as a fixed bug class to re-verify on current lefthook, not a live blocker. My secondary claim (buffered output OOM under agent fan-out) is **unverified** — I could not source it. |

The Flawfinder conclusion in particular was a confident wrong answer from a research agent that searched and found nothing. Both siblings had it right. This is worth noting as a process signal: **a research agent's "does not exist" is weaker evidence than its "here is the thing."** Verify negatives before acting on them.

---

## 1. Executive summary — decisions at a glance

| # | Question | Recommendation | Confidence |
|---|---|---|---|
| 1 | Memory | **No product fits.** Build a rebuildable typed projection over our own ledger. Shipped L0–L4 Ladder is **validated**; three gaps to close. | High |
| 2 | Docker vs desktop | **Hybrid:** native control plane + optional containerized workers. | High |
| 3 | Harness acquisition | **Detect-then-install.** BYO host harness default; manager installs only the missing. Never prebake CLIs into images. | High |
| 4 | Persistence | **Named volume per class** + host-identical bind mounts + `user_version` migrations + layout stamp. | High |
| 5 | Hand-built agents | **Yes — as RolePacks.** But subscriptions and API keys split by auth kind. | High |
| 6 | Model management | **Hybrid:** in-process Model Fabric + credential store + role slots. Gateways as profiles. Pinned tiers default. | High |
| 7 | pi profiles | **Yes, and late not early.** Multi-harness pack; oh-my-pi primary runtime target. | High |
| 8 | Steal list | **Yes.** gentle-ai frozen-candidate + happier action registry + kasetto lockfile. | High |
| 9 | Skills/MCP per profile | **Lockfile-pinned asset manifests**, not ad-hoc installs. | Medium |
| 10 | Coding standards | **Process-level, SARIF-shaped, changed-files-gated.** Fallow for TS/JS, clippy for Rust, `prek` for hooks. | High |

**Blocking gap discovered:** `studio-core` has the *shape* of credential isolation (`roles::profile_dir`, `SecretBroker::{mint,redeem}`) and **zero actual auth-material handling** — no keychain bridge, no token-file materialization, no refresh lifecycle. Every question in the OAuth/subscription/harness cluster lands on empty ground until that exists. This is the true start of the next phase.

---

## 2. Memory management (Q1)

### Findings
- The market has solved *remembering* and is failing at *not remembering* and at *forgetting*. Write-path discipline dominates retrieval architecture as a variable.
- **SWE-ContextBench** (`arXiv:2602.08316`) is the only coding-agent-specific memory benchmark (1,476 tasks, 51 repos, 9 languages). Its result is decisive and counterintuitive.
- **Mem0**, the most popular framework (66k stars), is as of April 2026 explicitly **ADD-only** — *"no UPDATE/DELETE. Memories accumulate; nothing is overwritten"* — and scored **worst of four** frameworks on the coding benchmark.
- No major framework has a production-grade deletion path. Verified erasure is unsolved market-wide.
- The best-in-corpus memory schema is still oh-my-pi `mnemopi` `MemoryRow` (`veracity`, `valid_until`, `superseded_by`, `trust_tier`, `recall_count`) with four-voice retrieval — which our shipped L3 already mirrors.

### Evidence

| Setting (SWE-ContextBench related pairs, Sonnet 4.5) | Resolved | Cost | Δ vs no-memory |
|---|---|---|---|
| No-Context | 26.26% | $0.79 | — |
| Free Context Learning (agent picks from full trajectories) | 26.26% | $0.98 | **no gain, +27% cost** |
| Free Summary Learning | **22.22%** | $0.91 | **−4 pts (worse than nothing)** |
| Oracle Context Learning | 27.27% | $0.77 | +1 pt |
| Oracle Summary Learning | **34.34%** | $0.85 | **+8.1 pts** |

Summary tokens averaged **217** vs **25,634** for full trajectories. `[VERIFIED: arXiv 2602.08316 §3.3, §5]`

Supporting: Claude Code's write path *"skips anything it can derive from the codebase"* with a hard **200-line / 25KB** always-on budget and a per-repo namespace **shared across all worktrees** `[VERIFIED: code.claude.com/docs/en/memory]`. Graphiti gives bi-temporal validity windows — facts are *superseded*, not deleted `[VERIFIED: github.com/getzep/graphiti]`. Supermemory is best-measured off the shelf (30.30% resolved, 74.51% top-1 at ~30 tokens) `[VERIFIED: arXiv 2602.08316 Tables 4–5]`. Codex's two-phase pipeline (leased rollout extraction → serialized consolidation against a git-baseline → `max_unused_days` / `usage_count` pruning) is the best engineering of *forgetting* found anywhere `[VERIFIED: openai/codex codex-rs/memories/README.md]`.

### Reasoning
The +8.1 pt gain came from **compact, correctly-selected, anchored** summaries. The −4 pt result came from *unfiltered* summaries. Same data, opposite outcomes. Therefore the load-bearing component is the **write path and selection policy**, not the vector index.

Our stack already contains the hard parts: an event-sourced ledger (which *is* Graphiti's "episodes"), receipts (which *is* provenance), a hash-chain plan ledger (a stronger supersession key than `isLatest` flags), and L3 lifecycle rows. Adding Mem0/Supermemory as a parallel store creates a **second history with no shared identity** — the documented recipe for stale-fact injection. Conversely, making memory a *projection* over the ledger makes erasure tractable: tombstone the event, rebuild the projection. That is one of only two viable paths to verified deletion anyone has named.

### Recommendations
1. **Keep the shipped Ladder (L0–L4).** It is validated, not superseded. Do not adopt a memory product as source of truth.
2. **Add a code-derivability deny-list to the write path.** Refuse anything derivable from the repo, git history, the ledger, or receipts. This is Claude Code's rule and the single highest-leverage change.
3. **Key validity windows to the plan-ledger hash chain.** L3 rows get `valid_from_seq` / `valid_to_seq` / `supersedes_id` pointing at plan hashes — bi-temporal semantics with cryptographic backing.
4. **Make the high-value artifact a ≤250-token summary anchored to `(path, symbol, commit_sha, plan_hash)`.** Never feed raw trajectories.
5. **Bound rung 3 hard:** `MEMORY.md` ≤200 lines / 25KB, topic files on demand. Budget rung 2 retrieval at 300–500 tokens.
6. **Namespace per-repo, not per-checkout** — worktrees must share one memory directory or parallel lanes fragment exactly when we need them not to.
7. **Build consolidation as a serialized, off-critical-path step** (Codex Phase-2 shape), not on the write path.
8. **L4 (vector) stays opt-in with mandatory reranker.** Do not gate anything on it existing.

---

## 3. Deployment topology: Docker stack vs desktop (Q2)

### Findings
- Docker-only fails the two hardest constraints we have: **macOS Metal GPU cannot enter a Docker VM at all**, and **Claude Code's OAuth lives in the macOS Keychain**.
- Bind mounts are **~3× slower than native on macOS** (virtiofs); Mutagen sync is 59% faster but paid. `[VERIFIED: paolomainardi.com 2025]`
- **Docker Desktop licensing** is free only for personal / <250 employees / <$10M revenue — an enterprise blocker to "just install Docker." `[VERIFIED: docs.docker.com/subscription-billing/desktop-license]`
- Comparable products split: **Goose** is native (Electron + Rust core + ACP); **OpenHands** is Docker-first *because it treats the agent as untrusted by default*. `[VERIFIED: github.com/Block/goose; docs.openhands.dev]`
- Worktrees created inside a container have `.git` pointers that resolve nowhere on the host unless paths are host-identical. `[VERIFIED: microsoft/TypeAgent#2365]`

### Evidence
| Dimension | Docker-only | Desktop-only | **Hybrid** |
|---|---|---|---|
| Install friction | High (Docker Desktop + license gate) | Low (signed installer) | Low, Docker opt-in |
| macOS GPU | **Broken** (Metal cannot enter VM) | Direct | GPU work on host |
| Git/FS perf | ~3× tax on bind mounts | Native | Native control plane |
| OAuth / keychain | **Broken by default** | Works | Works; workers get brokered tokens |
| Agent-write containment | Strong | **Weak** | **Strong at worker tier** |
| Headless / multi-machine | First-class | Weak | Natural |
| Fits Phases 1–4 as shipped | Requires re-architecture | Already matches | Additive: one `sandbox` adapter |

### Reasoning
Our differentiator is *orchestrating third-party harness CLIs against local git* — precisely the workload that fights containers (OAuth keychains, absolute worktree paths, weekly-updating CLIs). Containerizing the control plane taxes our core (worktree churn) to buy containment we can get more cheaply at the worker layer. Desktop-only is the inverse mistake: it gives up containment of agent-written code, which is a real risk once agents run arbitrary shell.

### Recommendations
1. **Native control plane.** `studio-core` stays a host binary, in or alongside the Tauri process. Tauri and the Ratatui TUI are *clients*, not the runtime.
2. **Optional sandbox worker tier** — disposable per-task rootless podman/docker (or Docker Desktop where present) that executes *agent-written code and harness CLIs*, never the scheduler. Opt-in capability, never a prerequisite.
3. **Host-identical bind paths** into workers; **engine owns all `git worktree add/lock/prune`**.
4. **Workers never receive** `docker.sock`, `~/.ssh`, `~/.aws`, `~/.gnupg`, or the host keychain. Short-TTL capability tokens only.
5. **Document rootless podman as the Linux fallback** — and its limits (`/etc/subuid`, no ports <1024, NFS home breakage).

---

## 4. Harness acquisition: BYO volume vs agent manager (Q3)

### Findings
- The four harnesses differ sharply on container-readiness: **Codex and OpenCode are container-friendly** (file-based auth, device-code login, `serve` modes); **Claude Code is the hard case** (macOS Keychain authoritative, **no RFC 8628 device-code flow** `[VERIFIED: anthropics/claude-code#44028]`); **Antigravity** needs `GEMINI_FORCE_FILE_STORAGE=true` or it loses tokens on every restart.
- Claude Code has a live **keychain-vs-file split-brain bug class** — client reports "Not logged in" while `~/.claude/.credentials.json` holds a valid Max credential. `[VERIFIED: anthropics/claude-code#91180, #91158]`
- Prebaking harness CLIs into images is the worst option on four axes at once: ToS/license risk (you are shipping Claude Code/Codex binaries), update-cadence skew (weekly CLI vs monthly image), OAuth death, and multi-arch build cost.

### Evidence
| Criterion | (a) BYO mount | (b) runtime installer | (c) prebaked image | **(d) hybrid** |
|---|---|---|---|---|
| Install friction | Lowest | Medium | High (N images × M arch) | Low — detect + reuse |
| Update cadence | None (user updates in place) | Best | **Worst** | Best — updates install *dir*, not image |
| OAuth / device flow | **Best** (keychain works on host) | Good if host-side | **Breaks** | Best — login on host |
| Multi-profile isolation | User must do it | Manager can | Per-container natural | Manager creates per-lane dirs |
| License / ToS | Clearest (user's own install) | Gray | **Highest** | Medium — "you own the harness" |

### Reasoning
Detection-over-configuration is the pattern OmniRoute ("CLI fingerprint matching") and pi (resolution order `--api-key` > `auth.json` > env) both use. The user who already runs Claude Code should not be forced through an install. The user who runs nothing should not be forced to install four things. The manager's job is *lane isolation and version pinning*, not distribution.

### Recommendations
1. **Probe first:** `which claude codex opencode pi agy` + known config dirs → register `source: host`.
2. **Install only what's missing** into `~/.studio/harness/bin/<name>/<version>/` — a **host-side** versioned directory, never an image layer.
3. **Never prebake harness CLIs into an image.** A worker *base* (Debian + git + node + python, no harness) is fine.
4. **Per-lane config dirs** created by the manager: `CLAUDE_CONFIG_DIR`, `CODEX_HOME`, `OPENCODE_CONFIG_DIR`, `~/.gemini/antigravity-cli`, with `GEMINI_FORCE_FILE_STORAGE=true` for Antigravity in workers.
5. **SecretBroker holds refresh material; workers get short-TTL access tokens only.** (Currently a stub — see §12.)

> **Conflict with `plans/phase 1/antigravity/` §3.2**, which recommends "Declarative Agent Manager with Prebaked Core Runtimes." I read that as the right *manager* and the wrong *prebake*. Prebaking harness CLIs is where the ToS and OAuth problems live. If "prebaked core runtimes" means a harness-free worker base image, we agree.

---

## 5. Persistence across image updates (Q4)

### Findings
Failure modes to design against: volume drift (one mega-volume with mixed ownership), `.git` pointer breakage from container-created worktrees, schema migrations against a live volume, and never-delete reapers (the in-corpus 100GB/8h incident).

### Recommendations — the layout

| Volume | Contents | Type | Survives update |
|---|---|---|---|
| `studio-state` | `studio.db` + WAL, `schema_migrations` via `PRAGMA user_version` | named | yes |
| `studio-evidence` | content-addressed sha256 blobs | named | yes |
| `studio-logs` | engine/scheduler/adapter JSONL, rotated | named | yes |
| `studio-harness` | per-harness config + auth, **per-lane subdir** | named | yes |
| `studio-toolchains` | node/python/uv/rustup caches, model cache | named (ro into workers) | yes |
| repos + `.worktrees/` | user-owned | **bind, host-identical path** | yes — never in an image |

Rules:
1. Engine and worker images version **independently**. Never put a harness CLI version in the engine's version string.
2. `VACUUM INTO` backup before every migration; `/state/.layout-version` stamp the engine refuses to boot against if unknown.
3. **One volume per class, never one mega-volume.** Each has a documented owner and prune policy.
4. **Worktrees live on the host.** Only their contents are bind-mounted. Engine owns all git ops.
5. `studio export` → db + evidence + config-without-secrets; secrets only under `--include-secrets` into an age envelope.

---

## 6. Subscriptions vs hand-built agents (Q5)

### Findings
- **Hand-built RolePacks work well for API-key and free-tier paths.** OpenCode Zen/Go are ordinary API keys despite the branding `[VERIFIED: opencode.ai/docs/providers]`; pi's provider table already lists them alongside ~30 others.
- **They cannot absorb OAuth subscriptions.** OpenCode states third-party harness use of Claude Pro/Max is *"explicitly prohibited by Anthropic"* and removed bundled plugins in 1.3.0 over it `[VERIFIED: opencode.ai/docs/providers]`. pi warns third-party harness usage *"draws from extra usage and is billed per token, not against Claude plan limits"* `[VERIFIED: pi providers.md]`.

### Reasoning
This is not a compromise, it is the only ToS-clean shape: **subscriptions ride harness delegation** (`auth_kind: cli_delegated` — the harness owns the OAuth dance), **API keys ride our Model Fabric**. Our ACP session bridge already *is* the delegation mechanism, so the work is metering and slot-binding, not reimplementing OAuth.

Note also that pi's provider table already includes **OpenCode Zen and Go**, plus subscription auth for Claude Pro/Max, ChatGPT Plus/Pro (Codex), and Copilot — so a pi/omp-based RolePack substrate gives us subscription *and* BYOK coverage from one config surface.

### Recommendations
1. Ship RolePacks bound to **model slots**, never providers.
2. `auth_kind: api_key | oauth | local | cli_delegated | cloud_iam` as a first-class enum.
3. Ledger rows carry `billed_kind` and are **allowed to flip** (subscription → per-token overage).
4. Never reimplement a harness's OAuth client as the default path.

---

## 7. LLM API & model management (Q6)

### Findings
- **OmniRoute is real and relevant** — and already running in this workspace. Combos with 19 strategies, Auto-Combo zero-config, budget headers (`X-OmniRoute-Budget`, `Budget-Fallback: strict` → 402), **Context Relay** (handoff summary at 85% quota on account rotation), one-click config of 10 coding CLIs. `[VERIFIED: github.com/diegosouzapw/OmniRoute docs/FEATURES.md]`
- **Unify has pivoted away** — now a continual-learning lab, not an LLM router. Do not build on it. `[VERIFIED: unify.ai]`
- LiteLLM supports Claude-Max only as **header passthrough** of a client-held OAuth bearer. BiFrost is a fast Go gateway. OpenRouter's "OAuth" **mints an API key** (not a refresh-token flow). Vercel meters BYOK spend **outside** budgets — a footgun not to copy.
- **Auto-routing is harmful for coding tasks.** Real harnesses (Claude Code, Codex, OpenCode, pi) all use explicit model selection. Auto-routing is an *aggregator* feature optimizing inference economics across anonymous prompts.

### Evidence — why auto-routing fails on coding `[INFERRED from verified router designs]`
1. **Tool-dialect churn** — a mid-task model swap breaks tool replay and `cache_control` markers.
2. **Context-window mismatch** — the router picks on request N; request N+50 is 180K tokens.
3. **Cache-cold hops** — every provider swap invalidates the prompt-cache prefix. OpenRouter's sticky routing + `session_id` and OmniRoute's `cacheAffinity` are both acknowledgments.
4. **Review independence erodes** — reviewer routing to the coder's family raises error correlation.
5. **Non-determinism** the doom-loop detector will misread as agent flakiness.

### Recommendations
1. **In-process Model Fabric** (Rust) — pi-ai-shaped unified client across `anthropic-messages` / `openai-completions` / `openai-responses`, per-model `cost{input,output,cacheRead,cacheWrite}`, retries, circuit breaker, session-sticky cache affinity.
2. **Credential store with discovery over configuration** — detect what's already logged in. Secrets in keyring; resolution supporting `$ENV` and `!cmd` (pi's pattern).
3. **Connection → Pool → RoleSlot.** The DAG names `role.coder`; the pool resolves it. Users edit order and membership, never routes. This is the "don't make me a router admin" surface.
4. **Pinned tiers by default.** Three onboarding profiles as editable rows: *Max-first* / *API-cheap* / *Local-only*. Per-role override for power users.
5. **Auto-routing opt-in per slot, forbidden on `coder.primary` and `reviewer`.**
6. **Sticky-per-task affinity.** One task = one connection. Fallbacks on hard failure / quota exhaustion only, never mid-task for cost.
7. **Budget arms on `input+output+cache_write`, excluding `cache_read`** (already correct in the roadmap). Three cap scopes: task, role-class, connection. Degradation ladder ends in a `blocked` receipt.
8. **Gateways are provider profiles, never a dependency.** OmniRoute is the strongest first integration — it is live here, and Context Relay is the only public design for OAuth account rotation.
9. **Honest 4-state connect UI:** pick source → connect with **live ping proof** (never "Connected" from a saved string) → assign roles showing *which account gets billed* → live status with a 24h staleness rule. Import harness configs read-only; export to harness is explicit with a diff preview.

---

## 8. Pi ecosystem: dedicated profiles, pi vs opencode (Q7)

### Findings

| Project | Stars | License | Role |
|---|---|---|---|
| `anomalyco/opencode` | **210,234** | MIT | mindshare + JSON-Schema config |
| `earendil-works/pi` | **109,563** | MIT | **refusal-plus-packaging** |
| `can1357/oh-my-pi` | **33,402** | MIT | batteries-included fork |
| `open-gsd/gsd-core` | **9.9k** | MIT | shipped production RolePack |

- **pi's differentiator is philosophical refusal, not features.** Four tools. Explicitly *No MCP. No sub-agents. No permission popups. No plan mode.* Distribution is a one-keyword npm manifest (`keywords: ["pi-package"]`). `[VERIFIED: earendil-works/pi README]`
- **RolePacks are already the ecosystem's center of gravity.** gsd-core ships **36 specialists + 72 skills** across 9+ harnesses **including a pi adapter** (`pi/gsd.cjs`). `wshobson/agents` has **40,004 stars**. Five of the top ~45 `pi.dev/packages` are role/subagent systems (`pi-subagents` 484K/mo, `@companion-ai/feynman` 181K/mo). `[VERIFIED: github.com/open-gsd/gsd-core contents/agents; pi.dev/packages]`
- **oh-my-pi promotes the concept to a typed subsystem:** `agents/*.md` with `output` (schema-validated yield), `spawns` allowlist, `autoloadSkills`, `readSummarize`, `advisor`, and **role-aliased models** (`model: "@review"` → `modelRoles.review: openai/gpt-5.4:high`). `omp --profile` relocates *every* OMP path — real multi-profile isolation. `[VERIFIED: review/oh-my-pi/docs/task-agent-discovery.md, config-usage.md]`
- **The decisive asymmetry:** oh-my-pi ingests **15 foreign config formats with OpenCode at priority 55**. An OpenCode-shaped pack is readable by omp today; the reverse is not true. `[VERIFIED: review/oh-my-pi/docs/config-usage.md]`

### Reasoning
The question "is it worth building dedicated profiles?" is answered by the market: this is not early, it is crowded. The scarce thing is therefore **not agent definitions** — gsd-core has 36 of them and `wshobson/agents` has 40k stars of them. What is scarce is a *verified completion contract* that works across harnesses. That reframes the answer: build one flagship RolePack to prove the runtime, treat the rest as ecosystem.

On substrate: oh-my-pi wins capability and isolation; base pi wins distribution; OpenCode wins mindshare. Binding to omp's `AgentDefinition` is lower-risk than base pi's `ExtensionAPI`, because base pi may absorb subagents/MCP and collapse the third-party market (110k stars creates convergence pressure, though the "Philosophy" section reads as principled restraint).

### Recommendations
1. **Author once against omp `AgentDefinition`**, emit three targets: omp-native `agents/` drop, base-pi `package.json#pi` shim, `.opencode/agents/` mirror.
2. **Grow `RolePack`** (currently name + tools + `skill_lockfile_hash`) with: system prompt, MCP slice, **model-slot bindings**, harness profile, `spawns` policy, `output` schema.
3. **Keep registration-parity tests** — they already catch the phantom-tool class (opencode-swarm v6.48.0 shipped 6 unregistered tools).
4. **Watch the base-pi convergence risk** quarterly against the `pi-subagents` / `pi-mcp-adapter` download curves.
5. **Extension entry must be `.ts`/`.js`** — gsd-core burned on `.cjs` being silently ignored by pi's `isExtensionFile()`.

> **Conflict with `plans/phase 1/antigravity/` §7.2** ("Optimal Division of Labor: OpenCode vs. Pi"). The omp-ingests-opencode asymmetry is the tiebreaker in my reading; if antigravity's split differs, this is the fact to argue from.

---

## 9. Cross-pollination: happier / gentle-ai / kasetto (Q8)

### Findings

| Project | What it is | Genuinely unusual strength |
|---|---|---|
| **happier-dev/happier** (1.7k★, MIT) | Multi-device control plane for harnesses you already have | **Single action registry + per-surface exposure matrix** (direct vs discoverable; per-surface allow/ask). **Non-escalating child sessions** — a child can never have more authority than its parent; requesting higher permission is **rejected, not downgraded**. |
| **Gentleman-Programming/gentle-ai** (7.3k★, Go, MIT) | Governs the agents you already use; ODD + RDD | **Frozen-candidate review authority**: freeze tree → risk tier → lens depth → **one bounded correction** → acknowledge token that is **burned**. Deterministic next-transition from a native binary. |
| **pivoshenko/kasetto** (205★, Rust, MIT/Apache) | Package manager for agent assets | **`kasetto.yaml` + `kasetto.lock` + content-hash sync.** Managed blocks in shared instruction files that survive hand-edits. `${kst_…}` secrets resolved at bind time, never in the lock. |

Also: **`gastownhall/beads`** (27.4k★) — typed task DAG as claimable work (`bd ready` / `--claim` / dependency release on close), the closest external match to our DAG. **`vibe-kanban` is sunsetting** (28.2k★, Rust) — a market signal that pure kanban-over-agents was not a sustainable product. **`smtg-ai/claude-squad` is AGPL-3.0** — read lifecycle states, never copy code.

### Steal list (ranked impact ÷ effort)

| Rank | Steal | From | Impact | Effort |
|---|---|---|---|---|
| 1 | Frozen-candidate review + burned authority, **extended to gate delivery** | gentle-ai | Very high | High |
| 2 | Action registry + surface exposure matrix | happier | Very high | Medium |
| 3 | Non-escalating child sessions (reject, never downgrade) | happier | High | Low |
| 4 | Lockfile-first content-hash asset sync | kasetto | High | Medium |
| 5 | Deterministic next-transition owned by the daemon | gentle-ai | High | Medium |
| 6 | Canonical tool normalization + `_raw` + fixture drift tests | happier | Medium | Low |
| 7 | Managed blocks in `CLAUDE.md`/`AGENTS.md` | kasetto | Medium | Low |
| 8 | Bind-time secret placeholders, never in the lock | kasetto | Medium | Low |
| 9 | Proportional routing (1–3 inline / 4+ delegate / heavy only on request) | gentle-ai | Medium | Low |

### Reasoning
These three are complementary rather than competing, and together cover roughly three of our pillars:
- **happier** gives *control-plane primitives* (how capabilities are exposed and bounded).
- **gentle-ai** gives the *evidence gate* — and notably **refused to gate delivery**, which is exactly where our Phase 3 merge queue already lives. Extending their freeze+burn to be what the queue requires is nearly free for us and is the highest-value single move available.
- **kasetto** gives the *reproducible substrate*. Worktree isolation is only actually reproducible if every worktree sees an identical tool surface.

### Recommendations
Adopt 1–5 as Phase 1 design inputs. Item 1 (frozen-candidate) is the one to prototype first — it is the moat-shaped item and the antithesis of "another kanban board."

---

## 10. Per-profile skills & MCP inventory (Q9)

### Findings
- Parallel worktrees that see different tool surfaces produce "works in my worktree" drift. kasetto solves this with content-hash locking across four asset kinds (skills, commands, MCPs, instructions), transformed per agent.
- Our Phase 2 already has compile-checked tool manifests + registration parity + the per-role schema-token scoreboard. The missing piece is **asset distribution**, not asset definition.
- Secrets must resolve at bind time. kasetto's model (env → cred file → 1Password/Vault/cloud KMS, never in the lock, sync fails closed on missing) is the right shape.

### Recommendations
1. `rolepack.yaml` + `rolepack.lock` with content hashes; `--locked` in CI and in worktree provisioning.
2. Managed-block upsert for `CLAUDE.md` / `AGENTS.md` — never full-file rewrite.
3. Secret placeholders resolved at bind time by `SecretBroker`; nothing secret in the lock or in agent settings files.
4. `extends:` layering: org → team → project → worktree.
5. Keep the per-role schema token cost scoreboard visible in the MCP Registry view.

---

## 11. Coding standards, static analysis & anti-circumvention (Q10)

### Findings

**Fallow is real and I was wrong to doubt it.** `[VERIFIED: github.com/fallow-rs/fallow + README]`
- `fallow-rs/fallow` — **4,900 stars**, Rust binary, **MIT**, created 2026-03-17, actively pushed.
- *"Codebase intelligence for TypeScript and JavaScript… unused code, duplication, circular deps, complexity hotspots, architecture boundaries, design-system styling drift."* Optional paid Fallow Runtime layer — **skip it**.
- Deterministic findings, typed output contracts, **no AI inside the analyzer**, no TS compiler or Node runtime needed.
- **`fallow audit` gates only what a PR changed** — the vitest excerpt shows *"audit gate excluded 163 inherited findings (run with --gate all to enforce)"*. That is exactly the severity-budget / baseline pattern, shipped. `--format json` returns one typed document; there is an **MCP integration** (docs.fallow.tools/integrations/mcp) and a version-matched agent skill.
- **Limit: TS/JS only.** It does not cover our Rust core. The Rust side stays clippy + tree-sitter.

**Other findings:**
- **OpenCode's own docs argue against LSP-as-gate:** *"Language servers can get out of sync, use significant memory, vary by version or project, and slow down agent workflows. In many projects it is better to have the agent run lint, typecheck, or other diagnostic CLI tools directly."* `[VERIFIED: opencode.ai/docs/lsp/]` OpenCode's LSP is **off by default**; Claude Code has none.
- **Prompt rules do not enforce anything.** Claude Code bypassed pre-commit via `--no-verify`, `git stash`, quiet flags, `core.hooksPath` override, and GitHub MCP write tools — **6 consecutive commits with failing tests despite explicit CLAUDE.md rules**. `[VERIFIED: anthropics/claude-code#40117]` `permissions.deny` has known gaps (equivalent-command construction; does not survive `bypassPermissions`).
- **SARIF `result.level` is optional.** CodeQL omits it and uses `rule.defaultConfiguration.level`. A naive `select(.level == "error")` gate passes on a repo full of errors. `[VERIFIED: trailofbits sarif-parsing skill]`
- **lefthook in worktrees** was a real bug (#901, `exit 128` after `git worktree add`, confirmed on 1.9.3) but is **closed**. Re-verify on current before adopting.

### Reasoning
Order matters more than tools: format (automatic) → fast deterministic gates → security at generation → slow backstops → judgment. Agents need **deterministic refusal**, not suggestions. Two consequences:

1. **The authoritative gate must be the same CLI diagnostics CI runs** — otherwise agent and CI disagree, and the agent optimizes to the wrong oracle.
2. **Anti-circumvention must be process-level.** The documented bypass vectors are mechanical, so the defenses must be mechanical.

### Recommendations — defense in depth

| Layer | What runs | Budget |
|---|---|---|
| 0 — agent loop | format-on-write; **`fallow guard <file>`** (read-only, on-demand) for TS/JS; ast-grep invariants; SARIF findings as fix-tasks | <2s, <500 tok |
| 1 — session (Stop) | tdd-gate, scope-guard, secret-scan, severity budget on the working diff | <10s |
| 2 — pre-commit | **`prek`** (Rust, pre-commit-compatible, monorepo workspace mode); `rustfmt --check`; `clippy -D warnings` on changed crates; `fallow audit --format json`; `ruff`/`biome`; gitleaks; **config-integrity** | <15s |
| 3 — pre-push | typecheck affected; unit tests affected; `cargo-audit`/`osv-scanner` on lockfile change; **marker-file freshness** | <90s |
| 4 — merge queue / CI | full matrix; Semgrep (custom rules); CodeQL on critical paths; trivy SBOM+CVE; `cargo-deny` licenses; **SARIF aggregate gating NEW findings only** | fail-closed |
| 5 — continuous | nightly audits; weekly CodeQL; auto baseline-ratchet PRs; rule-acceptance analytics | scheduled |

**Tools:** clippy + rustfmt + cargo-audit/deny (Rust) · **fallow** + tsc + biome/oxlint (TS/JS) · ruff + basedpyright (Python) · shellcheck + hadolint · ast-grep · Semgrep + CodeQL + trivy + osv-scanner + gitleaks. **Skip:** SonarQube for the inner loop (too slow; use as CI backstop only), Flawfinder (C/C++ only — *not* what "Fallow" meant), bandit (Semgrep covers it).

**Anti-circumvention (process-level, fail-closed):**
1. PreToolUse **command** hooks — not prompt hooks, not `permissions.deny` — blocking `--no-verify`/`-n`, `core.hooksPath` overrides, `HUSKY=0`/`LEFTHOOK=0`/`SKIP=*`, `git stash`+commit combos, quiet flags, and writes to gate configs (`.claude/settings.json`, `lefthook.yml`, `prek` config, `biome.json`, `clippy.toml`, `tsconfig.json`, `Cargo.toml [lints]`).
2. **Block the MCP write-through bypass** — deny `push_files` / `create_or_update_file` / `merge_pull_request` style tools that skip local hooks.
3. **Marker-file freshness:** pre-commit writes a tree-hash marker; pre-push rejects if stale. Survives `--no-verify` on commit.
4. **Config-integrity CI:** any diff to gate configs requires CODEOWNERS approval. Agents cannot self-approve.
5. **Suppressions never live in source.** Route `#[allow]` / `eslint-disable` / `# type: ignore` to a SARIF suppression file with required justification, owned by CODEOWNERS. (Fallow's design already excludes inherited findings by default rather than reading in-source ignores.)
6. **CI runs with ignores disabled** so the answer never depends on what the agent wrote into the file.
7. **Agent credentials are read-only for CI** — push lands as PR; merge requires Layer 4.
8. **Log every Bash command.** Git does not record `--no-verify`; our hooks must.

**Severity policy:** block **new** error-level findings only. A zero-findings gate on a legacy repo is how you teach an agent to mass-generate ignore comments.

> **Conflict with `plans/phase 1/antigravity/` §10.2**, which recommends lefthook and an in-loop managed LSP broker as primary. My reading: LSP as a *navigation/diagnostics aid* is fine (rust-analyzer, tsserver, basedpyright, gopls — in that order); LSP as *the gate* is not, per OpenCode's own guidance and CI-parity requirements. On lefthook, the worktree bug is closed — the remaining question is fan-out behavior under many concurrent agents, which `prek`'s workspace mode or `betterhook`'s worktree-native NDJSON design address more directly.

---

## 12. Cross-cutting finding: the auth-material gap

Regardless of how Q3/Q5/Q6 resolve, one thing is missing in the codebase today.

`studio-core/src/roles/mod.rs` has `profile_dir(base, lane)` and a `SecretBroker` with `mint`/`redeem`. That is the *shape* of credential isolation. There is **no**: keychain bridge (macOS `security find-generic-password`, Linux Secret Service), no token-file materialization, no refresh lifecycle, no `auth_kind` enum, no expiry/quota tracking.

Every OAuth/subscription/harness question above lands here. This is workstream 1 and it is blocking.

---

## 13. Dialectic summary

**Thesis (Agency OS):** ship a nine-agent agency out of the box — hand-built RolePacks, bundled Model Fabric, memory that compounds every week, containerized workers, burned-authority gates. The agency is a product, not a pile of configs.

**Antithesis:** *gsd-core has 36 specialists and `wshobson/agents` has 40k stars. Agent definitions are content, and content races to the bottom. The scarce thing is verified completion. Ship only the contract — a git-native evidence gate + merge queue — and let the ecosystem build the agency.*

**Synthesis:** the antithesis is partly right. RolePacks alone are content. But a contract needs a *runtime* to be enforceable across six harnesses, and the memory/evidence/metering substrate cannot be a config file. So: **build the runtime for contracts, ship one flagship RolePack to prove it, treat the rest of the pack market as ecosystem.**

### Feature vectors
| | Pitch | Falsification |
|---|---|---|
| **FV1 RolePack Fabric** | One source → omp / base-pi / opencode, lockfile-pinned, bound to model slots | `[HYPOTHESIS: compiled targets within 5% of omp-native on role-fidelity across 6 roles]` |
| **FV2 Subscription Bridge** | `cli_delegated` auth + quota-aware degradation + `billed_kind` flips | `[HYPOTHESIS: Max-subscription coder <20% of API-key USD cost while quota window holds]` |
| **FV3 Frozen-Candidate Gate** | Freeze → tier → one correction → burn; burn is what merge requires | `[HYPOTHESIS: ≥50% fewer review→fix→re-review loops on 20 tasks]` |
| **FV4 Memory as Ledger Projection** | Typed temporal facts over the hash chain + ≤250-token anchored summaries | `[HYPOTHESIS: deny-list write path stores ≥50% fewer items, preserves ≥95% task success]` |
| **FV5 Diagnostic Broker** | CLI diagnostics → SARIF → severity budget → `agent_prompt` fix-tasks | `[HYPOTHESIS: SARIF+agent_prompt out-fixes raw lint text at equal tokens]` |

### Lateral moves
1. **Containerize the blast radius, not the control plane** (§3).
2. **LSP-in-agent → CLI-diagnostics-as-data** (§11) — OpenCode's own advice plus CI parity.
3. **Auto-routing → pinned role slots with sticky-per-task affinity** (§7).

---

## 14. ADRs for Phase 1

- **ADR-P1-01 — Deployment topology:** native control plane + optional containerized worker tier. Docker is a capability, never a prerequisite. *(Reverses any compose-as-product reading.)*
- **ADR-P1-02 — Harness acquisition:** detect-then-install hybrid; host-side versioned install dir; **never prebake harness CLIs into images**.
- **ADR-P1-03 — Auth model:** `auth_kind` enum; OAuth subscriptions only via `cli_delegated` harness delegation; workers receive short-TTL access tokens only.
- **ADR-P1-04 — Model assignment:** Connection → Pool → RoleSlot; pinned tiers default; sticky-per-task affinity; auto-routing opt-in and forbidden on `coder.primary` / `reviewer`.
- **ADR-P1-05 — RolePack substrate:** omp `AgentDefinition` is the authoring contract; multi-harness emission; `rolepack.lock` content hashes.
- **ADR-P1-06 — Memory:** Ladder L0–L4 retained; memory is a rebuildable projection over the ledger; write-path deny-list mandatory; L4 opt-in with reranker.
- **ADR-P1-07 — Evidence gate:** frozen candidate + one bounded correction + burned acknowledgement; **the burned receipt is what the merge queue requires** (extending gentle-ai, which refused delivery gating).
- **ADR-P1-08 — Quality gates:** CLI diagnostics authoritative; Fallow for TS/JS, clippy for Rust; severity budget on NEW findings; suppressions only as SARIF objects; anti-circumvention is process-level.
- **ADR-P1-09 — Git hooks:** `prek` as default; re-verify lefthook worktree fix before adopting it; `betterhook` watched for NDJSON/worktree-native.

---

## 15. Workstream order

1. **WS1 — Fabric substrate (BLOCKING).** Auth-material subsystem (keychain bridge, token materialization, refresh lifecycle, `auth_kind`, quota tracking), Model Fabric, Connection/Pool/RoleSlot store. Without this, RolePacks have nothing to bind to and subscriptions do not work at all.
2. **WS2 — Contract runtime (the moat).** Frozen-candidate gate extended to delivery, deterministic next-transition, non-escalating spawn policy, action registry.
3. **WS3 — Content proof (one pack, not fifty).** Flagship RolePack on omp `AgentDefinition`, lockfile-pinned, three targets, Diagnostic Broker as its quality spine.

Explicitly out of scope (roadmap boundary): marketplace, cloud hosting, mobile companion, voice.

---

## 16. Open questions for the user

1. **WS1 ownership of keychain access** — macOS keychain bridging needs a design decision (host-side broker process vs Tauri-side plugin). Which?
2. **Worker tier priority** — is the sandbox worker tier (ADR-P1-01) in Phase 1, or a follow-on? It is additive and non-blocking for WS1/WS2.
3. **OmniRoute integration depth** — first-class provider profile only, or build on Context Relay for OAuth account rotation?
4. **Fallow scope** — adopt for the TS/JS surfaces (cockpit, adapters) now, given it is TS/JS-only and our core is Rust?
5. **Multi-harness ambition** — commit to three RolePack targets at v1, or ship omp-only first and gate multi-harness on H-C?

---

## Sources (high-signal)

**Memory:** arXiv 2602.08316 · code.claude.com/docs/en/memory · getzep/graphiti · arXiv 2501.13956 · supermemory.ai/docs · mem0ai/mem0 · openai/codex `codex-rs/memories/README.md` · MemTensor/MemOS
**Topology/OAuth:** docs.docker.com/subscription-billing/desktop-license · paolomainardi.com 2025 · anthropics/claude-code#91180 #91158 #44028 · openai/codex#9253 + `login.rs` · opencode.ai/v2/docs/cli/providers · microsoft/TypeAgent#2365 · docs.openhands.dev · Block/goose · SocialGouv/iterion
**pi:** earendil-works/pi · pi.dev/packages · `review/oh-my-pi/docs/{task-agent-discovery,config-usage,settings,providers,extension-loading}.md` · open-gsd/gsd-core · wshobson/agents
**Models:** docs.litellm.ai · maximhq/bifrost · openrouter.ai/docs · diegosouzapw/OmniRoute `docs/FEATURES.md` · opencode.ai/docs/providers · pi `providers.md` · vercel.com/docs/ai-gateway · unify.ai
**Steal list:** happier-dev/happier · Gentleman-Programming/gentle-ai · pivoshenko/kasetto · gastownhall/beads · BloopAI/vibe-kanban
**Quality:** **fallow-rs/fallow** · opencode.ai/docs/lsp · anthropics/claude-code#40117 · j178/prek · evilmartians/lefthook#901 · tupe12334/block-no-verify · trailofbits sarif-parsing · qodo-ai/qodo-skills · cursor.com/docs/bugbot

**Sibling reports:** `plans/phase 1/antigravity/STUDIO_PHASE_1_PLATFORM_REPORT.md`, `plans/phase 1/opencode/stc-next-phase-plan.md` — substantive agreement on hybrid topology, RolePacks, and non-bypassable gates; stated conflicts on prebaking, lefthook, and LSP-as-gate are marked inline above.
