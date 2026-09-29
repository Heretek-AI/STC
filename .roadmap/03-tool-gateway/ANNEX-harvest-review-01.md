# ANNEX harvest-review-01 → 03-tool-gateway (execution brief)

**Run:** `harvest-review-01` · **Gate-1:** approved (all 35 + 9 vectors + A1) · **Phase:** 03-tool-gateway · **Predecessor:** 02-contract-runtime SIGNED OFF (R2 A-pass/B-conditional; waivers + residuals recorded at sign-off)
**Parent GOAL:** `file://.roadmap/03-tool-gateway/GOAL.md` · **Parent dossier:** `file://.roadmap/03-tool-gateway/dossier.json` (verdict: pending)
**Gate dossier:** `file://.roadmap/harvest-review-01/dossier.json` sha256 `7cac8a00e6cdb0dba39e230e3f47d1bb91558eebc835f5d4cbe3f0ba4d100e5e`

## Phase hashes cited (from parent dossier.json)
- frontier: `86b6c81b505caccdffed71d0ca5139047cf4bd693e838791ab306025a52a9af7` (file://.research/frontier.json, VERIFIED_HASH — NOTE P02 lesson: file absent in-tree; live frontier is `.factory/frontier.json` `5bf2863f…`; do NOT assert sha256sum match against the absent path; cite dossier claim + footnote)
- v1 MCP registry (port candidate): `4396baade9cdacac6a91d95c0473643b63436dbcf8c772cdb42bc967e57b8096` (file://studio-core/src/mcp/registry.rs, FILE_HASH_ONLY — cite reproducible revision form per P02 §E4.3, no caret syntax)
- beta dossier: `082c5c6e1bc543996d2372206e7ca8123ba02e2f60e5108f53fbbf6fa206dc1` (alpha_completed=false → HYPOTHESIS)

## Harvest evidence cited
- darkharvest dossier.json: `17f5ceaaa0a46b78060d402abc2a3087a7d08cf3460604ca6969655682e076e6` (60/60 manager-recomputed OK)
- brainstorm REPORT.md: `9ed6ddfab988dfe7d69b0a2eaec7fbb1f854060b6bd48004b963c73f88691773` (32/32 OK)

## P03 adoptions (8) — implement with phase acceptance criteria
| ID | Verdict | License | Item | Citations (sha256) |
|---|---|---|---|---|
| f03-approval-tiers | clean-room | MIT | Tool approval tiers (read/write/exec) + object policies (allow/deny/prompt, override+reason) + unknown→exec + MCP=write — the write-intent contract (pairs with P02 ACP gate c010) | c014 `f6e4638194…` |
| f03-mcp-admission | clean-room | Apache-2.0 (dual) | MCP adapter charter: manifest→capabilities, admission ceilings, schema/description bounds, tool-name grammar, typed failures, single egress seam | c015 `7ad76256c1…`, c016 `3e283a44f8…` |
| f03-fastmcp-lens | clean-room | Apache-2.0 | Tool-lens primitives: enable/disable keys+tags (disabled = unlisted + uncallable), search-tools transform, version-range filters | c017 `329fd541c0…`, c018 `27d825c8e4…`, c019 `f4d13cb0bc…` |
| f03-fastmcp-cli | clean-room | Apache-2.0 | Schema-to-CLI generation: typed subcommands + --help + SKILL.md from tool schemas | c020 `dce670f72e…` |
| f03-vk-mcpconfig | clean-room | Apache-2.0 | Cross-harness MCP config read/write (JSON/TOML/JSONC) with comment preservation | c021 `4d6e1db7f9…` |
| f03-swe-aci | clean-room | MIT | Composable minimal ACI tool bundles + submit/review gate as YAML config | c022 `96aeb863cb…`, c023 `91a7a21429…` |
| f03-openchamber-toolmatch | clean-room | MIT | Tool-name wildcard grammar with hard bounds (single trailing *, length caps, 16 tools max) | c024 `54a88c0c13…` |
| f03-agentdeck-mcppool | clean-room | MIT | MCP process pool: socket proxy/HTTP lifecycle, scope launcher, FD-leak regression tests | c025 `b037e3a1d7…`, c026 `00bd17ff74…` |

## Vector folded into P03
- V3 Tool Misbehavior Fixture Farm: 5 in-repo stdio fixture tools + 5 tests (hang, secret-echo, order-dependent, egress-attempt, schema-rugpull); every hostile outcome a typed denial/timeout/redaction; zero secret material in outputs; in-process, no network, <20s. Kill: any hostile action completes unblocked.

## Guardrails (binding)
- Q9 bar: port `studio-core/src/mcp/*`; fail-closed posture — unknown tools deny typed, never widen; tool_open-on-demand semantics; do not preload schemas.
- Build on P02: approval tiers resolve through P02 policy resolver + ACP gate; unknown→exec pairs with P02 unknown-fail-closed posture (document the tier/gate interaction explicitly).
- P02 lessons applied: receipt header table must cite §-corrections, never assert irreproducible matches; v1 citations in reproducible revision form; counts exact.
- Evidence: every claim carries hash (VERIFIED / FILE_HASH_ONLY / UNVERIFIED_HYPOTHESIS).
- Scope firewall: no marketplace/cloud/mobile/voice. DoD command-first.

## Acceptance (unchanged from parent GOAL.md)
- [ ] MCP registry loads from manifest
- [ ] Allow/OnDemand/Deny slicing works
- [ ] lazy schema load keeps index bounded
- [ ] unknown tool denied typed
- [ ] write-detect + overflow tested
