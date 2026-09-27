# STC Next Phase v2 — Platformization Plan (Phase 6)

Date: 2026-09-26. Status: research complete, approved direction, ready to build.
Supersedes: `../opencode/stc-next-phase-plan.md` (v1, first brainstorm) on gateway
sidecar, Engram-as-store, lefthook default, and compose-as-product framing.
Companion audit: `../stc-next-phase-master-roadmap.md` (Phase 6 WS1–WS8, authoritative
for workstream internals). Sibling reports: `../claude/`, `../antigravity/`.

Carried constraints: local-first; four pillars unchanged; no marketplace / cloud
hosting / voice / mobile; no OAuth reimplementation; no shipped gateway sidecar;
no LSP-as-gate; workers never see raw credentials; UI never owns state.

---

## 1. Locked decisions (this session)

| ID | Decision |
|---|---|
| D-RUNTIME | Native control plane (host binary, in/alongside Tauri) + rootless **Docker** lanes. Socket-sharing dev-mode only via `.env` (`STUDIO_LANE_RUNTIME`) + `compose.override.dev.yaml` + always-on banner/badge. Quadlets rejected with the Docker lock-in. |
| D-IDENTITY | Lane git identity via SSH-agent forwarding + broker-minted short-lived deploy keys; GPG-in-lane fallback only. Token auth via broker-backed `GIT_ASKPASS`/extraHeader shims, never mounted `~/.ssh`. |
| D-HARNESS | Pi-first specialist lanes + OpenCode heavy coder, routed at runtime by measured $/task (Composio head-to-head: pi ~7x leaner fixed overhead, best pass rate, ~2x lower cost/success; OpenCode fewer turns). Claude/Codex CLIs = mounted legacy `cli_delegated` lanes. |
| D-PROFILES | Author once against omp `AgentDefinition`, emit 3 targets (omp-native, base-pi shim, `.opencode/` mirror). One flagship pack (`coder-web`); catalog expansion gated on H-C. |
| D-GATEWAY | Gateways (Bifrost/LiteLLM/OmniRoute) are passthrough provider profiles only. Minimal internal client for daemon-owned calls. Tiers preconfigured, user-overridable per role. |
| D-MEMORY | Ledger projection retained; write-path deny-list; ≤250-token anchored summaries; Engram eval spike only (kill-gated). |
| D-GATES | Format → fallow `audit` (TS/JS) → Semgrep Guardian → tests + tree-sitter → RDD freeze→burn → Bors. prek hooks. CLI diagnostics authoritative; LSP navigational. Suppressions as SARIF objects only. |

---

## 2. Trade-off matrix (decisive pairs)

| Dimension | Options | Verdict + one-line why |
|---|---|---|
| Memory store | Engram-as-L1/L3 vs ledger projection | **Projection** — second history with no shared identity injects stale facts |
| Memory retrieval | Vector-first vs anchored summaries | **Anchored summaries** (empirically +8pts vs −4 for unfiltered) |
| Runtime | Compose-as-product vs native plane + lanes | **Native plane + lanes** (Metal GPU, keychain OAuth, bind-mount perf, worktree path identity) |
| Lane identity | Mounted keys vs forwarded agent + deploy keys | **Forwarded agent + deploy keys** (lanes never hold long-lived secrets) |
| Harness | OpenCode-first vs pi-first | **Pi-first specialists, OpenCode heavy** (measured $/task, not brand) |
| Gateway | Sidecar vs passthrough | **Passthrough** (no team-product complexity without a team) |
| Hooks | lefthook vs prek | **prek** (Rust, workspace mode, no worktree bug class) |
| Gate diagnostics | LSP broker vs CLI→SARIF | **CLI diagnostics** (CI parity; OpenCode's own guidance) |
| Dead-code gate | knip/jscpd vs fallow | **Fallow** TS/JS (changed-files gate matches Bors philosophy); clippy + tree-sitter for Rust |

---

## 3. System architecture

```
CLIENTS (zero state; 100ms projection poll / E2E-encrypted relay, Happier pattern)
  Tauri cockpit · Ratatui TUI · (later: phone)
                        │ Tauri IPC invoke / HTTPS
NATIVE CONTROL PLANE (host; never containerized)
  studio-core daemon: DAG scheduler · claim broker · ledger · Bors queue
  Tool Lens gateway · Diagnostic Broker (CLI→SARIF→severity budget)
  CredentialStore (OS keyring) → SecretBroker → short-TTL lane tokens
  Gateways = passthrough provider profiles (Bifrost/LiteLLM/OmniRoute/OpenRouter)
        │ ProcessSpec spawn (host lanes)      │ rootless Docker (STUDIO_LANE_RUNTIME)
HOST LANES                                  CONTAINER LANES (default for new work)
  pi / opencode / claude / codex              pi specialists · opencode heavy · scanners
  per-lane config dirs; SSH-agent fwd sign    no sock/ssh/aws/gnupg/keychain inside
  OAuth CLIs as cli_delegated legacy lanes    broker deploy keys · 0600 token files
git worktrees on HOST (engine owns add/lock/prune; host-identical bind paths)
Memory: L0 ledger → L1 FTS5 projection (+Engram eval) → L2 md → L3 lifecycle → L4 gated
Gates: format → fallow audit → Guardian → tests/tree-sitter → RDD freeze→burn → Bors
  anti-circumvention at bridge (deny --no-verify, config writes, MCP write-through)
```

Per-task flow: scope → DAG commit → dispatch to cheapest-eligible lane (profile +
measured $/task) → manifest-sliced tools → evidence receipts → RDD freeze → one
bounded correction → burned ack → Bors lands → ledger + memory projection update.

---

## 4. Roadmap: MVP vs Production

### MVP (runs Monday)
1. **WS1 auth substrate**: `AuthKind`, keyring-backed `CredentialStore` (+0600 vault
   fallback), `SecretBroker::checkout` short-TTL token files, `auth_kind` on
   connections, `billed_kind` ledger rows. Test: `token_materialization_is_short_ttl`.
2. **WS6-no-auth subset in parallel**: fallow `audit --format json` as
   `diagnostics_budget_ok` DoD evidence; prek pre-commit (format+fast lints+gitleaks+
   config-integrity, <15s); format-on-write.
3. **Compose + persistence**: `compose.yaml`, `.env.example`, `compose.override.dev.yaml`,
   `studio up`/`backup`, per-class named volumes, `user_version` migrations with
   `VACUUM INTO` backup, host-identical worktree binds, rootless lanes.
4. **Real lane execution**: probe → versioned `~/.studio/harness/bin/` installs;
   ProcessSpec spawning for pi + opencode (+cli_shim profiles); mounted legacy lanes.
5. **Flagship `coder-web` RolePack**: omp-authored, lockfile-pinned, 3 emitted targets.

### Production (post-MVP proof)
6. Provider profiles + Providers view (live-ping proof, billed-account display,
   tier presets); auto-routing forbidden on `coder.primary`/`reviewer`; sticky-per-task.
7. RDD freeze→burn as merge requirement (WS8); action registry + exposure matrix.
8. Memory upgrades: deny-list, ledger-anchored validity windows, consolidation
   worker, Engram eval spike (kill: recall misses or >1 day ops).
9. E2E relay clients; GPU CDI smoke test for media lanes.
10. Profile expansion gated on H-C (>15% fidelity gap kills multi-emission).

### Kill-gated spikes (miss = cut scope, never slip MVP)
Engram recall/latency · H-A memory uplift (+5pts) · H-B review loops (−50% re-review) ·
H-C role fidelity (5%) · LM1 container restart + image swap (0 orphans, 0 index.lock).

---

## 5. Open items (need user input during build)
- GPU CDI smoke-test host selection.
- kasetto: pattern-integrate now vs time-boxed deeper eval.
- Installer signing cert ownership.
- domain_model.json facts block quarantine (stale Flawfinder/lefthook/counts) — decisions stand.
- Stats hygiene rule: re-fetch ecosystem counts at cite time, never copy forward.
