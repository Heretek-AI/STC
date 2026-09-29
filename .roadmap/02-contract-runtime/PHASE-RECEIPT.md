# PHASE-RECEIPT — 02-contract-runtime (initial implementation)

**Program:** STC v2 greenfield rebuild · run `harvest-review-01` · branch `v2-greenfield` (work delivered on `main` working tree; manager merges)
**Phase brief:** `.roadmap/02-contract-runtime/GOAL.md` · parent dossier `.roadmap/02-contract-runtime/dossier.json` (verdict: pending — NOT modified, see guard note) · annex `.roadmap/02-contract-runtime/ANNEX-harvest-review-01.md` (sha256 `9a361408cc883d69d8f83d907d903d7f26614838aac6fcf2b57cb950974d65f5`) · gate dossier `.roadmap/harvest-review-01/dossier.json` (`7cac8a00e6cdb0dba39e230e3f47d1bb91558eebc835f5d4cbe3f0ba4d100e5e`)
**Goal:** moat mechanism as reusable runtime — immutable frozen candidate (lineage/revision/target hashes), exactly ONE bounded correction, burned acknowledge token required for delivery, non-escalating spawn policy, typed failures everywhere.

> Every claim below carries the command run and its exit code. Open questions and deferrals are under **Residual limitations**, not overclaimed.

## Phase evidence hashes cited (from parent dossier.json — all re-verified this run)

| Claim | sha256 | Pointer | Check this run |
|---|---|---|---|
| frontier (20 settled decisions) | `86b6c81b505caccdffed71d0ca5139047cf4bd693e838791ab306025a52a9af7` | `file://.research/frontier.json` | see §E4.2 correction note (file absent in-tree; live frontier `.factory/frontier.json`) |
| v1 verify module (port/rewrite candidate) | `c1e49e97f6429dec4604dbcf76e3e4a27fa577284c0e0a82254dddce1ac8c29f` | `file://studio-core/src/verify/mod.rs` | see §E4.3 revision (full hash `5d3f70fb…`, no caret syntax) |
| v1 roles/RolePack + SpawnPolicy | `a414bc61fb12a41c11fe188c806efdaec0719be2de13fb5ec2ecd1464d8731ab` | `file://studio-core/src/roles/mod.rs` | see §E4.3 revision (full hash `5d3f70fb…`, no caret syntax) |
| gate dossier | `7cac8a00e6cdb0dba39e230e3f47d1bb91558eebc835f5d4cbe3f0ba4d100e5e` | `file://.roadmap/harvest-review-01/dossier.json` | read-only; 9 P02 adoption rows extracted |

## Starting state (repro)

The tree held **partial, uncommitted phase-02 work** (untracked `studio-core/src/verify/{freeze,dod,lease,receipts,landing}.rs`, 1780 lines; dirty `studio-core/Cargo.toml`, `studio-core/src/state/mod.rs`, `Cargo.lock`) with **no `verify/mod.rs`, no `roles/`, no approvals/policy/acp modules, no lib.rs wiring** — i.e. it did not compile as a tree (`verify/` inert without `mod verify`). Per the never-destructive rule I adopted that work, repaired 5 compile defects in it (see FIXED below), and built the missing 9 adoptions + 3 vectors + gate runner around it. No swarms or other programmers invoked.

## Gate / test summary (commands actually run, final tree)

| Gate | Command | Exit | Result |
|---|---|---|---|
| tests (workspace) | `cargo test --workspace` | 0 | **148 passed, 0 failed** — lib 117 (phase-01 intact + 33 new unit) · e2e 6 · mutants 13 · replay 6 · spawn-matrix 3 · syntax_gate 1 · cli 2 |
| clippy | `cargo clippy --all-targets -p studio-core -- -D warnings` | 0 | 0 warnings, no `#[allow]` in source |
| format | `cargo fmt --check` | 0 | clean |
| prek | `prek run --all-files` | 1 (guard conflict, see note) | 9/10 hooks Passed; `end-of-file-fixer` fails ONLY for wanting to write guarded dossiers (reverted); rustfmt/clippy/gitleaks/fallow/CODEOWNERS all Passed |

File hashes (via `sha256sum`, this run):

| Artifact | sha256 | Provenance |
|---|---|---|
| `studio-core/src/verify/mod.rs` (NEW: gate-order runner) | `0aa69bbb624ae9911e99f86113ee17657d419f8d86d7633aa2e7abb5de7254cb` | written this phase; QA-R1 CorrectionSpent (retry) |
| `studio-core/src/verify/freeze.rs` | `12ecdfecb24f09aa95ae63d6808238cb024936d7dfb0859ffbe3e0a1e752a807` | adopted + repairs + claim_correction/task-bound ledger (retry) |
| `studio-core/src/verify/dod.rs` | `99a7ed31cc3013292c296e6fafe138a023f1dcbd143175fdcbc1e40b592bbf93` | adopted as-is |
| `studio-core/src/verify/lease.rs` | `4f97bd2cc2bae682911d927962dedee4d22956332837b9619d74dfd792c2335a` | adopted as-is |
| `studio-core/src/verify/receipts.rs` | `9b8811dc18f2b1bf1660412758988a8bdc8178c71833c95d2e0bff3ca938c697` | adopted as-is |
| `studio-core/src/verify/landing.rs` | `34ed1c89b045510a716df904121dda35d028d57f2ef379278cb9f67f5783bc54` | adopted + repair + next_transition + TaskMismatch (retry) |
| `studio-core/src/roles/mod.rs` (NEW) | `7150df3502b1404c1529dce632437425a159c4f98ef8dccc154bed9ddd6b52b7` | written this phase; QA-R2/R3 hardened (retry) |
| `studio-core/src/approvals/mod.rs` (NEW) | `a69da9dbfaec273f3efe62b40cec6efe9306349ab048589f91d8243ae762267a` | written this phase |
| `studio-core/src/policy/mod.rs` (NEW) | `69ce0ef51bb2a4125b268427ff4955c492fe587554c5d483c361f8092bdc3e99` | written this phase |
| `studio-core/src/acp.rs` (NEW) | `d254a6117a4d7835f6a4bb912e2a41a5c00679a67497b2989a480da0fadf6332` | written this phase |
| `studio-core/src/lib.rs` | `b2735d10ef3e0002f1d4039e59584f0a721b6f3eb8c6cdbb2d401c1e06a9e1b0` | +5 `pub mod` lines |
| `studio-core/src/state/mod.rs` | `afe243b1b71baff25c49abc5af92fee463a6b9fa9f7e83b345d28259abbdcc07` | adopted tables + correction_attempts + ack task_id (retry) |
| `studio-core/Cargo.toml` | `768a9968f8911f3b7d55d3305f390b411dbb76bc1782b080c5e7d8e3cc6151a9` | adopted (`acp-http-adapter =0.4.2`, tree-sitter→main) |
| `tests/contract_runtime_e2e.rs` (NEW) | `be692f2306d57e60241ab47b8a961ede4a0b99c399efd3c29e9388cbec669723` | acceptance coverage; QA-R1/R2/R4 runtime paths (retry) |
| `tests/replay_vectors.rs` (NEW, V1) | `8657942443a2ddc16f1d3c5911c9940ef431a277b3faf914b1184be932563886` | 5 traces × 20 + socket guard; budget-free T1/T2 (retry) |
| `tests/mutation_calibration.rs` (NEW, V2) | `b153ec9c22362e761aeaff90b00a25ea065bcbc5cf82e47255dc060da108b49b` | 12 mutants incl. TaskMismatch M4 leg (retry) |
| `tests/spawn_matrix.rs` (NEW, V5) | `b8be9ac2f231d8efd0ff22a973bdcebc4b805240e91673b3667c868bb39af281` | 270 cells, all intents call request (retry) |

## What was built (9 annex adoptions)

- **f02-vk-approvals (c001–c003):** `approvals::ApprovalService` — request/status/outcome protocol, single-shot decisions, typed `ApprovalError`. Clean-room code (no upstream copied); vendoring deferred per annex guardrail (no network in build, zero contamination risk).
- **f02-yylo-receipts (c004–c005):** `verify::receipts` — versioned execution envelope from provider observations only; release receipt mints solely from an exactly-binding owner authorization (adopted).
- **f02-yylo-lease-fence (c007):** `verify::lease::LeaseFence` — current-token-only mutating ops, monotonic rotation, stable refusal codes (adopted).
- **f02-yylo-merge-landing (c006):** `verify::landing` — QUEUED/CONFLICT/GIT_INTEGRATED, CAS ref updates, `BurnedToken` unconstructible without a burned row, append-only delivery receipt (adopted + `next_transition` pure function for V1-T5).
- **f02-yylo-risk-tiers (c012–c013):** `verify::freeze::risk_tier/validation_depth` — shape-based tiers incl. `Release`, bounded lens depth, full-suite flags (adopted).
- **f02-ic-approvals (c008):** durable `contract_approvals` + single-use scope-bound `capability_leases` (fingerprint-committed, CAS consume) in `approvals::{save/load/decide_stored/mint_lease/consume_lease}` (written).
- **f02-ic-runtime-policy (c009):** `policy::RuntimePolicy` — reduce-only `resolve`, explicit non-empty yolo ack, fail-closed `parse`, JSON audit round-trip (written).
- **f02-ohmpi-acp-gate (c010):** `acp::decide` — closed tool→class map, 4 exact permission options, destructive-intent escalation, unknown-anything fail-closed (written).
- **f02-acp-http-adapter (c011):** `acp-http-adapter =0.4.2` exact pin; `acp::AcpTransport` trait (object-safe boxed future); `RealAcpTransport` wraps `AdapterRuntime` (never spawns, never `run_server`); `FakeTransport` for tests. Lockfile impact noted in `Cargo.toml` (axum+tower+hyper+tokio-util tree, tokio 1.x shared, no conflicts).
- **Q9 core:** `verify::run_gate_order` (freeze→tree-proof→DoD+tree-sitter→burned-ack→CAS-land, `GateError` names the refusing gate); `roles::RolePack` + `SpawnPolicy::request→SpawnDenied` (v1 `mcp_manifest` dropped — v2 has no mcp module; returns with phase-03 pool).

## Compile defects found in adopted work (FIXED, minimal repairs)

1. `freeze.rs:443` stray `}` (block never opened) — deleted one brace.
2. `freeze.rs:88` `RiskTier::Low;` (semicolon → `()` return) — removed semicolon.
3. `freeze.rs:432`, `approvals:244,350`, `landing:262` `with_write` closures returned `Result<usize, rusqlite::Error>` instead of `Result<_, StateError>` — wrapped `Ok(conn.execute(...)?)`.
4. `landing.rs:16` unused import `FreezeError` — removed.
5. `verify/mod.rs` (mine): `lease::{require_fence as _, ...}` referenced a nonexistent symbol — corrected to real exports.
6. `verify/mod.rs` (mine): `GateOutcome` needed `Debug` for `unwrap_err` — derived.
7. `acp.rs` (mine): RPITIT trait method not object-safe for `&dyn` — boxed futures.
8. `freeze.rs:382` clippy `-D warnings`: `to_string_in_format_args` — `&...[..12]` inline (no `#[allow]`, per repo rule).

## Acceptance checklist with evidence paths

- [x] gate order executable end-to-end on a fixture task — `run_gate_order` (`verify/mod.rs`); `contract_runtime_e2e::gate_order_lands_fixture_task_end_to_end`, `verify::tests::gate_order_lands_green_fixture_end_to_end`
- [x] freeze immutable + reproducible — `freeze_candidate` content binding + lineage/revision; `contract_runtime_e2e::freeze_is_immutable_and_reproducible`, `freeze::tests::freeze_is_deterministic_sorted_and_content_bound`
- [x] second correction rejected typed — runtime DB-backed per-freeze bound, `GateError::CorrectionSpent`; `contract_runtime_e2e::second_correction_is_rejected_by_the_runtime_typed` (block→spent→land→no-double-delivery, zero budget objects), V1-T2 20× (retry R1)
- [x] land denied without burned token — `LandingError::{AckNotBurned,NoBurnedAck,TaskMismatch}`; `contract_runtime_e2e::land_without_burned_token_is_denied` + `one_ack_lands_exactly_one_delivery_same_freeze`, V1-T3 20× (retry R4)
- [x] spawn escalation rejected typed — `SpawnDenied{scope,allowed}` via bound `request_spawn` (+`<empty-child>` denial, forged-basis contrast); `contract_runtime_e2e::spawn_escalation_is_rejected_typed`, V1-T4 20×, V5 270 cells all-exercising-request (0 false denials, 0 false allows) (retry R2)
- [x] tests cover all four — this file + `replay_vectors` (V1: 5×20 deterministic + `/proc/self/fd` no-socket guard) + `mutation_calibration` (V2: 12/12 typed vs ≥11/12 bar: 8 P02 + 4 P01) + `spawn_matrix` (V5)

## Guard note (do NOT modify GOAL.md / dossier.json — honoured)

`git status` shows zero modifications under `.roadmap/` tracked files (verified post-run, after reverting the hook below).

**prek record (corrected):** 9/10 hooks pass. `end-of-file-fixer` FAILS (exit 1) for one reason only: it appends trailing newlines to the guarded dossiers (6 dossiers + hash-manifest were committed without them). The earlier "exit 0" readings in this receipt were pipeline-masked (`| grep`/`| tail` exit codes, not prek's). Letting the hook write would violate the do-not-touch-dossier order, so I revert its 7 files with `git checkout -- .roadmap/` after every run — which deterministically re-arms the failure. Manager decision required: bless the whitespace fix (one newline per dossier) or waive the hook for `.roadmap/*/dossier.json`. All other hooks (yaml, large-files, merge-conflict, rustfmt, clippy, gitleaks, fallow, CODEOWNERS-gate) pass.

## Residual limitations (not overclaimed)

1. **DAG next-transition:** the goal's "deterministic next-transition for DAG nodes" is pinned only for the delivery transition (`next_transition`, V1-T5). No DAG module exists in the v2 tree; full DAG composition arrives with its phase.
2. **vk vendoring deferred:** f02-vk-approvals is clean-room protocol code, not a vendored file, per the annex "trim ts-rs if cockpit codegen lands later" guardrail. If the manager wants the upstream file vendored, that needs the BloopAI/vibe-kanban source (not in `/tmp/opencode/harvest-clones`, which holds only fastmcp + yylo) plus a ts-rs decision.
3. **V1 tokio-socket footnote:** a fresh tokio runtime opens 3 driver sockets at construction on this kernel; the no-socket guard baselines AFTER runtime setup, so it measures exactly what the replay opens (zero). Documented in-test.
4. **M12 calibration union:** the garbage-file mutant asserts `Sqlite(_) | NotV2{..}` (both typed fail-closed; exact variant depends on how far SQLite gets before refusing). All other 11 mutants pin exact variants.
5. **Branch:** work was delivered in the `main` working tree (where the partial work lived; switching branches under a dirty parallel tree risked loss). The `v2-greenfield` merge is the manager's call. Pre-existing dirt (`plans/*`, `scripts/stc_supervisor.py`, `--help*`, `cand.py`, `skills/`) was left untouched.

---

## QA ROUND 1 RETRY (status `retrying`, retries=1/3) — bounded fix list, proofs

Manager tiebreak SUSTAINED qa-b's FAIL (4 runtime evasions) over qa-a's conditional. All fixes below are runtime-enforced (never caller-honest) with regression tests that fail pre-fix.

### R1 — CorrectionBudget enforced by the runtime (was caller-held)

**Evasion reproduced:** `run_gate_order` took no budget; a fresh `CorrectionBudget` per attempt (or none) bought infinite corrections for the same frozen candidate.
**Fix:** `correction_attempts(freeze_hash PK, attempts)` table (`state/mod.rs`, SQL `--` comments — a first attempt with Rust `///` inside the SQL string broke every DB test; caught and fixed); `freeze::claim_correction` check-and-increments inside one `BEGIN IMMEDIATE` txn (`CORRECTION_MAX_ATTEMPTS=1`); `run_gate_order` claims on every DoD-block and returns new `GateError::CorrectionSpent{freeze_hash}` when spent. `CorrectionBudget` remains as advisory pre-flight only (docs say so).
**Proof (no budget object anywhere in the path):** `contract_runtime_e2e::second_correction_is_rejected_by_the_runtime_typed` — red→`DodBlocked`, red-again-same-hash→`CorrectionSpent`, green→lands, green-retry→`Landing(DuplicateReceipt)`; `grep CorrectionBudget tests/contract_runtime_e2e.rs` hits only a doc comment (no import, no object). Unit: `freeze::claim_correction_is_per_freeze_and_survives_fresh_objects`, `verify::runtime_bounds_corrections_without_caller_budget`; V1-T2 rerun as runtime path 20×.

### R2 — Spawn basis bound + empty-child denied + matrix honesty

**Fix:** `RolePack::request_spawn` binds the parent basis to declared `tools` (the raw `SpawnPolicy::request` primitive documents that it trusts its basis); empty child list → `SpawnDenied{scope:"<empty-child>"}` (was vacuous `Ok`).
**Proof:** `roles::empty_child_spawn_is_denied_not_vacuously_allowed`, `roles::forged_parent_basis_cannot_escalate_through_pack` (dispatcher-shape: forged basis makes the primitive allow `sast`, pack denies), e2e AC5 forged contrast + empty-child, V1-T4 bound path 20×. **Matrix:** intents relabeled to three spawn shapes (`PackPolicy`/`IsolatedBaseline`/`ForgedBasis`) — all 270 cells call `request` (was 90/270); `forged_basis_never_beats_the_bound_pack` cross-checks binding per cell; still 0 false denials + 0 false allows.

### R3 — Contains-based secret scan

**Fix:** `reject_raw_secrets` scans full text with trailing-run length thresholds (`sk-`+≥16, `AKIA`+≥16, any `xoxb-`/`xoxp-`/`ghp_`) instead of whitespace tokens.
**Proof:** `roles::secret_scan_catches_embedded_prefixed_and_json_bypass` — `key=sk-…`, JSON-embedded, and prefixed secrets all refuse; `desk-review`/`flask-app`/airport-code prose passes; placeholders pass.

### R4 — Task-bound single-delivery tokens

**Evasion reproduced:** `land` checked only freeze equality; one ack landed N deliveries across tasks.
**Fix:** `ack_tokens.task_id` (CREATE + pragma-gated `ALTER` for pre-existing DBs); `issue_token`/`AckLedger::acknowledge` take `task_id`; `BurnedToken::{from_store,from_ledger}` require it and return new `LandingError::TaskMismatch` on cross-task presentation; `land` re-checks binding.
**Proof:** e2e `one_ack_lands_exactly_one_delivery_same_freeze` (t1+t2 same freeze each land on own token; cross-presentation → `TaskMismatch`, record stays `Queued`), `landing::one_ack_lands_exactly_one_delivery`, M4 TaskMismatch leg, V1-T3 cross-task leg 20×.

### E4 — Receipt corrections (this section)

1. **Test count 138 → 148** (initial receipt undercounted: 117 lib + 6 e2e + 13 mutants + 6 replay + 3 matrix + 1 syntax_gate + 2 crash_recovery; the missing 2 were `studio-cli/tests/crash_recovery.rs`). Corrected above.
2. **Frontier claim:** parent dossier asserts `86b6c81b…` @ `file://.research/frontier.json` (VERIFIED_HASH — the dossier author's claim, guarded, NOT altered). Footnote: that file is ABSENT in this tree (`ls .research/frontier.json` → No such file); the live frontier in this tree is `.factory/frontier.json`, sha256 `5bf2863f2c574d5b84d2f9e02bd5f0bf15112c62cdd6f38eeae03103b237b6ed` (`sha256sum`, this run). I do not re-assert the dossier's hash against a file I cannot see.
3. **v1 citation revision:** `622e6b6^` resolves to `5d3f70fb07d06f84191354be52c15cf65d08661c` (verified `git log --format='%H %P' -1 622e6b6`; `622e6b6` is contained in main/v2-greenfield/origin). Reproducible commands without caret syntax: `git show 5d3f70fb07d06f84191354be52c15cf65d08661c:studio-core/src/verify/mod.rs | sha256sum` → `c1e49e97…`; same for `src/roles/mod.rs` → `a414bc61…` (both re-run this retry, match).

### Retry gate summary (final tree, commands actually run)

| Gate | Command | Exit | Result |
|---|---|---|---|
| tests (workspace) | `cargo test --workspace` | 0 | **148 passed, 0 failed** |
| clippy | `cargo clippy --all-targets -p studio-core -- -D warnings` | 0 | 0 warnings (incl. 2 `cloned-ref-to-slice-refs` fixed via `vec!` binding, no `#[allow]`) |
| format | `cargo fmt --check` | 0 | clean |
| prek | `prek run --all-files` | 1 (guard conflict, see note) | 9/10 Passed; end-of-file-fixer fails solely on guarded dossiers (reverted); all code hooks Passed |

Retry residuals: M12 still a two-variant typed union (unchanged); `CorrectionBudget` struct retained as advisory (documented, DB is authority); `.roadmap/h1-02-contract/` appeared (not mine, untouched).
