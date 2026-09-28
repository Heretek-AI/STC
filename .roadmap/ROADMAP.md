# STC v2 — Program Roadmap (greenfield rebuild)

**Run:** `stc-greenfield` · **Branch:** `v2-greenfield` (off `main`, this worktree) · **Autonomy:** full authority
**User:** homelab solo builder · **Scope:** four pillars + RolePack moat · **Out of scope:** marketplace, cloud hosting, mobile, voice, harness OAuth reimplementation (GOAL.md 6)

## Regime
- **Port rule (frontier `meaning_from_scratch`):** a v1 module ports ONLY if its tests pass and it is clippy+rustfmt clean **in the v2 tree**.
- **Carry-over (frontier `carryover`, AMENDED 2026-09-27):** port `studio-core` engine + `adapters/acp` + `adapters/cli`; **`tui` dropped** (superseded by browser-first cockpit); **rewrite** `cockpit/`.
- **Stack:** Rust/Tokio core + `axum`/tower-served browser-first SPA (no Tauri in v1). Crate names kept; add `studio-web`.
- **State:** greenfield v2 SQLite schema, **no v1 migration**; v1 db archived.
- **Gate order:** format -> fallow (new-findings-only) -> Semgrep -> tests + tree-sitter -> RDD freeze -> one bounded correction -> burned ack -> merge.
- **Live-fire:** DEFERRED to a later gated phase (no key present; never hunted or guessed).

## Phases
| # | Phase | DoD | Carries |
|---|-------|-----|---------|
| 01 | engine-skeleton | command | — |
| 02 | contract-runtime | command | — |
| 03 | tool-gateway | command | — |
| 04 | cockpit-shell-ms1 | command + **MS-1 browser** | MS-1 |
| 05 | cockpit-views | browser (per view) | — |
| 06 | rolepack-moat-ms2 | command + **MS-2 browser** | MS-2 |

## Milestones
- **MS-1 (in phase 04):** live browser session renders REAL projection data with staleness badge, zero mock data, browser read-only — screenshots required.
- **MS-2 (in phase 06):** full operator run (scope -> DAG -> dispatch -> review -> merge) visible/controllable in the browser with live proof.

## Kill criteria
- **MS-1 misses** (cannot render real projection in a browser without mock) -> freeze the roadmap, regrill scope; do NOT slip to P5.
- **Compiled RolePack fidelity trails native by >15%** -> cut multi-harness, pick one substrate (per Brainstorm III FV1 probe).
- Two consecutive QA failures on the same phase -> escalate to manager; three -> halt the phase and return with both QA reports.

## Swarm evidence status (gate cycle 1 of 5)
Both swarms FAILED identically (`alpha_dossier.json` missing) — brainstorm and darkharvest alike. Beta produced one
real dossier (P1 scope); its 7 cited sources were archived by the manager and **6 quotes re-verified** via
`iumbtems_verify_quote` (confidence 0.98). Sourced VERIFIED evidence is limited to those 6 plus file-hash pointers.
No backend debugging performed (loop rules).
