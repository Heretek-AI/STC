# harvest-review-01 — Darkharvest adoption roster (Gate-1 close)

**Arena:** rerun darkharvest over `review/` to find what STC can take to accelerate v2 development  
**Run:** `harvest-review-01` · **Cycle:** 1 of max 5 · **Synthesized:** 2026-09-29T03:16:19.216369+00:00  
**Dispatch:** MCP `iumbtems_*` swarms failed at backend (`RuntimeError: opencode backend execution failed`) — reported, not debugged; swarms ran via factory subagent roles (`darkharvester`, `brainstormer`).  
**Frontier:** `.factory/frontier.json` settled snapshot sha256 `6c2cbb6dce11bb3f…`

## Swarm evidence (manager-verified)

| Artifact | sha256 | Verification |
|---|---|---|
| darkharvest dossier.json | `17f5ceaaa0a46b78…` | **60/60 citation hashes recomputed = MATCH**; 35/35 items cited |
| darkharvest REPORT.md | `7eace08f30303851…` | hash match |
| brainstorm REPORT.md | `9ed6ddfab988dfe7…` | **32/32 citation hashes recomputed = MATCH** |
| crates.io proof: acp-http-adapter | `cb859f8eac29ee41…` | live re-check: 0.4.2, Apache-2.0 |
| crates.io proof: ts-rs | `1ab2761898dd373c…` | live re-check: 12.0.1 stable, MIT |

## Roster

**35 adoption items** (P02:9 P03:8 P04:4 P05:8 P06:6) **+ 9 brainstorm vectors** · 6 DEFERRED · 6 skipped (license-guard).

### Top-10 ranked adoptions

| # | Item | Verdict | Phase | Effort | Source |
|---|---|---|---|---|---|
| 1 | omp AgentDefinition contract + frontmatter serializer + discovery precedence/validation | clean-room (format compatibility required) | 06 | days | oh-my-pi (MIT) |
| 2 | Tool approval tiers (read/write/exec) + object policies (allow/deny/prompt, override+reason) + unknown/malform | clean-room | 03 | days | oh-my-pi (MIT) |
| 3 | Approval request/status/outcome protocol + in-process approval service | vendor+clean-room | 02 | hours | BloopAI/vibe-kanban (Apache-2.0) |
| 4 | MCP adapter charter: manifest-declared tools -> capabilities, catalog admission ceilings, schema/description b | clean-room | 03 | week+ | nearai/ironclaw (Apache-2.0) |
| 5 | axum projection serving: SSE combined history+live stream, WS snapshot+json-patch deltas with Ready frame | depend+clean-room | 04 | days | BloopAI/vibe-kanban (Apache-2.0) |
| 6 | Versioned execution-envelope/receipt schemas built from provider observations, never assistant prose | clean-room | 02 | days | yylo-dev/yylo (MIT) |
| 7 | Tool-lens primitives: enable/disable by keys+tags (disabled = not listed and not callable), search-tools trans | clean-room | 03 | days | jlowin/fastmcp (Apache-2.0) |
| 8 | Durable approval store + scoped capability leases + CAS records + auto-approve/persistent-approval policy | clean-room | 02 | week+ | nearai/ironclaw (Apache-2.0) |
| 9 | Normalized AgentEvent union + HookProvider interface with protocolVersion (refuse unknown provider versions) | clean-room | 05 | days | pixel-agents-hq/pixel-agents (MIT) |
| 10 | Attention-first state buckets (needs_input>failed>running>attention>done) + permission notification kinds | clean-room | 05 | hours | getpaseo/paseo (Apache-2.0) |

### Full matrix (35)

| ID | Phase | Verdict | Effort | Item | License |
|---|---|---|---|---|---|
| `f02-acp-http-adapter` | 02 | depend | days | Published Rust crate: minimal ACP HTTP-to-stdio adapter (acp-http-adapter) | Apache-2.0 |
| `f02-ic-approvals` | 02 | clean-room | week+ | Durable approval store + scoped capability leases + CAS records + auto-approve/persistent-approval p | Apache-2.0 (dual MIT OR Apache-2.0) |
| `f02-ic-runtime-policy` | 02 | clean-room | days | Deterministic monotonic runtime-policy resolver (reduce-only, fail-closed, explicit yolo ack, audit  | Apache-2.0 (dual MIT OR Apache-2.0) |
| `f02-ohmpi-acp-gate` | 02 | clean-room | days | ACP permission gate: fixed required-tool map, 4 permission options, unknown option IDs fail closed,  | MIT |
| `f02-vk-approvals` | 02 | vendor+clean-room | hours | Approval request/status/outcome protocol + in-process approval service | Apache-2.0 |
| `f02-yylo-lease-fence` | 02 | clean-room | days | Fencing lease tokens gating every mutating task operation (start/sync/checkpoint/evidence-run) | MIT |
| `f02-yylo-merge-landing` | 02 | clean-room | days | Native-git delivery adapter: CAS expected-old ref updates, QUEUED/CONFLICT/GIT_INTEGRATED states, ap | MIT |
| `f02-yylo-receipts` | 02 | clean-room | days | Versioned execution-envelope/receipt schemas built from provider observations, never assistant prose | MIT |
| `f02-yylo-risk-tiers` | 02 | clean-room | days | Risk-tiered validation depth: deterministic candidate risk plan, review sequence bounds, admitted-vs | MIT |
| `f03-agentdeck-mcppool` | 03 | clean-room | days | MCP process pool: socket proxy/HTTP server lifecycle, scope launcher, FD-leak regression tests | MIT |
| `f03-approval-tiers` | 03 | clean-room | days | Tool approval tiers (read/write/exec) + object policies (allow/deny/prompt, override+reason) + unkno | MIT |
| `f03-fastmcp-cli` | 03 | clean-room | days | Schema-to-CLI generation: typed subcommands + --help + SKILL.md companion generated from MCP tool sc | Apache-2.0 |
| `f03-fastmcp-lens` | 03 | clean-room | days | Tool-lens primitives: enable/disable by keys+tags (disabled = not listed and not callable), search-t | Apache-2.0 |
| `f03-mcp-admission` | 03 | clean-room | week+ | MCP adapter charter: manifest-declared tools -> capabilities, catalog admission ceilings, schema/des | Apache-2.0 (dual MIT OR Apache-2.0) |
| `f03-openchamber-toolmatch` | 03 | clean-room | hours | Tool-name wildcard rule grammar with hard bounds (single trailing *, length caps, 16 tools max) | MIT |
| `f03-swe-aci` | 03 | clean-room | hours | Composable minimal ACI tool bundles + submit/review gate as YAML config | MIT |
| `f03-vk-mcpconfig` | 03 | clean-room | days | Cross-harness MCP config read/write (JSON/TOML/JSONC) with comment preservation for per-executor MCP | Apache-2.0 |
| `f04-herdr-api` | 04 | clean-room | days | Schema-first daemon API with event hub, subscriptions, and wait-for-output semantics | Apache-2.0 |
| `f04-sandbox-daemon` | 04 | clean-room | hours | Daemon ensure-running with PID/version files and restart-on-version-mismatch | Apache-2.0 |
| `f04-ts-rs` | 04 | depend | hours | Rust->TypeScript type export (ts-rs) for cockpit API contracts | Apache-2.0 |
| `f04-vk-stream` | 04 | depend+clean-room | days | axum projection serving: SSE combined history+live stream, WS snapshot+json-patch deltas with Ready  | Apache-2.0 |
| `f05-agentdeck-terminal` | 05 | clean-room | days | Embedded VT terminal + web projection of live sessions (Go/charmbracelet x/vt) | MIT |
| `f05-herdr-detect` | 05 | clean-room | days | Versioned per-agent detection manifests (bundled/remote/override) with explain output and visible wo | Apache-2.0 (vendored libghostty-vt MIT) |
| `f05-oh-acp-authprobe` | 05 | clean-room | hours | Per-harness ACP auth-status probes for onboarding/Providers view (loggedIn JSON, stderr text, creden | MIT |
| `f05-openchamber-multirun` | 05 | clean-room | days | Multi-run compare (up to 5 models, per-run worktrees) + guided changes walkthrough | MIT |
| `f05-openfang-providers` | 05 | clean-room | hours | Providers view plumbing: health-probe cache with TTL + budgets endpoint on axum | MIT OR Apache-2.0 |
| `f05-paseo-buckets` | 05 | clean-room | hours | Attention-first state buckets (needs_input>failed>running>attention>done) + permission notification  | Apache-2.0 |
| `f05-px-events` | 05 | clean-room | days | Normalized AgentEvent union + HookProvider interface with protocolVersion (refuse unknown provider v | MIT |
| `f05-vk-kanban-diff` | 05 | clean-room | days | Kanban board + in-app diff/review workflow (task states, execution processes, head-SHA-fenced feedba | Apache-2.0 |
| `f06-handoff-pipeline` | 06 | clean-room | days | Handoff generation pipeline (generated handoff docs from session state) as evidence for inter-role e | MIT |
| `f06-meta-bus` | 06 | clean-room | hours | Tagged message bus schema for multi-pack teams: cause_by/sent_from/send_to + MessageQueue | MIT |
| `f06-omp-agentdef` | 06 | clean-room (format compatibility required) | days | omp AgentDefinition contract + frontmatter serializer + discovery precedence/validation | MIT |
| `f06-omp-spawns` | 06 | clean-room | hours | spawns policy resolution: disabled/explicit-list/unrestricted with default agent and typed rejection | MIT |
| `f06-opencode-agents` | 06 | clean-room (format compatibility required) | hours | .opencode agent markdown format (mode/description/color frontmatter + prompt body) and workspace ope | MIT |
| `f06-orca-versioned-skills` | 06 | clean-room | hours | Version-matched skills served by the binary (stub SKILL.md points to `orca skills get`) + versioned  | MIT |

### Brainstorm vectors (9) — kill-gated spikes

| # | Vector | Phases | Saving | Kill criterion |
|---|---|---|---|---|
| V1 | Replay Flight Recorder | 02, 06 | 3-6 days + zero-spend inner loop | 5 trace cases can't run offline/deterministically in 20 runs, or need a live model |
| V2 | Gate Mutation Calibration | 02 (all) | 1-2 weeks of false-green rework | <11 of 12 seeded mutants detected with typed reasons, or any needs human eyeball |
| V3 | Tool Misbehavior Fixture Farm | 03 | 3-5 days | Any malicious fixture completes without typed denial, or test needs network/daemon |
| V4 | Substrate Twin + Differential Matrix | 02/04/05/06 | 1-2 weeks | <70% normalized event alignment, or twin can't reproduce one scripted error/approval turn |
| V5 | Spawn-Policy Decision Matrix | 02, 06 | 3-5 days | <90% of matrix cells generate unambiguously, or any false denial on a declared allow |
| V6 | Discovery-Precedence Round-Trip Oracle | 06 | 3-7 days | Oracle disagrees with upstream rules on >2/10 hand-checked cases, or rejects >20% valid emissions |
| V7 | Projection Catalog Codegen | 04, 05 | 4-8 days | Generated first view exceeds hand-written LOC, or can't express stale/empty/error semantics |
| V8 | Chronofault MS-1 Evidence Gate | 04, 05 | 2-4 days, reused 5x | >1 flake in 10 runs, or any assertion needs mock data |
| V9 | Behavioral Fidelity Bench | 06 | 3-5 days + de-risks kill-gate | Bench can't discriminate a deliberately broken adapter (good >=90% vs broken <60%) |

### DEFERRED (out-of-scope attractors; in-scope variants proposed)

- **Mobile approval relay (push -> one-tap) as shipped by paseo/orca/agent-deck conductor** (ph04/05) — GOAL.md scope: no mobile code. In-scope variant proposed instead: cockpit Ask-policy surface + desktop notification (paseo attention payload patterns) with the same one-tap decision contract.
- **Cluster-scale declarative task orchestration (google/ax Task/Workspace CRDs, agent-substrate sandboxes)** (ph02-06) — Cloud/cluster hosting is out of scope; STC is a homelab solo builder harness. Revisit only if a remote-lane requirement lands.
- **Marketplace-style capability packs (OpenFang Hands, herdr plugins marketplace, fastmcp server registry installs)** (ph06) — No marketplace in scope. Internal analog: signed RolePack catalog with lockfile hashes.
- **Multi-tenant/cloud runtime profiles (ironclaw Enterprise*/Hosted multi-tenant branches, OpenHands cloud backends)** (ph02) — Out of scope; only the monotonic/fail-closed local policy mechanics are taken.
- **Voice control (paseo voice mode)** (ph05) — GOAL.md scope: no voice code.
- **TUI multiplexer lane (herdr/agent-deck/claude-squad style) as primary surface** (ph05) — Roadmap carry-over dropped tui in favor of browser-first cockpit; terminal visibility only if MS-2 needs raw PTY view (see f05-agentdeck-terminal).

### Skipped / license-guard

- `smtg-ai/claude-squad` — AGPL-3.0: Copyleft floor: clean-room patterns only, and its worktree/tmux patterns are already covered by Apache/MIT sources. No code or UI assets.
- `manaflow-ai/cmux` — GPL-3.0: Copyleft + macOS/Swift-only terminal; nothing Rust-portable was unique. Clean-room only.
- `multica-ai/multica` — NOASSERTION (Apache-2.0 + additional conditions): Non-floor custom license (redistribution conditions). Treat as clean-room reference for board-style agent presence only.
- `humanlayer/humanlayer` — Apache-2.0 (upstream deprecated): README states repo is deprecated ('pretty much all deprecated'); approval-store patterns superseded by vibe-kanban + ironclaw in this dossier.
- `zeroclaw-labs/zeroclaw` — MIT OR Apache-2.0: Rust but channel/bot-platform oriented (Discord/Lark/Matrix); no unique phase 02-06 mechanism found beyond ironclaw-class patterns. Low-signal for cap.
- `openai/symphony, awslabs/cli-agent-orchestrator, langchain-ai/open-swe, coleam00/Archon, generalaction/emdash` — varies (MIT/Apache-2.0): Adjacent multiplexers/worktree UIs; no mechanism beyond the already-claimed sources (vibe-kanban/herdr/paseo/agent-deck). Depth-3 expansion stopped at cap.

## Gate-1 acceptance

- [done] Roster presented with per-item hashed evidence (FILE_HASH_ONLY floor; manager recomputed all)
- [PENDING] Explicit user approval of adoption scope (this gate)
- [after] Approved items mapped to phase annexes citing phase hashes
- [after] Out-of-scope remains DEFERRED (no silent adoption)
- [after] Depend items carry lockfile-impact notes per GOAL.md 6

## Next after approval

1. Write `.roadmap/02-contract-runtime/` adoption annex citing darkharvest hashes; fold in vectors V1/V2/V5.
2. Spawn programmer (phase 02) → qa-a + qa-b → tiebreak → user sign-off.
3. Repeat per phase in build order (03 → 04 → 05 → 06); per-phase sign-off remains the gate.

## Honest limitations

- All corpus claims are `FILE_HASH_ONLY` (file + sha256 recomputed by manager; no content-addressed quote re-verification — iumbtems source cache not used).
- Effort-saved estimates are engineering judgment (bands), each backed by a cheap falsifiable spike (vectors) or port risk note (adoptions).
- omp/paseo/vibe-kanban evolve; contract pins (e.g. omp 18.3.2, omp field names) are re-validated at execution time.
