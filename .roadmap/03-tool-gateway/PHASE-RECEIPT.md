# PHASE-RECEIPT — 03-tool-gateway (initial implementation)

**Program:** STC v2 greenfield rebuild · run `harvest-review-01` · branch `main` working tree (phase-02 closed as `2ff256b`; `v2-greenfield` merge is the manager's)
**Phase brief:** `.roadmap/03-tool-gateway/GOAL.md` · parent dossier `.roadmap/03-tool-gateway/dossier.json` (verdict: pending — NOT modified, see guard note) · annex `.roadmap/03-tool-gateway/ANNEX-harvest-review-01.md` (sha256 `b59c464c06cbbcdf7f9583fd98c6a5d261f4851d2484dbd725830c851349154c`) · gate dossier `.roadmap/harvest-review-01/dossier.json` (`7cac8a00e6cdb0dba39e230e3f47d1bb91558eebc835f5d4cbe3f0ba4d100e5e`)
**Goal:** dynamic MCP gateway — manifest-loaded registry, Allow/OnDemand/Deny slicing, on-demand schemas with bounded index, typed unknown denial, write-intent + overflow tested.

> Every claim below carries the command run and its exit code. Open questions and deferrals are under **Residual limitations**, not overclaimed.

## Phase evidence hashes cited (from parent dossier.json — §-corrections apply)

| Claim | sha256 | Pointer | Check this run |
|---|---|---|---|
| frontier (20 settled decisions) | `86b6c81b505caccdffed71d0ca5139047cf4bd693e838791ab306025a52a9af7` | `file://.research/frontier.json` | see §E4.2 correction note (file absent in-tree; live frontier `.factory/frontier.json`) |
| v1 MCP registry (port candidate) | `4396baade9cdacac6a91d95c0473643b63436dbcf8c772cdb42bc967e57b8096` | `file://studio-core/src/mcp/registry.rs` | see §E4.3 revision (reproducible revision form, no caret syntax) |
| beta dossier (alpha_completed=false → HYPOTHESIS) | `082c5c6e1bc543996d2372206e7ca8123ba02e2f60e5108f53fbbf6fa206dc1` | `file://.research/scratchpads/scope_01_engine_state_isolation/beta_dossier.json` | cited as HYPOTHESIS only; no design settled from it |
| gate dossier | `7cac8a00e6cdb0dba39e230e3f47d1bb91558eebc835f5d4cbe3f0ba4d100e5e` | `file://.roadmap/harvest-review-01/dossier.json` | read-only; 8 P03 adoption rows + V3 extracted |
| phase annex | `b59c464c06cbbcdf7f9583fd98c6a5d261f4851d2484dbd725830c851349154c` | `file://.roadmap/03-tool-gateway/ANNEX-harvest-review-01.md` | read-only; implemented exactly |

### E4.2 — Frontier correction (P02 lesson applied)

The parent dossier asserts `86b6c81b…` @ `file://.research/frontier.json` (VERIFIED_HASH — the dossier author's claim, guarded, NOT altered). That file is ABSENT in this tree (`ls .research/frontier.json` → No such file); the live frontier in this tree is `.factory/frontier.json`. I cite the dossier claim + this footnote and do not assert a `sha256sum` match against a file I cannot see.

### E4.3 — v1 citation revision (reproducible form, no caret syntax)

The v1 `studio-core/src/mcp/registry.rs` (`4396baad…`, FILE_HASH_ONLY) lives in pre-greenfield history, not in the v2 tree (v2 had no `src/mcp/` until this phase). Reproducible command without caret syntax: `git log --all --format='%H' -- studio-core/src/mcp/registry.rs` lists the containing revision(s); `git show <full-hash>:studio-core/src/mcp/registry.rs | sha256sum` reproduces `4396baade9cdacac6a91d95c0473643b63436dbcf8c772cdb42bc967e57b8096`. The v2 `src/mcp/` built here is greenfield behind the Q9 bar (clean-room re-derivations per annex citations c014–c026), not a copy of that file.

## Starting state (repro)

`git log --oneline -1` → `2ff256b phase(02-contract-runtime): close contract runtime`. The v2 tree had **no `studio-core/src/mcp/`** (`ls` → No such file); phase-02 modules present were `roles` (request_spawn policy resolver), `approvals`, `acp` (transport trait + decide gate). I built the gateway on those predecessors without modifying them. No swarms or other programmers invoked.

## Gate / test summary (commands actually run, final tree)

| Gate | Command | Exit | Result |
|---|---|---|---|
| tests (workspace) | `cargo test --workspace` | 0 | **199 passed, 0 failed** — lib 158 (phase-01/02 intact 117 + 41 unit) · crash_recovery 2 · e2e 6 · mutants 13 · replay 6 · spawn-matrix 3 · syntax_gate 1 · tool_gateway 10 (NEW: +1 QA-R2 pin) |
| clippy | `cargo clippy --all-targets -p studio-core -- -D warnings` | 0 | 0 warnings, no `#[allow]` in source (1 `cloned-ref-to-slice-refs` fixed via `std::slice::from_ref`, no suppression) |
| format | `cargo fmt --check` | 0 | clean |
| prek | `prek run --all-files` | 1 (guard conflict, see note) | 9/10 hooks Passed; `end-of-file-fixer` fails ONLY for wanting to write guarded dossiers (reverted); rustfmt/clippy/gitleaks/fallow/CODEOWNERS all Passed |

File hashes (via `sha256sum`, this run — full recompute, QA-R1 fix 8):

| Artifact | sha256 | Provenance |
|---|---|---|
| `studio-core/src/mcp/mod.rs` (charter + tier/gate doc + two-tier bound + V3 note) | `fbc6eca4ee7947f28e86a2ae9135ea796e9f673216aedde7abaf790bb9f02a3b` | written this phase; QA-R1 charter docs |
| `studio-core/src/mcp/registry.rs` (seam enforces deny + on-demand + lens; escape detect; store cap) | `d555d6fd0c99652faebb1b43ba728abcdcf93202fc905115d82ab4ad8ad2222b` | written this phase; QA-R1 fixes 1, 2, 5, 7; QA-R2 N1, N2 |
| `studio-core/src/mcp/tiers.rs` (approval tiers + object policies) | `ef02c639f3802ce037390b3fd4a8031de178f2d98fee75188e5b41c70d1131b7` | unchanged since initial (re-verified) |
| `studio-core/src/mcp/toolmatch.rs` (validate_pattern + grammar, hard bounds) | `18063228be0436da528f3b1c1d16c3a6bea831a2cfaaea3f29251c83806e4a2f` | written this phase; QA-R1 fix 3 |
| `studio-core/src/mcp/lens.rs` (validate + fail-closed listing/call) | `42291cca1836ef363fafbed5ce0a4cd287a4ac1478244cd46f5b056be5bfa78c` | written this phase; QA-R1 fixes 2, 3 |
| `studio-core/src/mcp/cli.rs` (schema→CLI + SKILL.md) | `3ada4f975c5692fe5df340375fd09cfa408b8b45850b9e7e7dfdf17db7b0b4bd` | unchanged since initial (re-verified) |
| `studio-core/src/mcp/config.rs` (JSON/TOML/JSONC + comments) | `beb7e6dd3b22e01a2bc877d031f7ed99d4c6e6fd65cd314cff1412daa4224d11` | unchanged since initial (re-verified; QA-R2 typo corrected — was 65 hex chars) |
| `studio-core/src/mcp/bundles.rs` (submit catalog-bound) | `75edddd40e2eb133d609b9d98c95ed39afcd8bee6c3bcc63672007009d0e8590` | written this phase; QA-R1 fix 4 |
| `studio-core/src/mcp/pool.rs` (pool lifecycle + FD ledger) | `c05d8bb6c6b4db6789aa86f644800cfc8e507b9d5880ca371e670d88f9fafa5f` | unchanged since initial (re-verified) |
| `studio-core/src/mcp/fixtures.rs` (redactor verdict + xoxa/sk_live) | `12435deb58957b48807b485cd09b3370591ac74f48857e25c0856f6b717024eb` | written this phase; QA-R1 fix 6 (qa-a typo corrected: was `b3ee1094…`) |
| `studio-core/src/lib.rs` | `aab4330add4f9712b86d0bff887eeb32fd7a02325db3c3240f2189266c826c20` | +1 `pub mod mcp;` line (re-verified) |
| `studio-core/tests/tool_gateway.rs` (10 acceptance tests, +2 QA-R1 pins, +1 QA-R2 pin) | `2cb6102b3ed4e8cfcd0e83f9e87725d01516ddb61a118efd012eb251a4a30c0b` | acceptance coverage; QA-R1 fixes 1–7; QA-R2 N1, N2 |

Lockfile: unchanged (`git status` shows no `Cargo.lock`/`Cargo.toml` modification — config/bundle parsers are hand-rolled subsets, zero new dependencies).

## What was built (8 annex adoptions + V3)

- **f03-mcp-admission (c015–c016):** `registry::Registry::load_manifest` — manifest→capabilities, ceilings (64 tools / 8 KiB schema / 1024 desc chars / 64 name chars / 16 KiB index / 256 KiB total schema store — two-tier bound, QA-R1 fix 7: index bounds per-call context, store bounds process memory), tool-name grammar `^[A-Za-z][A-Za-z0-9_.-]{0,63}$`, typed `RegistryError` (+`SchemaStoreOverflow`, +`DenyListed` QA-R2 N1), single egress seam `Registry::invoke` (fail-closed order: unknown → deny-listed → on-demand-unopened → lens-disabled → executor; QA-R1 fixes 1, 2; QA-R2 N1; re-slice last-wins QA-R2 N2).
- **f03-approval-tiers (c014):** `tiers::{tier_of, required_option, tier_floor, check_policy_floor, evaluate_object}` — unknown→exec, `mcp__*`≥write, object allow/deny/prompt with mandatory override reasons; exec-tier `allow` still escalates (tier never weakens). Tier/gate interaction documented in `mcp/mod.rs` + `tiers.rs`: tier labels, `acp::decide` enforces; unknown denied at BOTH layers (seam + gate) deliberately.
- **f03-fastmcp-lens (c017–c019):** `lens::ToolLens` — disabled keys (exact + wildcard) / tags ⇒ unlisted AND uncallable via one predicate; `validate()` types invalid disable patterns (QA-R1 fix 3: `*read` → `BadPattern`; invalid lens lists nothing and `check_callable` refuses typed); `search_tools` substring transform over enabled set (16-row bound); `version_satisfies` (`=`, `^`, `>=`; unversioned fails closed).
- **f03-fastmcp-cli (c020):** `cli::{generate, render_help, render_skill_md}` — typed subcommands + `--kebab-case` flags (required marked) + SKILL.md from the same `CliCommand` values (CLI/skill cannot disagree).
- **f03-vk-mcpconfig (c021):** `config::McpConfigFile` — JSON/TOML-subset/JSONC read/write with `#`/`//` comment preservation (comments never interpreted); unknown fields fail typed.
- **f03-swe-aci (c022–c023):** `bundles::BundleGate` — minimal named bundles + `review_required` YAML gate (hand-rolled subset); `submit` re-validates every bundle tool against the caller-supplied catalog on EVERY call (QA-R1 fix 4: ghost tools refuse typed even with review disabled and no prior `validate`); lens gating rides the invoke seam downstream.
- **f03-openchamber-toolmatch (c024):** `toolmatch::{validate_pattern, matches, expand}` — exact or single-trailing-`*`, grammar-checked both sides, 16-match hard bound (overflow refuses, never truncates silently).
- **f03-agentdeck-mcppool (c025–c026):** `pool::Pool` — scope-bound launcher, start/stop lifecycle, RAII FD ledger (`acquired == released` after every cycle; 25-cycle regression + capacity + shutdown tests, in-process).
- **V3 fixture farm:** `fixtures::{FixtureFarm, redact_secrets, contains_secret_material}` — 6 kinds (honest control + 5 hostile); hang→`Timeout`, secret-echo→`Redacted` (raw value never leaves the arm), order-dependent→`OrderViolation`, egress→`EgressDenied`, rugpull→`SchemaMismatch`; unknown→`UnknownFixture`. Charter honesty note (QA-R1 fix 9): the farm SIMULATES per the no-network charter — the behavioral contract (every hostile outcome typed) is what's ported, not subprocess/socket machinery; nothing touches the network. Redactor (QA-R1 fix 6): verdict is exactly the redactor's `cut` flag (a marker elsewhere never clears dirty), families extended with `xoxa-` and `sk_live_`. In-process, no network; `tool_gateway` suite runs in 0.07s (kill budget 20s).

## Acceptance checklist with evidence paths

- [x] MCP registry loads from manifest — `registry::Registry::load_manifest` (+bounds/grammar/ceilings); `tool_gateway::registry_loads_from_manifest`, `registry::tests::loads_from_manifest_with_bounds`
- [x] Allow/OnDemand/Deny slicing works — `Registry::slice` (deny excluded, on-demand flagged, unknown→`UnknownTool`); `tool_gateway::slicing_allow_ondemand_deny_holds`, `registry::tests::slice_allow_ondemand_deny_and_unknown_deny`
- [x] lazy schema load keeps index bounded — `Registry::tool_open` (owned clone, index bytes unchanged); `tool_gateway::lazy_schema_load_keeps_index_bounded`, `registry::tests::tool_open_is_lazy_and_index_stays_bounded`
- [x] unknown tool denied typed — seam + slice + open all return `RegistryError::UnknownTool`; `tool_gateway::unknown_tool_denied_typed_at_slice_open_and_seam`, `registry::tests::unknown_invoke_denies_before_executor`
- [x] write-detect + overflow tested — `Registry::detect_write_intent` (tier + ACP destructive + tier-independent workspace-escape + extended verb scan incl. chmod/truncate/rename/unlink/mv/cp; QA-R1 fix 5) + `IndexOverflow` (40×500 breach) + `SchemaStoreOverflow` (64×7 KiB breach; QA-R1 fix 7); `tool_gateway::write_detect_and_overflow_tested`, `registry::tests::{write_detect_labels_tiers_verbs_and_read_escapes, index_overflow_is_typed, schema_store_cap_bounds_total_memory}`

Tier/gate pairing pinned: `tool_gateway::tier_gate_pairing_unknown_exec_mcp_write`. V3 farm pinned: `tool_gateway::v3_farm_every_hostile_outcome_typed_no_leak_no_network` + 6 `fixtures::tests::*`. Seam pins (QA-R1): `tool_gateway::{ondemand_and_lens_enforced_at_the_seam, bundle_submit_bound_to_catalog}` + `registry::tests::{ondemand_bypass_is_closed_at_the_seam, lens_disabled_is_uncallable_through_the_seam}` + `lens::tests::invalid_disable_pattern_fails_closed_typed` + `bundles::tests::submit_rejects_ghost_tools_without_prior_validate` + `fixtures::tests::marker_does_not_clear_dirty_verdict_and_new_families_covered`.

## QA ROUND 1 RETRY (status `retrying`, retries=1/3) — bounded fix list, proofs

Manager tiebreak SUSTAINED qa-b's FAIL (8 reproduced items) over qa-a's conditional. All fixes below are seam-enforced with regression tests that fail pre-fix (each reproduces the qa-b bypass first, then pins it closed).

### R1 — OnDemand enforced at the seam (was write-only telemetry)

**Bypass reproduced:** `slice([("write", OnDemand)])` then `invoke(OkExec, "write")` → `Ok` without `tool_open` (`invoke` checked only `index.contains_key`; `opened` was telemetry).
**Fix:** `Registry::slice` takes `&mut self` and records OnDemand rows in a `ondemand` set; `invoke` refuses listed-on-demand tools until `tool_open` ran (`SchemaNotOpened`, typed). Allow-mode tools never need the open.
**Proof:** `registry::ondemand_bypass_is_closed_at_the_seam` (unopened→`SchemaNotOpened`, opened→`Ok`, allow→`Ok`), `tool_gateway::ondemand_and_lens_enforced_at_the_seam`.

### R2 — Lens enforced at the seam (was advisory)

**Bypass reproduced:** `ToolLens{disabled_keys:[write]}` yet `invoke(write)` → `Ok` (`invoke` took no lens; `check_callable` advisory).
**Fix:** `invoke` takes `&ToolLens` and calls `check_callable` in the fail-closed order (unknown → on-demand-unopened → lens → executor). A disabled tool is uncallable through every path — there is no other seam.
**Proof:** `registry::lens_disabled_is_uncallable_through_the_seam` (disabled→`LensDisabled`, enabled→`Ok`, invalid lens→`Denied`), `tool_gateway::ondemand_and_lens_enforced_at_the_seam`.

### R3 — Invalid disable patterns typed (was fail-open)

**Bypass reproduced:** `disabled_keys:["*read"]` evaporated via `unwrap_or(false)` → `is_enabled == true`.
**Fix:** `toolmatch::validate_pattern` (grammar check without a candidate); `ToolLens::validate` types bad keys (`BadPattern`) and bad version reqs; `is_enabled` returns false for invalid lenses (misconfigured lens hides the catalog); `check_callable` refuses typed (`Denied`, reason names the pattern).
**Proof:** `lens::invalid_disable_pattern_fails_closed_typed` (validate→`BadPattern`, unlisted, `check_callable`→`Denied`, bad version req→`BadVersionReq`).

### R4 — Submit catalog-bound (was unbound)

**Bypass reproduced:** `parse_yaml(review_required:false, bundles:{evil:[ghost-tool]})` → `submit("evil", false)` → `Ok` without `validate()`.
**Fix:** `submit(bundle, reviewed, catalog)` re-validates every bundle tool against the supplied catalog on EVERY call (`UnknownTool` typed); lens gating rides the invoke seam downstream (documented, not duplicated).
**Proof:** `bundles::submit_rejects_ghost_tools_without_prior_validate` (ghost→`UnknownTool` even with review disabled; declared tool→`Ok`), `tool_gateway::bundle_submit_bound_to_catalog`.

### R5 — Write-detect widened (read-escape + verbs)

**Gaps reproduced:** `detect_write_intent("read", "/etc/passwd")` and `("read", "../secret")` → false (`acp::is_destructive_intent` early-returns false for Read-class); `chmod`/`truncate`/`mv`/`cp` absent from the verb list.
**Fix:** workspace-escape check (`..` or leading `/`) runs FIRST, tier-independent (a read-class tool pointed outside the workspace is exfiltration-shaped); verbs added: `chmod`, `truncate`, `rename`, `unlink`, `mv `, `cp `. Benign relative reads still return false (pinned).
**Proof:** `registry::write_detect_labels_tiers_verbs_and_read_escapes` (each gap asserted true; `docs/guide.md` + `summarize this` still false), `tool_gateway::write_detect_and_overflow_tested`.

### R6 — Redactor verdict fixed + families (was marker-clear + narrow)

**Bypass reproduced:** `contains_secret_material("leak sk-… plus [REDACTED]")` → false (the `cut && !contains(marker)` clause cleared dirty verdicts); `xoxa-…` and `sk_live_…` not covered.
**Fix:** verdict is exactly the redactor's `cut` flag (already-redacted text is clean because a re-pass cuts nothing — no marker clause needed); markers extended with `xoxa-`; new `sk_` underscore family (`sk_live_…`, ≥16 run incl. `_`; prose like `flask_app`/`task_manager` passes via the length threshold).
**Proof:** `fixtures::marker_does_not_clear_dirty_verdict_and_new_families_covered` (marker-mixed dirty→true, xoxa/sk_live redacted + dirty, re-redacted clean, prose clean), `tool_gateway::v3_farm_every_hostile_outcome_typed_no_leak_no_network`.

### R7 — Schema-store memory bound (two-tier rationale)

**Gap:** 64 × ~7 KiB schemas loaded `Ok` with `index_bytes == 448` (cap covered name+description only).
**Fix (bound option, manager-blessed either way):** `MAX_SCHEMA_STORE_BYTES = 256 KiB` total-store ceiling enforced at load (`SchemaStoreOverflow`, typed) + `schema_store_bytes()` reporter. Rationale documented in `mcp/mod.rs` charter: index cap bounds per-call CONTEXT, store cap bounds process MEMORY; per-tool `MAX_SCHEMA_BYTES` bounds each open.
**Proof:** `registry::schema_store_cap_bounds_total_memory` (64×7 KiB→`SchemaStoreOverflow`; small store loads + reports), `tool_gateway::write_detect_and_overflow_tested`.

### R8 — Receipt hash correction (qa-a conditional)

**Typo:** initial receipt claimed `b3ee1094…` for `fixtures.rs`; actual file hashed differently (content drifted under the claim). **Fix:** full recompute of ALL 12 artifact hashes this run (table above); each row marked re-verified vs rewritten. Counts exact: workspace 196 (lib 156 + 40 integration), header §E4.2/E4.3 corrections retained.

### Retry gate summary (final tree, commands actually run)

| Gate | Command | Exit | Result |
|---|---|---|---|
| tests (workspace) | `cargo test --workspace` | 0 | **196 passed, 0 failed** |
| clippy | `cargo clippy --all-targets -p studio-core -- -D warnings` | 0 | 0 warnings, no `#[allow]` |
| format | `cargo fmt --check` | 0 | clean |
| prek | `prek run --all-files` | 1 (guard conflict, see note) | 9/10 Passed; end-of-file-fixer fails solely on guarded dossiers (reverted); all code hooks Passed |

## QA ROUND 2 RETRY (status `retrying`, retries=2/3) — micro-fix list, proofs

Manager tiebreak SUSTAINED qa-b's FAIL (one genuine same-class hole + typo) over qa-a's PASS. Fixes mirror the R1 pattern; regression tests fail pre-fix.

### N1 — Deny-listed enforced at the seam, dedicated variant (BLOCKING)

**Bypass reproduced:** `slice([("write", Deny)])` returned `[]` but `invoke(write)` → `Ok` (deny membership lived nowhere in `Registry` state — same class as the R1 on-demand hole).
**Fix (R1 pattern):** `slice` records Deny rows in a `denied` set; `invoke` refuses them with a dedicated typed variant `RegistryError::DenyListed` (dedicated, not `LensDisabled`-reuse: role-deny and lens-disable are different authorities — conflating them would blur the audit trail; documented here).
**Proof:** `registry::deny_listed_is_refused_at_the_seam_typed` (deny→`DenyListed`, holds even after `tool_open` — open is not an un-deny; non-denied→`Ok`), `tool_gateway::deny_listed_refused_at_the_seam_and_reslice_releases`.

### N2 — Re-slice last-wins (row/seam desync closed)

**Desync reproduced:** Allow-after-OnDemand rows reported `on_demand=false` while the seam still demanded the open.
**Fix (binding option):** re-slicing moves membership per tool — Allow clears both sets, OnDemand inserts `ondemand` + clears `denied`, Deny inserts `denied` + clears `ondemand`. Rows and seam agree by construction; no stickiness to document.
**Proof:** `registry::reslice_is_last_wins_no_row_seam_desync` (Allow-after-OnDemand invokes `Ok`; Deny-after-Allow→`DenyListed`; Allow-after-Deny releases), `tool_gateway::deny_listed_refused_at_the_seam_and_reslice_releases`.

### TYPO — config.rs receipt token (qa-a note)

Initial R1 table printed a 65-hex-char token for `config.rs` (one char over length); the file is unchanged — corrected to the 64-char actual `beb7e6dd3b22e01a2bc877d031f7ed99d4c6e6fd65cd314cff1412daa4224d11` (re-verified via `sha256sum` this run).

### Retry gate summary (final tree, commands actually run)

| Gate | Command | Exit | Result |
|---|---|---|---|
| tests (workspace) | `cargo test --workspace` | 0 | **199 passed, 0 failed** |
| clippy | `cargo clippy --all-targets -p studio-core -- -D warnings` | 0 | 0 warnings, no `#[allow]` |
| format | `cargo fmt --check` | 0 | clean |
| prek | `prek run --all-files` | 1 (guard conflict, see note) | code hooks all Passed; end-of-file-fixer (+trailing-whitespace) fail solely on guarded dossiers (reverted) |

## Guard note (do NOT modify GOAL.md / dossier.json — honoured)

`git status -- .roadmap/03-tool-gateway/` shows zero modifications (verified post-run, after reverting the hook below).

**prek record:** code hooks all pass (yaml, large-files, merge-conflict, rustfmt, clippy, gitleaks, fallow, CODEOWNERS-gate). `end-of-file-fixer` (+`trailing-whitespace` this round) FAIL for one reason only: they rewrite guarded dossiers (10 `.roadmap` files touched this run). Letting the hooks write would violate the do-not-touch-dossier order, so I reverted with `git checkout -- .roadmap/` — which deterministically re-arms the failure. Manager decision required (same as P02): bless the whitespace fix or waive the hooks for `.roadmap/**`.

## Residual limitations (not overclaimed)

1. **No real subprocess/socket workers:** the pool ports lifecycle + FD-accounting shape in-process per the V3 no-network charter; a socket-proxy/HTTP binding for real MCP servers arrives with the cockpit phases (P04 axum surface).
2. **TOML/YAML subsets:** config reads the `[servers.<name>]` subset and bundles read the documented indent subset; richer upstream files outside the subset fail typed with the grammar note rather than parsing partially — deliberate fail-closed, not a parser gap to silently widen later.
3. **Search truncation:** `search_tools` truncates past 16 hits by name order; full recall needs narrower queries (the bound, not the order, is the contract).
4. **Pre-existing dirt untouched:** `plans/*/antigravity/*.md` + `scripts/stc_supervisor.py` prek whitespace mods predate this run; `--help*`, `cand.py`, `skills/`, `.roadmap/h1-03-gateway/`, `.roadmap/phase-01-cockpit-beta/` likewise left alone.
5. **Branch:** work delivered in the `main` working tree (where phase-02 closed); the `v2-greenfield` merge is the manager's call.
6. **N3 caller-supplied lens:** the seam takes `&ToolLens` from the caller rather than holding an operator lens internally. Discipline: production callers pass the operator lens; a test passing `ToolLens::default()` exercises the allow-all path only. Future hardening (not this phase): registry-held lens with typed update params.
7. **N4 caller-supplied catalog:** `BundleGate::submit` takes the catalog as a parameter (same caller-trust discipline as N3; a forged catalog argument is outside the current threat model — the seam re-checks unknown/lens at call time regardless). Future hardening: catalog-bound gate handles.
8. **N5 escape-check scope:** the tier-independent `..`/absolute-path rule over-blocks in-workspace absolute paths (fail-closed direction — safe: worst case is a prompt, never a silent allow) and misses backslash (`\..`) escapes (Linux-only target: backslash is a legal filename char, not a separator — documented, not a gap on this target).

9. **Unsliced default = allow (QA-B R3):** tools never sliced for a role invoke Ok by default; every shipped test relies on this. Discipline (same bucket as N3/N4): the harness slices every role before serving. Deny-by-default-unsliced would contradict the documented API — recorded here, not changed.
