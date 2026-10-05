# Phase 01 — Cockpit WebUI beta polish (verified live)

Arena: Cockpit WebUI beta (#17) — 4 views + Providers.
Frontier: `.factory/frontier.json` (settled, user-approved 2026-09-27).

## Baseline (verify first)
- `npm run build` in `cockpit/` green (`cockpit/src/App.tsx`, `cockpit/src/tokens.css`, `cockpit/src/pages/Providers.tsx`)
- `cargo check` / `cargo test` green (projection `Snapshot` shape in `studio-core/src/projection`)
- Current state: `App.tsx` has 2 tabs (war-room grid of 4 sections + Providers); oklch tokens + tabular-nums already in `tokens.css`

## Goal
Impeccable-skill design pass over War Room, Agent Stream, Audit+Gatekeeper, MCP Registry, Providers:
hierarchy, density, empty/loading/error states, responsive, oklch + tabular-nums consistency,
keyboard + contrast baseline — all verified LIVE via chrome-devtools MCP against real projection data.

Real-swarm orchestration (BLOCKED at dossier stage, backend `claude -p` missing) still produced
high-value falsifiable scopes — use as spike hypotheses:
1. Failure-legible projection states (empty/loading/stale/error/filtered-out/unauthorized + scoped retry)
2. Keyboard-first triage path (War Room anomaly → Stream cause → Audit decision → Registry health)
3. oklch + tabular-nums integrity under live ticks (jitter/CLS/AA, breakpoints + 200% zoom)
4. Schema-driven density/hierarchy per view vs uniform-grid control (anti-slop detector)

## Acceptance (from #17, must all be checked with evidence)
- [ ] All views reviewed via chrome-devtools with real data; issue list closed (screenshots per view)
- [ ] Empty/loading/error states designed, not default (per-stream scoped retry, no full-page reload losing filters)
- [ ] No placeholder/mock data anywhere in beta
- [ ] Frontend build + Tauri check green
- [ ] Keyboard nav + contrast AA baseline measured on live nodes (light + dark oklch)
- [ ] Providers view + dev-mode badge resolved (in-scope per frontier)

## Hard rules
- UI never owns state (projection only, staleness badges everywhere)
- No mock data shipped as finished
- Verify claims against running UI, not code reading
- Fail closed: missing backend/evidence → `blocked` receipt, never downgrade

## Dossier
See `dossier.json` (goal/evidence/acceptance/brief/verdict/hashes).
Status: DRY-RUN — mock swarm only. Real-swarm VERIFIED hashes for Cockpit claims are MISSING.
Programmer + qa-a/qa-b spawn BLOCKED until real swarm fixed or user grants dry-run waiver.
