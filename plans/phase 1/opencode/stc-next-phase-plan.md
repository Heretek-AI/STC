# STC Next Phase — Research Findings, Evidence, Reasoning & Recommendations

Date: 2026-09-26. Status: brainstorm/research complete. No code written (plan mode).
Source interviews: 10-question brainstorm session; web research conducted same day.
Constraints carried forward: local-first; four pillars unchanged (Manager→DAG dispatch,
worktree isolation, MCP scoping, cockpit); no marketplace / cloud hosting / voice / mobile.

> NOTE ON LOCATION: this file was staged here because plan-mode restricts plan
> writes to this directory. Intended final home: `plans/phase 1/opencode/`
> (currently exists, empty). Move: `cp /home/john/.opencode/plan/stc-next-phase-plan.md
> "/home/john/Projects/STC/plans/phase 1/opencode/"`.

---

## 0. Decisions already locked this session

| # | Decision | Rationale in one line |
|---|---|---|
| D1 | Compose stack over desktop-app-first | Core is already daemon-shaped; Tauri/TUI become clients of the stack |
| D2 | BYOK mount (day one) + declarative agent manager (product path), converging on RolePacks | Escape hatch now, opinionated path later |
| D3 | Forgo "bring your agent"; hand-build pi/opencode profiles; user brings keys/subscriptions; Claude/Codex CLIs = mounted legacy lanes | OAuth can't be extracted; Go/Zen turn subscriptions into URL+key |
| D4 | Integrated gateway (Bifrost sidecar), Go/Zen defaults, preconfigured model tiers, user-overridable; generate pi/opencode provider configs from one WebUI | Single pane of glass, native execution |
| D5 | Pi as profile substrate, opencode as fallback lane; 5-profile starter library | Pi packages are the ideal RolePack unit; ACP bridge already abstracts the difference |
| D6 | Rootless (Docker) lanes by default; socket-sharing dev-mode only via `.env` + explicit override yaml | Socket = root-equivalent; untrusted model code must never reach it silently |
| D7 | `.env` (`STUDIO_LANE_RUNTIME`) + `.env.example` + `compose.override.dev.yaml` + first-run wizard + always-on dev-mode banner/badge | Loud privilege, gitignored `.env` |

---

## 1. Memory management

### Findings
- The 2026 market splits memory into **working memory** (session files: GSD-core `STATE.md`/`CONTEXT.md`, opencode-swarm ledger pattern) and **long-term cross-session memory** (Mem0, Zep/Graphiti, Letta, Engram, Cognee, Supermemory).
- **Engram** (Gentleman-Programming, ~6.3k stars, Go, **SQLite + FTS5**, MCP + HTTP + CLI + TUI, agent-agnostic, local-first, git-sync sharing, `gentle-engram` pi package): closest match to STC's Phase-5 Memory Ladder L1/L3. Evidence: repo docs + package listing.
- **Zep/Graphiti**: best temporal/contradiction handling; operational cost (service to run; Graphiti-only = build your own storage/ops).
- **Mem0** (~49k stars): fastest integration, personalization-focused, weak on institutional code knowledge, reported 7–8s recall latency — wrong shape for inner-loop coding.
- **Letta**: memory-as-runtime locks you into their framework — wrong shape for harness-agnostic platform.
- **pi-hermes-memory** (31k/mo, SQLite FTS5, token-aware policy-only memory): independent convergence on STC's architecture; second source.
- Best compaction strategy found: GSD-core's fresh-context subagents + structured artifacts (beats summarizers for coding work).

### Reasoning
STC's L0 (git receipts/ledger) is the moat and stays. L1/L3 as built are a subset of Engram — rebuilding is waste. L4 must stay gated (unranked vector hits near a verification gate = nondeterminism in the discipline layer).

### Recommendations
1. **Adopt Engram as L1/L3 substrate** (vendor or port); keep L0 ledger, L2 markdown, gated L4.
2. Audit `pi-hermes-memory` as second source.
3. Adopt GSD-style fresh-context + artifact compaction over "smart summarizer" approaches.
4. Spike exit criterion: Engram MCP wired to one lane, cross-session recall demonstrated, FTS latency measured <500ms on a 10k-doc store.

## 2. Docker stack vs desktop app

### Findings
- STC core (daemon + SQLite + git + tool gateway + `/snapshot` polling UI) is already server-shaped.
- Happier/Happy (2026 breakout control-plane products, MIT, Claude/Codex/OpenCode/Gemini/Pi support): validated the thesis — *be the control plane, not the agent*; thin clients over a relay; explicitly no marketplace/hosting/voice scope creep.
- Tauri cost already paid once: webkit deps, signing, per-OS bundles.

### Reasoning
Compose gives lane-per-container isolation mapping 1:1 to the worktree/session model, cgroup resource limits per agent, restart-policy crash recovery — all currently hand-built. UI invariant ("projection of studio.db") is transport-agnostic; the React app already polls HTTP.

### Recommendations
1. Compose stack becomes the product; Tauri/TUI/phone become clients.
2. `studio up` wrapper with profiles (`core`, `full`, `gpu`); keep native binary path for contributors.
3. Docker prerequisite accepted; mitigated by one-command up.

## 3. Agent handling: mount vs manager

### Findings
- **kasetto** (Rust, declarative `kasetto.yaml`, syncs skills/MCPs/instructions across harnesses, v3.9.0, agents+hooks management on roadmap): the exact "agent manager" shape requested, but small (~200 stars) — integrate the pattern, pin loosely.
- **gentle-ai** (`install/sync/doctor`, curated skills, per-machine, never installs agents): philosophically aligned with STC scope boundaries.
- Mounting `~/.claude`/`~/.codex`/opencode configs into lanes is ~20 lines of compose and sidesteps all token-handling liability.

### Reasoning
Different users, different answers: existing-setup users need zero-friction mount; new users need opinionated setup. Both converge if RolePacks are the desired-state unit.

### Recommendations
1. Day one: volume-mount escape hatch for existing configs.
2. Product path: kasetto-compatible declarative sync applying RolePacks into lanes.
3. Absorb-vs-integrate decision on kasetto deferred until it proves velocity or stalls.

## 4. Persistence across image updates

### Findings
Standard practice: named volumes/bind mounts for all state; `PRAGMA user_version` forward migrations in entrypoint; git as ultimate backstop (Engram git-sync chunks = same idea); `sqlite .backup` discipline.

### Reasoning
STC already has the primitives (`migrate()`, ledger, `refs/preserved/*`); needs formalizing, not inventing. The feared failure is silent unbounded growth (already an invariant) and `docker volume prune` by the user.

### Recommendations
1. Only named volumes persist (db, memory, ledger, checkouts); images stay stateless; update = pull + recreate.
2. Version table + entrypoint migrations.
3. `studio backup` command from day one of the stack.

## 5. OAuth CLIs vs hand-built agents on BYOK keys

### Findings
- Claude/Codex OAuth is non-extractable — those tools only work inside their CLIs.
- **OpenCode Go** ($10/mo, OpenAI-compatible, works with any harness incl. Pi; generous open-model limits) and **Zen** (pay-per-request, zero-markup gateway) convert subscriptions into URL + key.
- Qwen3.7 Plus-class models currently lead intelligence-per-dollar for agentic coding.

### Reasoning
Hand-building profiles on pi/opencode covers ~90% of the capability matrix with ~10% of the auth pain, and STC's Tool Lens + verification gates apply uniformly instead of fighting per-CLI permission models.

### Recommendations
1. First-class: hand-built pi/opencode profiles on user-supplied keys/subscriptions.
2. Second-class "legacy lanes": mounted Claude/Codex CLIs.
3. Never attempt OAuth extraction.

## 6. LLM API management

### Findings
- **Bifrost** (Go, web UI, virtual keys, hierarchical budgets, MCP filtering, adaptive LB) vs **LiteLLM** (100+ providers, Python, incumbent): Bifrost fits STC's stack aesthetic and needs; LiteLLM's breadth only matters for exotic backends.
- Pangolin critique noted: virtual keys ≠ identity — keep gateway behind STC's own auth/capability-token boundary.
- gentle-ai per-phase model routing (Opus-plan/Gemini-build style) validates tiering.

### Reasoning
Users shouldn't manage provider sprawl; STC shouldn't become a billing company. Gateway sidecar + curated defaults + escape hatches.

### Recommendations
1. Bifrost sidecar in compose; Go + Zen preconfigured; custom endpoints allowed.
2. Three shipped tiers — flagship (Manager/Reviewer), workhorse (Coder), disposable (Researcher/scanners) — remappable per-role in the MCP Registry view (becomes the model console).
3. WebUI collects credentials once, generates native pi/opencode provider configs into lanes.

## 7. Pi profiles: worth it, pi vs opencode, fork watchlist

### Findings
- Pi: minimal core + skills/markdown + extensions + npm/git packages (2000+), `pi install` one-liners, 4 modes (interactive/print/JSON/RPC/SDK), OpenClaw built on its SDK.
- oh-my-pi (33.4k stars): batteries-included proof; vanilla pi: minimal proof. Same package unit either way.
- Forks: **open-gsd/gsd-core + gsd-pi** (phase loops, fresh subagents, worktrees, cost ledger — overlaps Pillars 1–3; validation + competition), **LaziPi** (distro model), **Dirac** (hash-anchored/AST edits — audit vs write-intent fabric), **beads** (file-based planning — overlaps plan ledger).
- Criticism on record: pi-with-everything bloat costs more tokens than opinionated defaults ("saving cents, losing dollars") — curation matters more than coverage.

### Reasoning
Pi packages are the ideal RolePack unit: versionable, auditable as text, one-command install. opencode keeps value via TUI + Go/Zen billing. ACP bridge abstracts the runtime difference.

### Recommendations
1. Author profiles on pi; keep opencode as fallback lane.
2. Starter library (5): `coder-web`, `coder-systems`, `reviewer-security`, `researcher-docs`, `qa-tests` — each = skills + MCP slice + model tier, shippable as pi package AND opencode config from one RolePack source.
3. Audit Dirac's edit engine; track gsd-core as competitor-validation.

## 8. Happier / gentle-ai / kasetto — steal list

- **happier**: control-plane-not-agent thesis; normalized cross-provider tool/diff display; one-inbox lanes; relay sync for TUI/phone visibility. Steal framing + relay pattern.
- **gentle-ai/Engram**: memory substrate (Q1) + **GGA** provider-agnostic pre-commit review (DoD with someone else maintaining it) + per-phase model routing validation.
- **kasetto**: declarative-env pattern for RolePack installation (Q3); pin loosely, absorb if it stalls.

## 9. Skills/MCP per profile

### Findings
- Sources: Gentleman-Skills (627 stars), pi.dev packages (filter by downloads: `pi-subagents`, `cc-safety-net`, `pi-hermes-memory` proven), claude-code-templates hooks catalog, Felo skill bundles (skills-not-extensions discipline).
- Threat model (flagged in the wild): malicious skills blocked pre-commit — skills are arbitrary code execution.

### Reasoning
STC built enforcement (manifests, `tool_open`, overflow); missing curation + governance. Pin-by-hash (lockfile hashing already built) + skill-install covered by write-authority.

### Recommendations
1. Curate, don't aggregate: start from the proven-core lists above.
2. Enforce RolePack lockfile pins through `pi update` (never floating).
3. Extend write-authority model to skill installation; add malicious-skill pre-commit hook.

## 10. Coding standards stack

### Findings
- **Fallow** (`fallow-rs/fallow`, 4.9k stars, MIT, Rust binary, TS/JS): dead code with proof (`--trace`), duplication (strict→semantic), complexity + 0–100 health score, **architecture boundary presets**, `audit` changed-files gate (fails only on introduced findings — matches Bors philosophy), auto-fix dry-run, MCP server, version-matched agent skill, Claude hooks, typed JSON + deterministic reruns, SARIF output. Limits: TS/JS only (not STC's Rust core); paid Runtime layer irrelevant — skip it.
- **Semgrep Guardian** (June 2026: MCP + hooks + skills, scans on every file write, blocks hallucinated deps/secrets): generation-time SAST built exactly like STC Phase 2/3.
- GitGuardian (secrets depth via plugins + pre-commit), SonarQube/Cloud (trends/coverage backstop, too slow for inner loop), lefthook (lane-standard hook manager), claude-code-templates hooks (curate, don't adopt wholesale).

### Reasoning
Order matters more than tools: format (automatic) → fast deterministic gates (fallow audit) → security at generation (Guardian) → slow backstops (Sonar) → judgment (GGA/reviewer). Agents need deterministic refusal, not suggestions.

### Recommendations (adopted stack)
1. Formatter automatic in-lane, gated in receipts.
2. **Fallow `audit --format json`** as DoD evidence check (tests-green, syntax-ok, fallow-clean); lefthook pre-commit + merge queue.
3. `fallow guard <file>` exposed to coder lanes as read-only on-demand tool.
4. Semgrep Guardian MCP+hooks as generation-time security gate.
5. SonarQube/Cloud as CI backstop; GitGuardian for secrets depth.
6. Curate 5–10 claude-code-templates hooks into RolePacks as pi extensions + lefthook entries.
7. TS/JS only for fallow — Rust core keeps clippy + tree-sitter gate.

---

## Locked follow-up decisions (from session close)
- Lanes: rootless Docker default; socket-sharing dev-mode only (`.env` + override yaml + banner/badge).
- `.env` gitignored; `.env.example` committed; wizard writes `.env`, refuses privileged-dev without interactive yes.
- GPU lanes provisional pending rootless CDI `nvidia-smi` smoke test.

## Suggested spike sequence (each independently shippable, exit-gated)
1. **Engram eval** (1 day): MCP to one lane, cross-session recall demo, FTS <500ms @10k docs. Kill: recall misses or ops burden > a day.
2. **Guardian trial** (1 day): MCP+hooks in one lane, blocks a seeded vuln + hallucinated dep. Kill: false-positive rate blocks legit work.
3. **Compose spike** (2–3 days): Bifrost sidecar + Go/Zen defaults, rootless lanes, `studio up` profiles, named-volume persistence + migration entrypoint. Kill: rootless breaks the core lane workload.
4. **Five pi profiles as RolePacks** (3–5 days): skills+MCP+model tier each, lockfile-pinned, installable both ways.
5. **lefthook + GGA + fallow into merge queue** (2 days): DoD evidence extended, tap-out semantics unchanged.

## Open questions for the user
- Q10 leftover: none remaining ("fallow" resolved → adopted above).
- GPU CDI smoke test: which host runs it first?
- kasetto: integrate pattern now, or time-box a deeper eval first?
- Signing cert for installers: required before any distribution, still unowned.
