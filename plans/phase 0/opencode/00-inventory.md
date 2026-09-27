# Studio — 00 Inventory & Cherry-Picks (Phase 1)

Source: read-only audit of `/home/john/Projects/STC/review` (12 dirs). No code generated.

## 1. Matrix

### paseo — `paseo/docs/architecture.md:22-53,361-388`, `docs/agent-lifecycle.md`, `docs/data-model.md`
- Local-first Node daemon + Expo/Electron/CLI + E2E relay (NaCl box).
- `AgentManager` FSM `initializing→idle⇄running→error→closed`; `closed`=persisted resumable; `ensureAgentLoaded()` same-ID resume.
- Parent/child via `paseo.parent-agent-id` labels; subagent vs detached; cascade-archive `agent-lifecycle.md:96-104`; `autoArchive` after terminal turn.
- WS hello/caps + `session.supports()` gating; JSON + binary terminal frames `shared/binary-frames/terminal.ts` (Output 0x01/Input 0x02/Resize 0x03/Snapshot 0x04); 8MiB HWM, 4MiB soft backpressure.
- File JSON `$PASEO_HOME/agents/{cwd}/{id}.json` atomic temp+rename, Zod, no migrations. Opaque `wks_<hex>` workspaces; `cwd` execution vs `worktreeRoot` backing; `WorkspaceProvisioningService` durable placement authority.
- Tool catalog `server/agent/tools/` transport-neutral + thin MCP adapter `mcp-server.ts:31-52`; per-provider `paseoTools{enabled,disabledTools[]}` (`protocol/src/provider-config.ts:31-34`, `server/agent/paseo-tool-policy.ts:14-29`).
- Flaw: no DAG, JSON-no-migration, weak token ledger, allowlist not least-privilege.

### agent-orchestrator — `agent-orchestrator/docs/architecture.md`, `backend/internal/`
- Go daemon + Electron/React. Observe→Update→Derive.
- `SessionMgr` command engine (`session_manager/manager.go:376-393`), `LifecycleMgr` fact reducer (`lifecycle/manager.go:220-268`), SCM Observer 30s, Reaper 5s, CDC poller 100ms batch 512 (`cdc/poller.go`, `cdc/event.go:22-47`, `cdc/retention.go:10-27` 7d/100k).
- Durable facts only: `activity_state/is_terminated/mode+handles+generation/transitions/PR facts`; display derived at read (`service/Session` precedence).
- Ports/adapters `backend/internal/ports/`; 23+ agent adapters as leaves; CLI thin HTTP; `Workspace` interface `ports/outbound.go:322-351` with `Destroy` refuses dirty, `ForceDestroy` only after `StashUncommitted→refs/ao/preserved/<sess>` (temp GIT_INDEX_FILE, never shared stash).
- TUI↔Chat controller replacement same session, CAS `session_mode`, generation fence, durable outbox.
- Flaw: tmux/conpty + detached ACP weight, 30s poll latency, no token ledger billing-grade without `usage.sql` pipeline.

### agent-swarm — `agent-swarm/CLAUDE.md`, `src/http/poll.ts`, `src/heartbeat/`, `src/tools/`
- Bun+TS, `bun:sqlite` WAL. API server sole DB owner (`check-db-boundary.sh`); workers HTTP + `X-Agent-ID`; key via `getApiKey()` only.
- Lead/worker pool + poll-claim atomic `UPDATE WHERE unassigned` in txn + `afterCommit` telemetry (`poll.ts:287-590`); `canClaim` budget gate + `budget_refused` trigger.
- Workflows DAG `definition.ts`: single entry, `next` string|fan-out|port-map, `inputs` must be upstream. Convergence on taken `activeEdges` only.
- `BEGIN IMMEDIATE` + `afterCommit` (never microtask); forward-only `src/be/migrations/NNN_*.sql`.
- Tools return `SwarmToolResult`, registrar builds wire; overflow 10KB → 24h `mcp:overflow:<agentId>` KV same pointer both channels (`kv-overflow.ts`, `tools/utils.ts:349-692`); `kv-get` exempt; script SDK 64MiB throw.
- Flaw: Bun-locked, overflow not scoping.

### opencode-swarm — `opencode-swarm/README.md:19,118-129`, `AGENTS.md`, `src/worktree/`, `src/tools/`, `src/plan/ledger.ts`
- Bun/Node OpenCode plugin; architect-led gated team (architect/explorer/coder/reviewer/test_engineer/critic…); Lean Turbo file-disjoint parallel lanes else serial.
- `.swarm/plan-ledger.jsonl` authoritative (`ledger.ts:47-101` seq/hash-chain/source), `plan.json/md` projections; two hashes: ledger-inclusive vs structure-exclusive.
- `TOOL_METADATA.agents[]` → derived `AGENT_TOOL_MAP` + opt-in overlays `MEMORY/SKILL/COUNCIL/TURBO/EXTERNAL` (`config/constants.ts:249-390`, `agents/index.ts:1378-1468`); PR fail-closed `observe|validate|null`.
- `scope-guard.ts:77-274` coder-only, per-session root, exact v2 binding, `SCOPE_NOT_DECLARED/ROOT_ESCAPE/VIOLATION`; `shell-write-detect` 8 cats; `WRITE_TOOL_NAMES` single list; patch caps 1M/2M.
- Subprocess: array-spawn, explicit `cwd`, `stdin:ignore`, `timeout`, bounded IO, `proc.kill()` in `finally`; init fail-open + post-resolution queue (`repro-704`).
- Worktree base `<project>/.swarm-worktrees`, `provisionWorktree` verify `rev-parse --git-dir`, ownership-gated reclaim, `merge.ts:988-1139` merge/rebase/cherry-pick/squash-unstaged + OID fences.
- Context-budget prune-outgoing-only (0.7 warn/0.9 enforce); PRM trajectory hard-stop.
- Flaw: single OpenCode host, Windows `bun:` loader fragility.

### Orkas — `Orkas/README.md:167-197`, `src/main/features/group_chat/bus.ts`, `actor-budgets.ts`, `core-agent/src/agent/context-budget.ts`
- MIT Electron; Commander + 9 specialists, BYOK.
- Single FIFO runtime/conversation; nested in-process dispatch; `globalSlots=10/dispatchSlots=4`, nested skips global (deadlock by construction); 3 verbs `hand_off_to/dispatch_to/run_worker`.
- Derived budget `usableInput=window-maxOut-safety`; shares 0.6 active/0.4 history; workspace ledger survives compaction; spin detector.
- Hybrid FTS5+vector RRF 0.7/0.3 on-device ONNX; signal-weighted metacognition; 4 runaway guards (turns=100, tool rounds 120/100, loop, watchdog).
- Flaw: no worktree engine, Electron-heavy.

### orca — `orca/AGENTS.md`, `src/shared/worktree/`, `src/shared/child-process/`, `src/main/git/worktree-removal.ts`
- Electron+TS fleet, any CLI agent, desktop+mobile.
- Execution host owns agent-status single store; readers subscribe. Verdicts `live/unverifiable/exited`; loss of contact ≠ death. Rules need captured PTY transcripts.
- Remote wire compat: optional fields only, opcode negotiation. Git 2.25 baseline + `GitCapabilityCache`.
- `runProcess/spawnProcess` single chokepoint (30s, 8MiB, `truncated`, `windowsHide,shell:false`); bundled `rg` via `spawnBundledRipgrep`; WSL `--exec` + banner fence.
- Worktree classification `orca-managed/external/unknown-legacy/agent-scratch` (`ownership.ts:111-159`); removal fast-path rename-to-trash + `prune` + async unlink; locked→`unlock` hint never folded to dirty.
- Flaw: terminal-scrape brittleness without transcripts.

### agent-of-empires — `agent-of-empires/AGENTS.md`, `src/process/`, `src/events/`, `src/git/worktree/`
- Rust TUI+daemon + web; tmux persistent sessions (`Ctrl+b d`); `aoe-plugin-api` manifest; ACP-worker.
- Every worktree locked `aoe-managed`; `remove_worktree(path,force)` unlock→`remove [--force]`; `compute_path ../{repo}-worktrees/{branch}`; `.git gitdir:` rewritten for containers; mutations 30s bound, observation 5s.
- Trash `git worktree move → .aoe-trash/<sess>` previewable; external move repaired from `worktree list`.
- Flaw: POSIX-only, cargo feature-matrix disk cost.

### iPolloWork — `iPolloWork/AGENTS.md`, `apps/app`, `apps/orchestrator`
- pnpm Electron + OpenCode sidecar (not fork) + DSH peer; Codex via `ipollowork-ui-mcp`.
- Engine Protocol normalizes task/streaming; one global extension lifecycle (plugins/skills/agents/commands/services/auth).
- `fraimz` frame-proof `evals/results/<id>/fraimz.html`; `/voiceover` demo-before-code; `maintainable-code` gate.
- Flaw: source-available license, sidecar upgrade coupling.

### munder-difflin — `munder-difflin/README.md:130-159`, `HIVE.md`, `SPEC.md`, `src/main/git.ts`
- Electron/React/Pixi/xterm/node-pty; GOD (Michael) + router + single-committer hive (memory+mailbox+blackboard+log; agents write `outbox/`, router → `inbox/`).
- `addWorktree(cwd,wt,base)` `agent/<slug>` branch, fallback plain add; `remove --force` always; `runGit` 8s SIGKILL; `worktreeHasUnintegratedWork` dirty/ahead → preserve; `gc` only if clean+ahead==0; `linkWorktreeDeps` symlink node_modules (junction win) with realpath guard.
- Avatar FSM + `Stop` hook `{"decision":"block"}` loop; per-agent autonomy + steer→constrain→stop breaker; token ledger from transcripts.
- Flaw: Pixi theatrics overhead, mailbox poll latency.

### oh-my-pi — `oh-my-pi/AGENTS.md`, `packages/catalog/src/compat/rules/`, `packages/agent/`, `packages/coding-agent/`
- Bun+TS+Rust `pi-natives`; `coding-agent` CLI primary.
- Model policy in KDL `compat/rules/**/*.kdl` → `rules.json` (never `id.includes` in TS); values via `@oh-my-pi/pi-catalog`.
- Workers re-enter `cli.ts` via `__omp_worker_*`; Bun APIs (`Bun.file/serve/SQLite`); TUI sanitization `replaceTabs/truncate/shortenPath`; prompts in `.md` + Handlebars.
- Tool scopes `workspace|sandbox|coordination|device`; approval `allow`; ACP gate.
- Flaw: single-agent, no fleet/DAG.

### claude-smart — `claude-smart/ARCHITECTURE.md`
- Python hooks (6: SessionStart/UserPromptSubmit/PreToolUse/PostToolUse/Stop/SessionEnd) + reflexio SQLite `localhost:8071`.
- JSONL buffer `~/.claude-smart/sessions/{id}.jsonl`; per-session injection dedup (in-mem seen-state, backfill next-best); `user_id=basename(git-toplevel)`.
- Host LLM via `claude -p` subprocess, no keys. Correction-SOP vs success-recipe extraction.
- Flaw: no orchestration, compact loses dedup.

### awesome-agent-orchestrators
- Taxonomy only: Parallel TUI/CLI vs Desktop/Web vs Swarms vs Loop/Task Runners vs Infra vs Assistants. Use for build-vs-buy: tmux+worktree, mailbox, merge-queue patterns. Resting list signals churn.

## 2. Cherry-Picks (adapt directly)
1. AO CDC `change_log`→poller→SSE + 7d/100k janitor.
2. AO durable-facts/derived-status + never-trust-failed-probe.
3. Paseo WS hello/caps + `session.supports()` + binary terminal frames.
4. Paseo opaque `wks_<hex>` + `cwd` vs `worktreeRoot` + provisioning service.
5. Paseo cascade-archive + autoArchive + open-tab labels.
6. Swarm `TOOL_METADATA`→derived map + opt-in overlays + `doctor tools`.
7. Swarm `plan-ledger.jsonl` + context-budget prune-outgoing-only.
8. Swarm scope files + shell-write AST + `withTimeout/proc.kill/cwd/stdin:ignore`.
9. Agent-swarm API-owns-DB + `BEGIN IMMEDIATE` + `afterCommit` + `getApiKey()`.
10. Orkas 3-verb Commander + slots anti-deadlock + derived budget.
11. Orca single status store + transcript-evidenced rules + `live/unverifiable/exited`.
12. AoE tmux persistence + container sandbox; oh-my-pi KDL policy + worker re-entry; claude-smart dedup.
