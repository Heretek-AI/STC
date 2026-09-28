# 02-contract-runtime — Contract & verification runtime (evidence-gated delivery)

**Program:** STC v2 greenfield rebuild · run `stc-greenfield` · branch `v2-greenfield`
**Frontier:** `file://.research/frontier.json` sha256 `86b6c81b505caccdffed71d0ca5139047cf4bd693e838791ab306025a52a9af7`
**DoD:** command-first; browser verification only at MS-1 (phase 04) and MS-2 (phase 06).

## Goal
Implement the moat mechanism as reusable runtime: freeze an immutable candidate (lineage/revision/target hashes), allow exactly ONE bounded correction, issue a burned acknowledge token, and make the burned receipt what delivery requires. Non-escalating spawn policy with typed rejection. Deterministic next-transition for DAG nodes.

## Acceptance criteria
- [ ] gate order executable end-to-end on a fixture task
- [ ] freeze immutable + reproducible
- [ ] second correction rejected typed
- [ ] land denied without burned token
- [ ] spawn escalation rejected typed
- [ ] tests cover all four

## Brief (to programmer)
Port studio-core/src/verify/* behind the Q9 bar; rewrite where the contract is wrong (prior review ran against a LIVE tree into a FROZEN candidate). Implement RolePack contract + SpawnPolicy non-escalation. Typed failure reasons everywhere. No swarm involvement.

## Evidence discipline
Every claim in `dossier.json` carries a hash. VERIFIED = quote re-checked in the content-addressed cache.
FILE_HASH_ONLY = real file + real sha256; claim about the file not independently verified.
UNVERIFIED_HYPOTHESIS = direction only; not settled design.

## Swarm status (honest)
Gate cycle 1/5. Both swarms FAILED (`alpha_dossier.json` missing); beta produced one P1 dossier; alpha+audit never ran.
Manager archived the 7 beta-cited sources and re-verified 6 quotes via `iumbtems_verify_quote` (0.98).
