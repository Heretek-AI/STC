# Studio — 04 ADRs & Roadmap (Phase 4)

## ADR-1 Runtime language

**Decision: Go daemon + TS adapters + Bun tooling (AO precedent).**

- Go: static binary, `tmux/conpty` control, SQLite + CDC, loopback HTTP/WS, cross-platform PTY. Matches `agent-orchestrator/backend` (`ports/service/lifecycle/storage/cdc/httpd/adapters`) and load-bearing rules (never store display status, never trust failed probe, never force dirty, `~/.ao` only, loopback only, CLI thin, CDC truth, adapters leaves, hooks gitignored, migrations immutable).
- Rust alternative (AoE `src/process/,src/events/`) if TUI-first: stronger supervision + event log, but POSIX-only + cargo feature-matrix disk cost. Keep as TUI/sandbox reference, not daemon.
- TS/Bun only for plugin/adapter layer: portability risk under Node sidecar (opencode-swarm `sqlite-loader.ts`, no top-level `bun:`, `dist/` uncommitted). oh-my-pi KDL + Bun APIs for CLI perf, not fleet core.
- Python only for reflexio-style learning sidecar (claude-smart), never hot path.

## ADR-2 IPC / transport

- Loopback HTTP + WS (JSON + binary terminal frames Paseo `shared/binary-frames/terminal.ts`) + SQLite CDC → SSE fanout. Unix socket `browser.sock` for browser bridge (daemon authorizes, Electron owns targets; no remote-debug port). Relay E2E NaCl `box` for remote (zero-knowledge, QR pairing, `relay disabled until consent`).
- Second LAN listener only opt-in `0.0.0.0` behind bearer, never control routes (`/shutdown`, telemetry); single exempt `GET /api/v1/identity` (opaque host + contract ver). Plaintext home-network only by decision (AO `adr/0001`, Paseo `SECURITY.md`).
- No gRPC (overkill), no raw TCP. Schemas append-only (`protocol` package source of truth, zod-aot inbound validation, `COMPAT(name)` shims, dotted `domain.op.response` RPCs).

## ADR-3 Persistence & identity

- SQLite WAL `BEGIN IMMEDIATE` + `afterCommit`; `change_log` triggers; file JSON only for agent records with atomic rename + Zod (Paseo) where SQLite overkill. Ledger append-only JSONL authoritative, projections derived. Opaque IDs (`wks_<hex>`, `prj_<hex>`, `wt2:host:inst`); `workspaceId` ownership never inferred from `cwd`.

## Phased build

### P0 MVP CLI (Worktree + Manager-Worker)
- Go daemon: `SessionMgr/LifecycleMgr` minimal, `Workspace{Create/Destroy/ForceDestroy/Stash/Apply}` (AO), `createWorktree/deletePaseoWorktree` semantics (Paseo), trash + prune + retry rm.
- Manager-Worker loop: spawn row→worktree→runtime→MarkSpawned; `AgentClient` trait (Paseo `agent-sdk-types.ts:197-237,646-712`) for Claude/Codex/OpenCode/pi; thin `ao`-style CLI over HTTP; CDC SSE.
- Ledger `.swarm/plan-ledger.jsonl` + `plan.json` projection; token meter (provider usage else 0.33/char).
- Exit: parallel coders in isolated worktrees, no collide, `paseo pid` supervisor guard.

### P1 Tool gating & MCP Router
- `TOOL_METADATA→AGENT_TOOL_MAP` + opt-in overlays + `paseoTools` per-provider + `tool_filter.overrides` with authoritative strip.
- `scope-guard` + `resolveWriteTargets` + `shell-write-detect` + `runProcess` chokepoint; `check-tool-registration` CI.
- Overflow KV 10KB→24h + summarizer + `retrieve_*` + per-session dedup.
- Exit: Coder/Researcher/Reviewer see only role schemas; overflow pointer both channels.

### P2 Auditor gate
- Reviewer/test_engineer + `syntax_check` (tree-sitter 20 grammars) + `placeholder_scan` + `sast` 68 rules + `sbom` + `quality_budget`; scope-aware destructive block; `SCOPE_VIOLATION` → architect advisory loop.
- Merge queue Bors-style + file-disjoint fast path; conflict → typed files → coder loop; `pr_feedback` activation only when no owner else queued.
- PRM + circuit breaker + backstop 25M; `Archive finished` + cascade-archive.
- Exit: nothing ships without reviewer+tests; evidence per task `.swarm/evidence/`.

### P3 Cockpit
- TUI first (AoE `structured_view/render.rs`, oh-my-pi TUI sanitization): War Room board + Agent Stream + Diff + MCP matrix per 03.
- Then Electron desktop + mobile companion (Paseo/Orca): coalescers (terminal 5ms, agent 60ms), frame-commit, paced reveal, virtualized diff, `doctor tools` matrix.
- Perf bars: echo p50 2.3ms/p95 3.3ms; keydown→commit p50 18ms; agent gap p50 317→17ms, CV 4.11→1.86 (Paseo benchmarks).

## Risks
- Terminal-scrape brittleness → require PTY transcripts for readiness rules (Orca).
- Windows PTY/cmd/shim + WSL UNC → `runProcess` wrapper + `GitCapabilityCache` + Git 2.25 baseline.
- OpenCode cache pinning → `update` clears all layouts + `diagnose` version check.
- JSON-no-migration drift → SQLite for DAG/billing, JSON only for agent records with optional-field compat.
