# 04-cockpit-shell-ms1 — Minimal cockpit shell + MS-1 browser gate

**Program:** STC v2 greenfield rebuild · run `stc-greenfield` · branch `v2-greenfield`
**Frontier:** `file://.research/frontier.json` sha256 `86b6c81b505caccdffed71d0ca5139047cf4bd693e838791ab306025a52a9af7`
**DoD:** command-first; browser verification only at MS-1 (phase 04) and MS-2 (phase 06).

## Goal
Introduce studio-web (axum+tower) serving the browser SPA and /api projection reads, plus ONE real view rendering live projection data with staleness badges. Server owns projection; browser reads only. Carries MILESTONE MS-1.

## Acceptance criteria
- [ ] studio-web serves SPA + /api real projection
- [ ] MS-1 live browser renders real data + staleness badge + zero mock, screenshots
- [ ] browser read-only
- [ ] missing daemon fails closed legibly
- [ ] frontend build + cargo test green

## Brief (to programmer)
Add studio-web crate: axum + tower, serve SPA static assets, expose /api projection reads. Rewrite cockpit/ browser-first (no Tauri). Wire one view to real projection data with staleness badge. Verify MS-1 live via chrome-devtools against real data; capture screenshots. Zero mock data.

## Evidence discipline
Every claim in `dossier.json` carries a hash. VERIFIED = quote re-checked in the content-addressed cache.
FILE_HASH_ONLY = real file + real sha256; claim about the file not independently verified.
UNVERIFIED_HYPOTHESIS = direction only; not settled design.

## Swarm status (honest)
Gate cycle 1/5. Both swarms FAILED (`alpha_dossier.json` missing); beta produced one P1 dossier; alpha+audit never ran.
Manager archived the 7 beta-cited sources and re-verified 6 quotes via `iumbtems_verify_quote` (0.98).
