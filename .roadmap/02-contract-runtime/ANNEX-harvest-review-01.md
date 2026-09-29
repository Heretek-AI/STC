# ANNEX harvest-review-01 → 02-contract-runtime (execution brief)

**Run:** `harvest-review-01` · **Gate-1:** approved (all 35 + 9 vectors + A1) · **Phase:** 02-contract-runtime
**Parent GOAL:** `file://.roadmap/02-contract-runtime/GOAL.md` · **Parent dossier:** `file://.roadmap/02-contract-runtime/dossier.json` (verdict: pending)
**Gate dossier:** `file://.roadmap/harvest-review-01/dossier.json` sha256 `7cac8a00e6cdb0dba39e230e3f47d1bb91558eebc835f5d4cbe3f0ba4d100e5e`

## Phase hashes cited (from parent dossier.json)
- frontier: `86b6c81b505caccdffed71d0ca5139047cf4bd693e838791ab306025a52a9af7` (file://.research/frontier.json, VERIFIED_HASH)
- v1 verify module (port/rewrite candidate): `c1e49e97f6429dec4604dbcf76e3e4a27fa577284c0e0a82254dddce1ac8c29f` (file://studio-core/src/verify/mod.rs, FILE_HASH_ONLY)
- v1 roles/RolePack + SpawnPolicy: `a414bc61fb12a41c11fe188c806efdaec0719be2de13fb5ec2ecd1464d8731ab` (file://studio-core/src/roles/mod.rs, FILE_HASH_ONLY)
- beta dossier: `082c5c6e1bc543996d2372206e7ca8123ba02e2f60e5108f53fbbf6fa206dc1` (alpha_completed=false → HYPOTHESIS)

## Harvest evidence cited
- darkharvest dossier.json: `17f5ceaaa0a46b78060d402abc2a3087a7d08cf3460604ca6969655682e076e6` (60/60 citations manager-recomputed OK)
- brainstorm REPORT.md: `9ed6ddfab988dfe7d69b0a2eaec7fbb1f854060b6bd48004b963c73f88691773` (32/32 manager-recomputed OK)
- crates.io live re-checks: acp-http-adapter 0.4.2 Apache-2.0; ts-rs stable 12.0.1 MIT (dossier pins 11.x for vibe-kanban parity)

## P02 adoptions (9) — implement with phase acceptance criteria
| ID | Verdict | License | Item | Citations (sha256) |
|---|---|---|---|---|
| f02-vk-approvals | vendor+clean-room | Apache-2.0 | Approval request/status/outcome protocol + in-process approval service | c001 `c9d6b38dc6…`, c002 `3c32da5997…`, c003 `09a8042c81…` |
| f02-yylo-receipts | clean-room | MIT | Versioned execution-envelope/receipt schemas from provider observations | c004 `6ed37d179f…`, c005 `8f05615667…` |
| f02-yylo-lease-fence | clean-room | MIT | Fencing lease tokens gating mutating task ops | c007 `fbb900ad6a…` |
| f02-yylo-merge-landing | clean-room | MIT | Native-git delivery adapter (CAS, QUEUED/CONFLICT/GIT_INTEGRATED) | c006 `9e5426a3d6…` |
| f02-yylo-risk-tiers | clean-room | MIT | Risk-tiered validation depth | c012 `1c282de21d…`, c013 `82bcea6a05…` |
| f02-ic-approvals | clean-room | Apache-2.0 (dual) | Durable approval store + scoped capability leases + CAS | c008 `f5bb8dc702…` |
| f02-ic-runtime-policy | clean-room | Apache-2.0 (dual) | Deterministic monotonic runtime-policy resolver, fail-closed | c009 `4cc1a18df5…` |
| f02-ohmpi-acp-gate | clean-room | MIT | ACP permission gate (fixed tool map, unknown fail-closed) | c010 `7918ebae50…` |
| f02-acp-http-adapter | depend | Apache-2.0 | Published crate acp-http-adapter (pin exact 0.4.2, wrap behind trait, verify permission callbacks) | c011 `bf562d6616…` |

## Vectors folded into P02
- V1 Replay Flight Recorder (02,06): 5 trace fixtures (freeze→correction→ack; 2nd correction typed-reject; land-without-token denial; spawn-escalation reject; next-transition determinism), 20× deterministic, no-socket guard.
- V2 Gate Mutation Calibration (02): 12 mutants (8 P02-AC + 4 P01 invariants); pass ≥11/12 typed rejections.
- V5 Spawn-Policy Decision Matrix (02,06): generate 6 flagship roles × 15 tools × 3 intents (~270 cells); 0 false denials on declared allows.

## Guardrails (binding)
- Q9 bar: port `studio-core/src/verify/*` behind Q9; rewrite where contract is wrong (LIVE tree → FROZEN candidate). RolePack + SpawnPolicy non-escalation, typed failures everywhere.
- acp-http-adapter: 0.x churn — pin exact, trait-wrap, lockfile-impact note per GOAL.md 6.
- Vendored approvals.rs: trim ts-rs if cockpit codegen lands later.
- Scope firewall: no mobile/cloud/marketplace/voice; A1 approval-surface variant is P04/05 work — NOT in this phase.
- Evidence: every claim carries hash (VERIFIED / FILE_HASH_ONLY / UNVERIFIED_HYPOTHESIS per parent GOAL.md).
- DoD: command-first; browser only at MS-1/MS-2.

## Acceptance (unchanged from parent GOAL.md)
- [ ] gate order executable end-to-end on a fixture task
- [ ] freeze immutable + reproducible
- [ ] second correction rejected typed
- [ ] land denied without burned token
- [ ] spawn escalation rejected typed
- [ ] tests cover all four
