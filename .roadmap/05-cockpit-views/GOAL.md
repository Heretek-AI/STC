# 05-cockpit-views — Operator views: War Room, Agent Stream, Audit+Gatekeeper, MCP Registry, Providers

**Program:** STC v2 greenfield rebuild · run `stc-greenfield` · branch `v2-greenfield`
**Frontier:** `file://.research/frontier.json` sha256 `86b6c81b505caccdffed71d0ca5139047cf4bd693e838791ab306025a52a9af7`
**DoD:** command-first; browser verification only at MS-1 (phase 04) and MS-2 (phase 06).

## Goal
Deliver the high-density operator surface (Pillar 4): five views, all projection-only reads with staleness badges, and designed empty/loading/error states (not defaults).

## Acceptance criteria
- [ ] all five views verified live with real data + screenshot each
- [ ] empty/loading/stale/error states designed with per-view scoped retry
- [ ] zero mock data
- [ ] tokens + tabular-nums consistency
- [ ] keyboard nav + AA contrast baseline measured
- [ ] builds green

## Brief (to programmer)
Build the five views over /api. Every view is a read projection with a staleness badge; UI never owns state. Design non-default states with per-view scoped retry. Verify each view live via chrome-devtools with screenshots.

## Evidence discipline
Every claim in `dossier.json` carries a hash. VERIFIED = quote re-checked in the content-addressed cache.
FILE_HASH_ONLY = real file + real sha256; claim about the file not independently verified.
UNVERIFIED_HYPOTHESIS = direction only; not settled design.

## Swarm status (honest)
Gate cycle 1/5. Both swarms FAILED (`alpha_dossier.json` missing); beta produced one P1 dossier; alpha+audit never ran.
Manager archived the 7 beta-cited sources and re-verified 6 quotes via `iumbtems_verify_quote` (0.98).
