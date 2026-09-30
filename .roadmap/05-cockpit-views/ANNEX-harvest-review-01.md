# ANNEX harvest-review-01 → 05-cockpit-views (execution brief)

**Run:** `harvest-review-01` · **Gate-1:** approved (all 35 + 9 vectors + A1) · **Phase:** 05-cockpit-views · **Predecessors:** 02 SIGNED OFF (2ff256b), 03 SIGNED OFF (6b2cb8d), 04 SIGNED OFF + MS-1 PASSED (83ce455)
**Parent GOAL:** `file://.roadmap/05-cockpit-views/GOAL.md` · **Parent dossier:** `file://.roadmap/05-cockpit-views/dossier.json` (verdict: pending)
**Gate dossier:** `file://.roadmap/harvest-review-01/dossier.json` sha256 `7cac8a00e6cdb0dba39e230e3f47d1bb91558eebc835f5d4cbe3f0ba4d100e5e`

## Phase hashes cited
- frontier: `86b6c81b505caccdffed71d0ca5139047cf4bd693e838791ab306025a52a9af7` (NOTE P02–P04 lessons: file://.research/frontier.json ABSENT in-tree; live frontier `.factory/frontier.json` `5bf2863f…` — cite dossier claim + footnote, never assert sha256sum match against the absent path)
- v1 projection: `3600ef7d48ac6c1c325b49d3f976c1995b4fa75b62fdcf48aef5465c7c633c4f` (FILE_HASH_ONLY — reproducible revision form, no caret syntax)
- beta dossier: `082c5c6e1bc543996d2372206e7ca8123ba02e2f60e5108f53fbbf6fa206dc1` (HYPOTHESIS)
- P04 foundation (live): studio-web SSE + staleness taxonomy + ms1_gate.sh (commit 83ce455); StatusView + AskView(read-only) patterns to extend

## Harvest evidence cited
- darkharvest dossier.json: `17f5ceaaa0a46b78060d402abc2a3087a7d08cf3460604ca6969655682e076e6`
- brainstorm REPORT.md: `9ed6ddfab988dfe7d69b0a2eaec7fbb1f854060b6bd48004b963c73f88691773`

## P05 adoptions (8)
| ID | Verdict | License | Item | Citations (sha256) |
|---|---|---|---|---|
| f05-px-events | clean-room | MIT | Normalized AgentEvent union + HookProvider interface with protocolVersion (refuse unknown provider versions) — Agent Stream / War Room backbone | c035 `e18cca3ada…` |
| f05-paseo-buckets | clean-room | Apache-2.0 | Attention-first state buckets (needs_input>failed>running>attention>done) + permission notification kinds — deterministic cockpit ordering + Ask taxonomy | c036 `524849c42c…`, c037 `f85b298112…`, c038 `724298a5c7…` |
| f05-herdr-detect | clean-room | Apache-2.0 (vendored libghostty-vt MIT) | Versioned per-agent detection manifests (bundled/remote/override) + explain output + working/blocked/idle flags — PATTERN ONLY (ghostty-vt explicitly rejected as depend: Zig toolchain) | c039 `a1bfa4441f…`, c040 `54e89030f1…`, c041 `038d0aa23f…`, c042 `c74713ae3f…` |
| f05-vk-kanban-diff | clean-room | Apache-2.0 | Kanban board + in-app diff/review workflow (task states, execution processes, head-SHA-fenced feedback) — Audit+Gatekeeper review surface | c043 `e71748fe5c…`, c044 `b66ef1b465…` |
| f05-openfang-providers | clean-room | MIT OR Apache-2.0 | Providers view plumbing: health-probe cache with TTL + budgets endpoint on axum | c045 `37e11a2b9e…` |
| f05-oh-acp-authprobe | clean-room | MIT | Per-harness ACP auth-status probes for onboarding/Providers (loggedIn JSON, stderr text, credential-file presence) with unknown fallback | c046 `26b3f74dcf…` |
| f05-agentdeck-terminal | clean-room | MIT | Embedded VT terminal + web projection of live sessions — PATTERN ONLY (Go/charmbracelet source; no language migration; terminal visibility only if MS-2 needs raw PTY) | c047 `76bd357e57…`, c048 `7f9ddf5be6…` |
| f05-openchamber-multirun | clean-room | MIT | Multi-run compare (up to 5 models, per-run worktrees) + guided changes walkthrough | c049 `e048681a17…` |

## A1 — decide wiring (THIS phase completes the variant)
Gate-1 approved the in-scope reshape: cockpit Ask-policy surface + desktop notification with the one-tap decision contract. P04 delivered read-only Ask queue (decision buttons disabled, never fake-enabled). THIS phase wires the one-tap decision through the P02 approval service (durable store + scoped leases c008, request/status/outcome protocol c001-c003): approve/deny buttons enabled ONLY where the service exposes a live decision path for that request; every decision lands in the ledger with the same one-tap contract; anything not service-backed stays visibly disabled. Desktop notification: attention payload patterns only (no mobile code, no push infra).

## Vectors
- V7 codegen revisit (deferred spike from P04): with FIVE views sharing shape, re-evaluate the catalog generator kill criterion (generated LOC vs hand-written for view #2). If kill still holds, hand-wire + say so; if the shape repeats enough to flip, generate with the red-on-stale contract test.
- V8 gate reuse: extend ms1_gate.sh pattern per view (seeded fixtures + frozen clock + screenshots; fresh/stale/lost assertions where views consume streams).

## Guardrails (binding)
- Every view is a read projection with staleness badge (fresh/stale/lost taxonomy); UI never owns state. Zero mock data (V8-style DB-only assertion per view).
- Designed empty/loading/error states with per-view scoped retry (not defaults) — V7 fixtures cover these.
- Tokens + tabular-nums consistency; keyboard nav + AA contrast baseline measured (report measurements, not adjectives).
- Each view verified live via chrome-devtools with screenshot.
- P02–P04 lessons: receipt header cites §-corrections; counts exact; prek guarded-dossier writes reverted + noted.
- Evidence: every claim carries hash. Scope firewall holds.
- DoD: command-first; browser verification only at MS-1 (done) and MS-2 (phase 06).

## Acceptance (unchanged from parent GOAL.md)
- [ ] all five views verified live with real data + screenshot each
- [ ] empty/loading/stale/error states designed with per-view scoped retry
- [ ] zero mock data
- [ ] tokens + tabular-nums consistency
- [ ] keyboard nav + AA contrast baseline measured
- [ ] builds green
