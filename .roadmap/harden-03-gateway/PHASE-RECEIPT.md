# PHASE-RECEIPT — harden-03-gateway (FULL REWORK + hardening+merge)

**Run:** `harvest-harden-01` · **Phase:** `harden-03-gateway` · **Date:** 2026-10-05 UTC
**Mode:** FULL REWORK + hardening+merge, no new features beyond seam closure. No swarms or other programmers invoked (factory-programmer rule).

## 1. Phase evidence hashes cited

- **Frontier:** `harvest-harden-01` same-run, full rework + hardening+merge, cycle 1/5 gate approved, `duckduckgo` active.
- **Brainstorm fallback** `ses_ef5672448ffe1zw9QyVkogSaMM`: V03-A1 registry-held lens, V03-A2 catalog-bound handles, V03-B canonical containment, V03-C closed-by-default slicing, V03-D seam order + merge gate (A1+A2 together, then C, then B+D, matrix green before merge).
- **Darkharvest fallback** `ses_ef56662dcfferHnxbSKwM28EaF` report `92845a708a0a40a07b5eed63dd6fdc112df11f2164cc1ed21cfa459492e1bda6`: F1 lens (casbin-rs depend non-central only, rest clean-room), F2 handles (kani/proptest depend dev-only, rest clean-room), F3 containment (std canonicalize depend in-tree, openat2 clean-room wrapper deferred), F4 slicing receipts (proptest/kani dev-only), F5 ordered seam. All depend verdicts dev-only; no runtime vendor; GPL/AGPL never copied.
- **Anchors (pre-rework, verified):**
  - `registry.rs` `d555d6fd0c99652faebb1b43ba728abcdcf93202fc905115d82ab4ad8ad2222b`
  - `lens.rs` `42291cca1836ef363fafbed5ce0a4cd287a4ac1478244cd46f5b056be5bfa78c`
  - `bundles.rs` `75edddd40e2eb133d609b9d98c95ed39afcd8bee6c3bcc63672007009d0e8590`
  - `tiers.rs` `ef02c639f3802ce037390b3fd4a8031de178f2d98fee75188e5b41c70d1131b7`
  - `fixtures.rs` `12435deb58957b48807b485cd09b3370591ac74f48857e25c0856f6b717024eb`
  - `toolmatch.rs` `18063228be0436da528f3b1c1d16c3a6bea831a2cfaaea3f29251c83806e4a2f`
  - `worktree/mod.rs` `670244b77b7cca005e2c719de17a9abdedc5bae7bf534538b5e9a1acb3cffac2`
  - h1-03 receipt `8f47a35746f3b72ebb2c2e139dba3683c293a835ec8c433e6ac724d525ac566e`
  - priors `17f5ceaaa0a46b78060d402abc2a3087a7d08cf3460604ca6969655682e076e6`, `9ed6ddfab988dfe7d69b0a2eaec7fbb1f854060b6bd48004b963c73f88691773`, gate `7cac8a00e6cdb0dba39e230e3f47d1bb91558eebc835f5d4cbe3f0ba4d100e5e`

## 2. Goal implementation (in order)

### (1) V03-A1+A2 together — registry-held lens + catalog-bound handles
- `Registry::invoke(&self, executor, tool, args, receipt)` takes **no lens param**. Held `active_lens` only; mutation via `rotate_lens(lens, note)` which validates (`Denied` on invalid, no state change) and appends `LensAuditEntry {seq, generation, disabled_keys/tags/version_req, note}`. Forged-lens unrepresentable (compile-time: no param). `lens_audit()` exposes the trail.
- Bundle submit lives on `Registry::submit_bundle(&self, gate, bundle, reviewed, handle)` — **no caller `catalog: &[String]` param**. Catalog authority is `self.index`; ghost tools refuse `BundleError::UnknownTool` every submit. `CatalogHandle {generation}` has no public constructor (`catalog_handle()` only); stale generations refuse `BundleError::StaleHandle`. Ghost-submit unrepresentable.
- Tests: `held_lens_disabled_is_uncallable_no_forged_param` (unit), `forged_lens_unrepresentable_and_rotate_audited` + `forged_catalog_unrepresentable_ghost_submit_refused` (integration). Stale-handle pinned in `stale_receipt_and_handle_refuse_typed_after_reload`.

### (2) V03-C — closed-by-default + versioned receipts + reload invalidates
- Fresh `load_manifest` (gen 1) covers nothing; `invoke`/`tool_open` demand `SliceReceipt` and refuse `NotSliced` when `!covered.contains(tool)`. `slice()` returns `SliceReceipt {generation, covered, rows}` (private fields, only producer). Generation mismatch → `StaleHandle {expected, found}` at both seams. `reload(json)` swaps catalog, bumps generation, clears `covered/allowed/ondemand/denied/opened`; lens/root/rules survive; old receipts/handles invalidate typed.
- Tests: `fresh_registry_invokes_nothing_closed_by_default` (unit), `unsliced_deny_closed_by_default_and_reload_invalidates` (integration), `stale_receipt_and_handle_refuse_typed_after_reload` (both).

### (3) V03-B — canonical containment with bound root
- `bind_root(&Path)` refuses symlinks typed (self + ancestors), then `worktree::canonicalize_or_create`; stored root is canonical absolute. `check_containment(tool, target)` joins relative targets to root / normalizes absolute via `worktree::normalize_path`, demands component-wise `starts_with(root)` (so `<root>-evil` denies — not substring), and refuses symlink components up to root. Empty/missing root fails closed. `detect_write_intent` kept as heuristic with explicit doc that the authoritative path gate is canonical.
- Matrix green: inside allow, `a/./b/../c` allow (fold), `../outside` deny, `/etc/passwd` deny, prefix-sibling deny, symlink-inside deny (unix), symlinked-root bind deny (unix). No `contains()`-only path remains on the seam — every seam path runs canonical first.
- Tests: `containment_matrix_canonical_not_substring` (unit + integration `containment_matrix_canonical_with_bound_root`).

### (4) V03-D — single ordered egress + hostile farm through seam
- Order enforced in `invoke`: **unknown → coverage (StaleHandle, then NotSliced) → deny (DenyListed) → ondemand (SchemaNotOpened) → lens (LensDisabled/Denied, held only) → tier/object/write-intent incl. containment (ContainmentDenied/TierDenied) → executor (Denied)**. Tier stage: (i) canonical containment, (ii) read-tier write-intent escalation (`TierDenied`), (iii) object rules only when configured (empty = ambient so honest Write passes; non-empty maps any `evaluate_object` refusal to `TierDenied`). Executor `Ok` outputs scanned with `fixtures::contains_secret_material` → `Denied` redaction guard (no leak as `Ok`).
- Hostile farm through seam: `FixtureSeamExec` routes `FixtureFarm` via `Registry::invoke`; hang → `Denied` containing “timed out”, secret-echo → `Denied` with zero `sk-test` leak, order/egress/rugpull → `Denied` with order/egress/denial text, unknown → `UnknownTool`. Leaky `Ok(secret)` executor → seam redaction `Denied` with no leak.
- Tests: `seam_order_unknown_coverage_deny_ondemand_lens_tier_executor` (unit + integration `seam_order_...`), `hostile_farm_through_seam_typed_refusal_timeout_redaction`.

### (5) Merge proofs — no docs-only closure
- Code + tests changed (not docs-only): `registry.rs` full rework, `bundles.rs` submit-bound rework + `StaleHandle`, `mod.rs`/`pool.rs` seam-order docs, `tool_gateway.rs` 16 tests (5 pre-existing adapted + 5 vector + hostile + carryovers).
- New tests per vector: forged-lens, forged-catalog, containment matrix, unsliced-deny, seam-order (+ hostile-through-seam, stale-handle, audit).

## 3. Acceptance — commands + exit codes

- `cargo test --workspace` → exit `0`. All suites `ok`, `0 failed` (198 studio-core lib, 16 tool_gateway, 18 web_contract, plus remaining suites; `FAILED` count `0`).
- `cargo clippy --workspace --all-targets -- -D warnings` → exit `0`, no warnings.
- `cargo fmt --check` → exit `0`.
- No GPL/AGPL copy: `grep -ri GPL studio-core/src/mcp/` hits only the provenance comment “No GPL/AGPL code copied” (no copied code); darkharvest F-verdicts honored (no runtime vendor, no `casbin-rs`/`openat2` uptake).
- No new runtime deps: `git diff -- Cargo.toml studio-core/Cargo.toml Cargo.lock` empty; `kani`/`proptest` count `0` in `Cargo.lock` — hand-rolled receipts/handles/containment, no new deps. Dev-only `tempfile` (already in lockfile under `[dev-dependencies]`) reused for containment tests with no lockfile impact.
- No mobile/cloud/marketplace/voice: no new scope; only pre-existing `marketplace` string in `config.rs` unknown-field test data.
- Banned: no swarms/programmers invoked by this programmer (single phase brief per spawn).

## 4. Hashes (post-rework)

- `registry.rs` `ecba3eace41d5fc6f42c46b45a6ebe27bec2793f1233a34c2e052180c2cc3210`
- `bundles.rs` `fc354420e1b7f074f264f284ce312403bbb00bc1f691f830279e0c8e4e837942`
- `tiers.rs` `ef02c639f3802ce037390b3fd4a8031de178f2d98fee75188e5b41c70d1131b7` (unchanged)
- `lens.rs` `42291cca1836ef363fafbed5ce0a4cd287a4ac1478244cd46f5b056be5bfa78c` (unchanged)
- `fixtures.rs` `12435deb58957b48807b485cd09b3370591ac74f48857e25c0856f6b717024eb` (unchanged)
- `toolmatch.rs` `18063228be0436da528f3b1c1d16c3a6bea831a2cfaaea3f29251c83806e4a2f` (unchanged)
- `worktree/mod.rs` `670244b77b7cca005e2c719de17a9abdedc5bae7bf534538b5e9a1acb3cffac2` (unchanged, reused normalize + symlink refusal)
- `tests/tool_gateway.rs` `e93f77678df848a9123f3b4ba469193f33fb977e39e5afaff9c3009ca204ff12`
- `mcp/mod.rs` `e35423361b495bfcf1d2939320a150b7ef991ae167b24efd784b8bf0e067b975`
- `mcp/pool.rs` `eeeb9464fab3ba3fcba4c2c2e760f963d3ecc6436f7754c6f743a2ee84a70ae4`

## 5. Retry 1 — qa-b FAIL closure (F1–F5 enforcement, no swarms)

**Date:** 2026-10-05 UTC · **Run:** `harvest-harden-01` · **Phase:** `harden-03-gateway` · **Retry:** 1
**Mode:** Bounded enforcement fixes only (no docs-only closure, no new features, no runtime deps, no GPL). No swarms or other programmers invoked (factory-programmer rule: single phase brief per spawn).

### 5.1 Phase evidence hashes cited (pre-retry anchors)

- **Anchors (post-rework, pre-retry, verified against qa-b report):**
  - `registry.rs` `ecba3eace41d5fc6f42c46b45a6ebe27bec2793f1233a34c2e052180c2cc3210`
  - `bundles.rs` `fc354420e1b7f074f264f284ce312403bbb00bc1f691f830279e0c8e4e837942`
- **qa-b FAIL:** report `34d02b32f1462d4c1b7660e915e1e16a1f650d64487f5d3a71909eb59ba028c1`; repros `/tmp/opencode/qa-b-harden03*` (main `26333ab9e489f04f37f461b64cb73152ea231d5bd5464eb5de3c797d364c5a79`, edge2 `9edd2e262fd36e623f4c61d9b61e59bde29e1276d8c9c4e9e7d6bd9376f2c9c8`, edge3 `56ec4f38d30b5281e3136a1c5ef696469e469aba3b7ab96410830c6cdd337b25`); probe crate `c8ca51d26579ad6e422ff46db3612eda715fa016e7969230eca9bbd5a56d20ba` / `1d1c06f22a3f6b348e956bb4bc2d199d44bdca857a1f1a28f317e7c3d234afd3` / `64e5b8c542f3ca5bf486e6a8df84ef16e5a3881d7c0477f6f8557cd8606eb3c4`.
- **Core gates green pre-retry:** 284 suites `ok`, `0 failed` (198 studio-core lib, 16 tool_gateway, 18 web_contract); clippy `-D warnings` exit 0; fmt exit 0 (per qa-b §Core gates, hashes match receipt exactly).

### 5.2 Bounded fixes (enforcement, not docs)

- **F1 symlink-fold bypass (V03-B):** `Registry::check_containment` now walks symlink-aware PRE-fold (each `Normal` pushed onto `root`/`/` is `symlink_metadata`-checked BEFORE a later `..` can erase it) then `starts_with(root)` + defense-in-depth re-walk. `link/secret` stays DENY; `link/../evil`, `a/../link/../evil2`, abs `<root>/link/../evil3` now DENY (were ALLOW). Ancestor-above-root + check-then-use TOCTOU kept documented only (`openat2` deferred per darkharvest F3 clean-room verdict). Anchors: `Registry::check_containment`, `Registry::bind_root`.
- **F2 Err-path leak:** `Registry::invoke` `map_err` now scans `Err` strings with `fixtures::contains_secret_material`/`redact_secrets` same as `Ok` (secret-shaped `Err` → `Denied{reason:"output redacted: secret material blocked"}`, never echoes raw `sk-test` into `Denied` reason). Anchors: `Registry::invoke`, `fixtures::contains_secret_material`.
- **F3 receipt desync:** `invoke`/`tool_open` now enforce `receipt.covered`/`rows` binding to the live slice (not gen-only + audit-only). `tool_open` demands `receipt.covered` contain the tool (`NotSliced` otherwise). `invoke` demands `receipt.covered` (`NotSliced`) + `rows` mode match live (`Denied` desync when Deny→Allow or Allow↔OnDemand mismatch; live `DenyListed` still wins). `slice()` revokes `opened` for the sliced tool (fail-closed: OnDemand→Deny→OnDemand without fresh `tool_open` → `SchemaNotOpened`). `slice([read])` receipt no longer invokes `write` Ok; `slice([write Deny])`→Allow stale `r_deny` no longer Ok; stale `opened` no longer sticks. Anchors: `Registry::slice`, `Registry::tool_open`, `Registry::invoke`, `SliceReceipt::covered/rows`.
- **F4 redactor gaps:** `fixtures::redact_secrets` extends markers to `xoxo-/xoxr-/gho_/github_pat_` (same marker-replace as `xoxa/b/p/ghp_`) + `Bearer <token>` run ≥16 (whole `Bearer <token>` cut; `Bearer of` prose len 2 stays clean). Keeps `sk-/AKIA/xoxa/b/p/ghp_` green, no FP on `flask_app`/`task_manager` prose. `xoxo-`/`gho_`/`Bearer` `Ok` now `Denied` (were `Ok` leak). Anchors: `fixtures::redact_secrets`, `fixtures::contains_secret_material`.
- **F5 validate footgun:** `BundleGate::validate(catalog:&[String])` marked LINT-ONLY, NEVER AUTH (doc: caller snapshot, forged `ghost-tool` passes by construction; exists for offline lint/tests only). Submit seam stays bound-only (`Registry::submit_bundle` held index + `CatalogHandle`, ghost refused typed every submit). New test pins `validate(&forged)` Ok vs `submit_bundle` `UnknownTool`. Anchors: `BundleGate::validate`, `Registry::submit_bundle`, `CatalogHandle`.

### 5.3 Acceptance — commands + exit codes (post-retry)

- `cargo test --workspace` → exit `0`. All suites `ok`, `0 failed` (203 studio-core lib [+5 new: symlink-fold, err-redact, receipt-bind, redactor-families, validate-lint], 20 tool_gateway [+4 new: symlink-fold-through-seam, receipt-bind, err-redact, redactor-families], 18 web_contract, plus remaining suites = 293 total; prior 284 all still green, `FAILED` count `0`).
- `cargo clippy --workspace --all-targets -- -D warnings` → exit `0`, no warnings.
- `cargo fmt --check` → exit `0`.
- No GPL/AGPL copy: `grep -ri GPL studio-core/src/mcp/` hits only provenance comments “No GPL/AGPL code copied” (no copied code); darkharvest F-verdicts honored (no runtime vendor, no `casbin-rs`/`openat2` uptake, `kani`/`proptest` count `0` in `Cargo.lock`).
- No new runtime deps: `git diff -- Cargo.toml studio-core/Cargo.toml Cargo.lock` empty (hand-rolled walk/binding/redaction, std only + existing `serde`/`serde_json`/`thiserror`; dev-only `tempfile` reused with no lockfile impact).
- Repro re-run (external probe, path-dep, no repo edits): `link/../evil` DENY, `a/../link/../evil2` DENY, abs `link/../evil3` DENY; `LeakyErr(sk-test)` `Denied` with zero `sk-test` leak; old `r_read`→`write` `NotSliced`, old `r_deny`→Allow `Denied` desync, `opened` after Deny→OnDemand `false` + `SchemaNotOpened`; `xoxo-/gho_/Bearer` `contains_secret==true` + `Ok`→`Denied`; `validate(&forged)` still Ok (lint-only by design) vs `submit_bundle` `UnknownTool`.
- Banned: no swarms/programmers invoked by this programmer (single phase brief per spawn).

### 5.4 Hashes (post-retry)

- `registry.rs` `32d575397ac11810763d7cf4f58708bb7015fe96e54131625095b86df68589d6`
- `bundles.rs` `3f6c55337e9dadc2fb7c7685967c798fdc038aedc30679703f9be6a7b8caf840`
- `tiers.rs` `ef02c639f3802ce037390b3fd4a8031de178f2d98fee75188e5b41c70d1131b7` (unchanged)
- `lens.rs` `42291cca1836ef363fafbed5ce0a4cd287a4ac1478244cd46f5b056be5bfa78c` (unchanged)
- `fixtures.rs` `2c5513ad3ec6ce2fbc57a08b57e3dde76ba2717613570610fa6db408a86fcc74`
- `toolmatch.rs` `18063228be0436da528f3b1c1d16c3a6bea831a2cfaaea3f29251c83806e4a2f` (unchanged)
- `worktree/mod.rs` `670244b77b7cca005e2c719de17a9abdedc5bae7bf534538b5e9a1acb3cffac2` (unchanged)
- `tests/tool_gateway.rs` `864d5d68768b947b31227282f9cb02482f7413a645dc809f0317f6e1e0e09df1`
- `mcp/mod.rs` `e35423361b495bfcf1d2939320a150b7ef991ae167b24efd784b8bf0e067b975` (unchanged)
- `mcp/pool.rs` `eeeb9464fab3ba3fcba4c2c2e760f963d3ecc6436f7754c6f743a2ee84a70ae4` (unchanged)
