# ANNEX harvest-review-01 → 04-cockpit-shell-ms1 (execution brief, carries MS-1)

**Run:** `harvest-review-01` · **Gate-1:** approved (all 35 + 9 vectors + A1) · **Phase:** 04-cockpit-shell-ms1 · **Predecessors:** 02 SIGNED OFF (2ff256b), 03 SIGNED OFF (6b2cb8d)
**Parent GOAL:** `file://.roadmap/04-cockpit-shell-ms1/GOAL.md` · **Parent dossier:** `file://.roadmap/04-cockpit-shell-ms1/dossier.json` (verdict: pending)
**Gate dossier:** `file://.roadmap/harvest-review-01/dossier.json` sha256 `7cac8a00e6cdb0dba39e230e3f47d1bb91558eebc835f5d4cbe3f0ba4d100e5e`

## Phase hashes cited (from parent dossier.json)
- frontier: `86b6c81b505caccdffed71d0ca5139047cf4bd693e838791ab306025a52a9af7` (NOTE P02/P03 lessons: file://.research/frontier.json ABSENT in-tree; live frontier is `.factory/frontier.json` `5bf2863f…` — cite dossier claim + footnote, NEVER assert sha256sum match against the absent path)
- v1 projection (read candidate): `3600ef7d48ac6c1c325b49d3f976c1995b4fa75b62fdcf48aef5465c7c633c4f` (file://studio-core/src/projection/mod.rs, FILE_HASH_ONLY — reproducible revision form, no caret syntax)
- v1 state: `dbc6bf00233ebe76bf6ce397a36f7690adf22d04024f1d193d45402cd2db8503` (file://studio-core/src/state/mod.rs, FILE_HASH_ONLY — note: v2 tree has its own evolved state/mod.rs; port-or-reference explicitly)
- beta dossier: `082c5c6e1bc543996d2372206e7ca8123ba02e2f60e5108f53fbbf6fa206dc1` (alpha_completed=false → HYPOTHESIS)

## Harvest evidence cited
- darkharvest dossier.json: `17f5ceaaa0a46b78060d402abc2a3087a7d08cf3460604ca6969655682e076e6` (60/60 OK)
- brainstorm REPORT.md: `9ed6ddfab988dfe7d69b0a2eaec7fbb1f854060b6bd48004b963c73f88691773` (32/32 OK)
- crates.io live re-checks (gate-1): ts-rs stable 12.0.1 MIT (dossier pins 11.x for vibe-kanban 11.1.0 parity — decide at implementation with lockfile note)

## P04 adoptions (4)
| ID | Verdict | License | Item | Citations (sha256) |
|---|---|---|---|---|
| f04-vk-stream | depend+clean-room | Apache-2.0 | axum SSE history+live stream + WS snapshot/json-patch deltas with Ready frame; axum 0.8.4/tower-http 0.5 vetted set (phase already planned axum/tower — adopt exact feature flags as starting point) | c027 `53c0d41258…`, c028 `d43e64bf46…`, c029 `b4f61a7a63…` |
| f04-ts-rs | depend | Apache-2.0 (crate MIT) | ts-rs Rust→TS type export for cockpit API contracts (NOT the xazukx fork — crates.io ts-rs; generate in build/test step; generated files committed or CI-checked) | c030 `518ced2460…`, c031 `4dde733a1e…` |
| f04-herdr-api | clean-room | Apache-2.0 | Schema-first daemon API: event hub, subscriptions, wait-for-output semantics; history cursor + Lost/Unavailable gap semantics | c032 `9110fa6662…`, c033 `ccb0c00cde…` |
| f04-sandbox-daemon | clean-room | Apache-2.0 | Daemon ensure-running with PID/version files, restart-on-version-mismatch | c034 `b5abab3b9f…` |

## A1 — cockpit Ask-policy approval surface (04/05, approved gate-1 variant)
In-scope reshape of the DEFERRED mobile-relay attractor: cockpit Ask-policy surface + desktop notification with the one-tap decision contract (no mobile code). Evidence: paseo attention payloads (f05-paseo-buckets c055-c057 pattern) + P02 vk approval protocol (c001-c003) + P02 durable approval store (c008). Scope for THIS phase: read-only Ask queue surfacing over the approval store (decide-later wiring lands with P05 views); one-tap decision contract honored where the P02 approval service exposes it, otherwise visibly disabled (never fake-enabled).

## Vectors folded into P04
- V7 Projection Catalog Codegen (04,05): one catalog entry (status or tasks) → generated /api handler + TS type + 3 state fixtures (empty/loading/error) + contract test red-on-stale-DTO. Kill: generated LOC > hand-written, or surface can't express stale/empty/error.
- V8 Chronofault MS-1 Evidence Gate (04,05): scripts/ms1_gate.sh — seeded DB fixture ledger + frozen clock + CDC pause/burst + ring-overflow gap; 3 screenshots (fresh/stale/lost); badge text asserted; DB-only data source (zero mock). Pass: 10 runs ≤1 flake. Staleness taxonomy fresh/stale/lost (time-stale vs sequence-lost), not a single badge.

## Guardrails (binding)
- Server owns projection; browser reads only. Zero mock data at MS-1 (V8 asserts DB-only source).
- MS-1 live verification via chrome-devtools against real data + screenshots (parent brief).
- Missing daemon fails closed legibly (f04-sandbox-daemon ensure-running semantics inform the UX, not mask it).
- P02/P03 lessons: receipt header cites §-corrections; v1 citations reproducible form; counts exact; prek guarded-dossier writes reverted + noted.
- Evidence: every claim carries hash. Scope firewall holds (A1 is desktop/cockpit only).
- DoD: browser verification at MS-1 (this phase carries the milestone).

## Acceptance (unchanged from parent GOAL.md)
- [ ] studio-web serves SPA + /api real projection
- [ ] MS-1 live browser renders real data + staleness badge + zero mock, screenshots
- [ ] browser read-only
- [ ] missing daemon fails closed legibly
- [ ] frontend build + cargo test green
