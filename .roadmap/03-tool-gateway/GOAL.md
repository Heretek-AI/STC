# 03-tool-gateway — Dynamic MCP gateway / Tool Lens (Pillar 3)

**Program:** STC v2 greenfield rebuild · run `stc-greenfield` · branch `v2-greenfield`
**Frontier:** `file://.research/frontier.json` sha256 `86b6c81b505caccdffed71d0ca5139047cf4bd693e838791ab306025a52a9af7`
**DoD:** command-first; browser verification only at MS-1 (phase 04) and MS-2 (phase 06).

## Goal
A dynamic MCP gateway: slice per-role tool manifests (Allow/OnDemand/Deny), load full tool schemas on demand, detect write-intent, and cap context overflow. Fail closed on unknown tools with a typed reason.

## Acceptance criteria
- [ ] MCP registry loads from manifest
- [ ] Allow/OnDemand/Deny slicing works
- [ ] lazy schema load keeps index bounded
- [ ] unknown tool denied typed
- [ ] write-detect + overflow tested

## Brief (to programmer)
Port studio-core/src/mcp/* behind the Q9 bar. Preserve fail-closed posture: unknown tools deny typed, never silently widen. Keep tool_open-on-demand semantics; do not preload all schemas.

## Evidence discipline
Every claim in `dossier.json` carries a hash. VERIFIED = quote re-checked in the content-addressed cache.
FILE_HASH_ONLY = real file + real sha256; claim about the file not independently verified.
UNVERIFIED_HYPOTHESIS = direction only; not settled design.

## Swarm status (honest)
Gate cycle 1/5. Both swarms FAILED (`alpha_dossier.json` missing); beta produced one P1 dossier; alpha+audit never ran.
Manager archived the 7 beta-cited sources and re-verified 6 quotes via `iumbtems_verify_quote` (0.98).
