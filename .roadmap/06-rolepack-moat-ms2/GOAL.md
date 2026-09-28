# 06-rolepack-moat-ms2 — RolePack moat (omp + pi + opencode) + MS-2 operator run

**Program:** STC v2 greenfield rebuild · run `stc-greenfield` · branch `v2-greenfield`
**Frontier:** `file://.research/frontier.json` sha256 `86b6c81b505caccdffed71d0ca5139047cf4bd693e838791ab306025a52a9af7`
**DoD:** command-first; browser verification only at MS-1 (phase 04) and MS-2 (phase 06).

## Goal
Prove the moat with content: one RolePack source compiled to omp AgentDefinition, pi package, and .opencode/agents/, plus the ported role catalog (subject to the Q9 bar). Close with an end-to-end operator run visible in the browser. Carries MILESTONE MS-2.

## Acceptance criteria
- [ ] one pack source compiles to all three targets
- [ ] ported catalog passes tests+clippy citing v1 sha256
- [ ] MS-2 full operator run visible/controllable in browser with live proof + screenshots
- [ ] four pillars visible in one browser session
- [ ] non-escalation holds across targets
- [ ] kill-gate: >15% fidelity gap cuts multi-harness

## Brief (to programmer)
Author one RolePack source; implement compile to omp + pi + opencode. Port the existing ~30-role catalog (studio-core/src/roles) subject to the Q9 bar. Run MS-2 operator flow live in the browser; capture evidence. If compiled fidelity trails native by >15%, CUT multi-harness (kill-gate), do not extend.

## Evidence discipline
Every claim in `dossier.json` carries a hash. VERIFIED = quote re-checked in the content-addressed cache.
FILE_HASH_ONLY = real file + real sha256; claim about the file not independently verified.
UNVERIFIED_HYPOTHESIS = direction only; not settled design.

## Swarm status (honest)
Gate cycle 1/5. Both swarms FAILED (`alpha_dossier.json` missing); beta produced one P1 dossier; alpha+audit never ran.
Manager archived the 7 beta-cited sources and re-verified 6 quotes via `iumbtems_verify_quote` (0.98).
