# ANNEX harvest-review-01 → 06-rolepack-moat-ms2 (execution brief, carries MS-2 — FINAL PHASE)

**Run:** `harvest-review-01` · **Gate-1:** approved (all 35 + 9 vectors + A1) · **Phase:** 06-rolepack-moat-ms2 · **Predecessors:** 02 SIGNED OFF (2ff256b), 03 SIGNED OFF (6b2cb8d), 04 SIGNED OFF + MS-1 PASSED (83ce455), 05 SIGNED OFF (e553c61)
**Parent GOAL:** `file://.roadmap/06-rolepack-moat-ms2/GOAL.md` · **Parent dossier:** `file://.roadmap/06-rolepack-moat-ms2/dossier.json` (verdict: pending)
**Gate dossier:** `file://.roadmap/harvest-review-01/dossier.json` sha256 `7cac8a00e6cdb0dba39e230e3f47d1bb91558eebc835f5d4cbe3f0ba4d100e5e`

## Phase hashes cited
- frontier: `86b6c81b505caccdffed71d0ca5139047cf4bd693e838791ab306025a52a9af7` (NOTE P02–P05 lessons: file://.research/frontier.json ABSENT in-tree; live frontier `.factory/frontier.json` `5bf2863f…` — cite dossier claim + footnote, never assert sha256sum match against the absent path)
- v1 roles catalog (port candidate): `a414bc61fb12a41c11fe188c806efdaec0719be2de13fb5ec2ecd1464d8731ab` (FILE_HASH_ONLY — reproducible revision form, no caret syntax; the v2 tree now has its own evolved roles/mod.rs — port-or-reference the ~30-role catalog explicitly against this hash)
- beta dossier: `082c5c6e1bc543996d2372206e7ca8123ba02e2f60e5108f53fbbf6fa206dc1` (HYPOTHESIS)

## Harvest evidence cited
- darkharvest dossier.json: `17f5ceaaa0a46b78060d402abc2a3087a7d08cf3460604ca6969655682e076e6`
- brainstorm REPORT.md: `9ed6ddfab988dfe7d69b0a2eaec7fbb1f854060b6bd48004b963c73f88691773`

## P06 adoptions (6 — the moat content)
| ID | Verdict | License | Item | Citations (sha256) |
|---|---|---|---|---|
| f06-omp-agentdef | clean-room (format compat) | MIT | omp AgentDefinition contract + frontmatter serializer + discovery precedence/validation — the exact compile target (TOP-1 harvest item) | c050 `bab92622d0…`, c051 `edcf4603de…`, c052 `524de1af7e…`, c053 `b2481452d0…` |
| f06-omp-spawns | clean-room | MIT | spawns policy resolution (disabled/explicit-list/unrestricted + default agent + typed rejection) — pairs with P02 request_spawn | c054 `8f1a9340aa…` |
| f06-opencode-agents | clean-room (format compat) | MIT | .opencode agent markdown format (mode/description/color frontmatter + prompt body) + workspace opencode.json | c055 `5262b2b02f…`, c056 `abf7873bc6…` |
| f06-meta-bus | clean-room | MIT | Tagged message bus schema (cause_by/sent_from/send_to + MessageQueue) for multi-pack teams | c057 `f21ec5023b…` |
| f06-handoff-pipeline | clean-room | MIT | Handoff generation pipeline (session-state → handoff docs) as inter-role evidence | c058 `df9a56481d…` |
| f06-orca-versioned-skills | clean-room | MIT | Version-matched skills served by the binary (stub SKILL.md → versioned reference) | c059 `cd1b364bf3…`, c060 `51bc9c91c5…` |

## Vectors (the kill-gate instruments — run FIRST, they gate scope)
- V9 Behavioral Fidelity Bench (FIRST): mechanical behavioral bench BEFORE the multi-emitter — 20 tasks (4 classes × 5), deterministic-artifact grading (never LLM-judge), calibrate with a deliberately broken adapter (good ≥90% vs broken <60%, separation ≥30pts). Decides the kill-gate early: >15% fidelity gap → CUT multi-harness, do not extend. Spend-capped, cheapest adequate model.
- V6 Discovery-Precedence Round-Trip Oracle: ported oracle over harvested corpus definitions + every STC emission; precedence-collision test (project pack vs user pack); diagnostic bar (file:line + hint).
- V5 Spawn Matrix extended: P02's 270-cell generator reused across omp/pi/opencode emissions (declared allows never false-deny per target; escalations typed-reject per target).
- V4 Substrate Twin (bounded): twin binary + recorded transcript for the operator E2E; differential run against real CLI at close.
- V1 Replay: operator-run trace fixtures for the deterministic inner loop.

## MS-2 operator run
Full flow (scope → DAG → dispatch → review → merge) visible AND controllable in the browser with live proof + screenshots; four pillars visible in one session; non-escalation holds across targets (V5 per-target matrix is the evidence).

## Guardrails (binding)
- Kill-gate is BINDING: fidelity gap >15% → cut to one substrate, do not extend. V9 runs first so the cut (if any) happens before emitter work.
- Format-compat verdicts (f06-omp-agentdef, f06-opencode-agents): field names are contract — no renaming; pin the upstream version compiled against; emit-verify roundtrip tests.
- P02–P05 lessons: receipt header cites §-corrections; counts exact; prek guarded-dossier writes reverted + noted.
- Evidence: every claim carries hash. Scope firewall holds. No marketplace (signed internal catalog only, per DEFERRED log).
- DoD: browser verification at MS-2 (this phase carries the milestone).

## Acceptance (unchanged from parent GOAL.md)
- [ ] one pack source compiles to all three targets
- [ ] ported catalog passes tests+clippy citing v1 sha256
- [ ] MS-2 full operator run visible/controllable in browser with live proof + screenshots
- [ ] four pillars visible in one browser session
- [ ] non-escalation holds across targets
- [ ] kill-gate: >15% fidelity gap cuts multi-harness
