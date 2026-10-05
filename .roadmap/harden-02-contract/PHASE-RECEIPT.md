# PHASE-RECEIPT — harden-02-contract (hardening-only, no new features)

**Program:** STC v2 greenfield rebuild · run `harvest-harden-01` · phase `harden-02-contract`
**Phase brief:** `.roadmap/harden-02-contract/phase.json` (goal: Harden h1-02 residuals: M12 exact-typed narrowing, CorrectionBudget removal/trap, DAG cage, receipt guard (V02-A/B/C/D + H1-H4 clean-room))
**Mode:** hardening-only. No new features, no new network deps, no mobile/cloud/marketplace/voice. No swarms or other programmers invoked (single programmer implementation).

> Every claim below carries the command run and its exit code. Open questions and deferrals are under **Residual limitations**, not overclaimed.

## Phase evidence hashes cited (from phase brief — all re-verified this run)

| Claim | sha256 / REPORT | Pointer | Check this run |
|---|---|---|---|
| frontier (harvest-harden-01, Harden residuals 02 then 03, cycle 1/5 approved) | run `harvest-harden-01` | `file://.factory/harvest-harden-01/state.json` | `cat` confirms `harden-02-contract: briefed` (this run) |
| brainstorm fallback REPORT (V02-A M12 discriminant, V02-B CorrectionBudget trap/removal, V02-C DAG cage, V02-D receipt guard) | `ses_ef74bea89ffey39YDGFjUC2bOI` | session REPORT | vectors implemented §1–§4 |
| darkharvest fallback REPORT (H1-H4 clean-room, SPDX MIT/Apache-2.0) | `ses_ef74accf9ffeK2IsbsWDDibSsF` | session REPORT | no GPL/AGPL copy (§5, grep exit 1) |
| anchor `studio-core/src/state/mod.rs` (pre-phase) | `6428fabc3f97768b147aac5c3eedf25d449113363443407d7108adee7360c20d` | `file://studio-core/src/state/mod.rs` | `sha256sum` pre-edit matched exactly (see §E) |
| anchor `studio-core/src/verify/freeze.rs` (pre-phase) | `12ecdfecb24f09aa95ae63d6808238cb024936d7dfb0859ffbe3e0a1e752a807` | `file://studio-core/src/verify/freeze.rs` | `sha256sum` pre-edit matched exactly |
| anchor `studio-core/src/verify/landing.rs` (pre-phase) | `34ed1c89b045510a716df904121dda35d028d57f2ef379278cb9f67f5783bc54` | `file://studio-core/src/verify/landing.rs` | `sha256sum` pre-edit matched exactly |
| anchor `studio-core/src/verify/mod.rs` (pre-phase) | `0aa69bbb624ae9911e99f86113ee17657d419f8d86d7633aa2e7abb5de7254cb` | `file://studio-core/src/verify/mod.rs` | `sha256sum` pre-edit matched exactly |
| anchor PHASE-RECEIPT 02 | `44daa0ca810dd5834dffadfe8f88a1f994e645fe426d0578f5e8b5279fa69994` | `file://.roadmap/02-contract-runtime/PHASE-RECEIPT.md` | `sha256sum` this run matched exactly |
| anchor `mutation_calibration.rs` (pre-phase) | `b153ec9c22362e761aeaff90b00a25ea065bcbc5cf82e47255dc060da108b49b` | `file://studio-core/tests/mutation_calibration.rs` | `sha256sum` pre-edit matched exactly |
| prior gate dossier | `7cac8a00e6cdb0dba39e230e3f47d1bb91558eebc835f5d4cbe3f0ba4d100e5e` | `file://.roadmap/harvest-review-01/dossier.json` | read-only, not modified (guard §4) |
| darkharvest dossier | `17f5ceaaa0a46b78060d402abc2a3087a7d08cf3460604ca6969655682e076e6` | dossier | read-only, not modified |
| brainstorm dossier | `9ed6ddfab988dfe7d69b0a2eaec7fbb1f854060b6bd48004b963c73f88691773` | dossier | read-only, not modified |

## Gate / test summary (commands actually run, final tree)

| Gate | Command | Exit | Result |
|---|---|---|---|
| tests (workspace) | `cargo test --workspace` | 0 | **274 passed, 0 failed** — lib 194 (192 prior + 3 new spike − 1 removed) · e2e 6 · mutants 13 · replay 6 · rolepack 4 · spawn-matrix 3 · syntax_gate 1 · tool_gateway 10 · cli crash 2 · web lib 14 · web_contract 18 |
| clippy | `cargo clippy --all-targets -p studio-core -- -D warnings` | 0 | 0 warnings, no `#[allow]` in source |
| format | `cargo fmt --check` | 0 | clean |
| constructor grep gate | `grep -rn "CorrectionBudget::new" studio-core/src/` | 1 (no hits = pass) | zero occurrences (V02-B) |
| bare-type grep | `grep -rn "CorrectionBudget" studio-core/src/` | 1 (no hits = pass) | full removal, zero bare mentions |
| license grep | `grep -rniE 'general public license\|affero\|AGPL\|GPL-3\|GPLv3' studio-core/src` | 1 (no hits = pass) | no GPL/AGPL copy (H1-H4 clean-room MIT/Apache-2.0) |
| receipt guard | `scripts/guard-receipt-manifest.sh` | 0 | PASS, elapsed <15000ms (see §4) |

File hashes (via `sha256sum`, post-phase final tree):

| Artifact | sha256 | Provenance |
|---|---|---|
| `studio-core/src/state/mod.rs` | `5d63b2e6bfae68dfaee4e043d1012ae4fbe631b0a79376c7fe8464cf40f046f9` | + `SQLITE_MAGIC` discriminant + `garbage_discriminant` + M12 corpus test (V02-A/H1) |
| `studio-core/src/verify/freeze.rs` | `265006ea3b2aaaac89e81529b36408fb64be41095ddc749f58e35b78163a3ca1` | − removed budget type, + constructor-absent grep-gate test (V02-B/H2) |
| `studio-core/src/verify/landing.rs` | `f9b17efe6e0b156e2b6afc6f726f7ce91c2c24a7a8a2883073960d1fadd44bda` | + DAG cage docs + 20-node replay test (V02-C/H3) |
| `studio-core/src/verify/mod.rs` | `532d8d487b71abaf3da1e6f63c115b1317379e2783fadbc6701e7402849c9286` | − removed export, docs → runtime-DB authority (V02-B) |
| `studio-core/tests/mutation_calibration.rs` | `acaf931e9d2b1d2df6d57ba95ff5dac64eb76c7d5bf11415e642c4ed67df8e1a` | M2 → DB path, M12 → exact `NotV2{found:0}` |
| `scripts/guard-receipt-manifest.sh` (NEW) | `5a9616f6aa97e02349520db2f3a0ed4ed957de250027fa9f2df291981235c206` | receipt hash-manifest guard, <15s (V02-D/H4) |

## What was hardened (4 residuals, no new features)

### 1. V02-A/H1 — M12 exact-typed narrowing (magic-byte discriminant corpus)

**Was:** `m12_garbage_file_never_becomes_a_store` asserted a two-variant union `Sqlite(_) | NotV2{..}` (both typed fail-closed; exact variant depended on how far SQLite got before refusing).

**Now:** `state/mod.rs` adds `SQLITE_MAGIC = b"SQLite format 3\0"` (verified `head -c16 studio.db` = `SQLite format 3\0` this run) + `garbage_discriminant(path)` pre-check in `StateStore::open`: existing non-empty file lacking the header → deterministic `NotV2{found:0}` BEFORE SQLite runs. Empty/missing files fall through (fresh-DB creation preserved); header-spoofed corrupt files pass the pre-check and fail later as `Sqlite` (TOCTOU guard documented on the constant and in `open`).

**Proof (kill-gated spike):** `state::tests::m12_discriminant_corpus_6_inputs_x50_runs_exact_typed` — 6 inputs × 50 runs = 300 opens: (a) `not a database` → exact `NotV2{found:0}`; (b) empty → `Ok`; (c) truncated `SQLite format` → exact `NotV2`; (d) 1 KiB non-header → exact `NotV2`; (e) valid DB → `Ok` ×50; (f) header+1 KiB garbage → `Sqlite` (union load-bearing for spoof/race). FAILS pre-fix (garbage was `Sqlite`, not exact `NotV2`); PASSES post-fix. M12 mutant itself narrowed to `matches!(err, NotV2{found:0})`.

### 2. V02-B/H2 — CorrectionBudget removal + grep gate (fresh-object test stays green)

**Was:** `CorrectionBudget::{new,request,spent}` in-memory advisory type retained alongside the DB authority (`correction_attempts` + `claim_correction`); M2 tested the in-memory object, so deleting caller plumbing was unproven.

**Now:** type REMOVED entirely (`freeze.rs` struct+impls+`Default` deleted; `verify/mod.rs` export + docs rewritten to runtime-DB authority; SQL comment in `state/mod.rs` reworded). `grep -rn "CorrectionBudget::new" studio-core/src/` → exit 1 (zero hits); `grep -rn "CorrectionBudget" studio-core/src/` → exit 1 (zero bare mentions — full removal).

**Proof (kill-gated spike):** `freeze::tests::removed_budget_constructor_absent_from_runtime_src` — walks `studio-core/src` at test time, asserts zero contiguous constructor occurrences (needle assembled as `["Correction","Budget","::","new"].concat()` so the test's own source never self-matches). FAILS pre-fix (constructor existed); PASSES post-fix. **Fresh-object authority stays green:** `freeze::claim_correction_is_per_freeze_and_survives_fresh_objects`, `verify::runtime_bounds_corrections_without_caller_budget`, `contract_runtime_e2e::second_correction_is_rejected_by_the_runtime_typed`, and rewritten M2 (`claim_correction` twice same hash → `BudgetSpent`, other hash unaffected).

### 3. V02-C/H3 — DAG next-transition cage (pure over DeliveryRecord only + deferred note)

**Was:** `next_transition` documented as "pure … (DAG composes it in a later phase)" — correct but uncaged (no purity pins, DAG scope ambiguous).

**Now:** docs rewritten as an explicit CAGE (total function of `record.state` only; no I/O/state/clock/mutation; takes `&`) + `deferred:dag-phase` note (multi-node scheduling does NOT exist here and is NOT implied; this is the single-node step the DAG will compose later).

**Proof (kill-gated spike):** `landing::next_transition_cage_20_node_replay_is_pure_and_deterministic` — 20 nodes cycling all 3 states (7 Queued + 7 Conflict + 6 Integrated), 50 full replays in reverse + forward order: every call maps `Queued→Land/Conflict→Requeue/GitIntegrated→Terminal`, records observably unchanged, order-independent, count `(7,7,6)` pinned. FAILS on any hidden impurity/mutation/wrong mapping; PASSES post-fix.

### 4. V02-D/H4 — receipt hash-manifest guard script (<15s, no false positives)

**Was:** prek `end-of-file-fixer` conflicted with byte-pinned dossiers (revert-after-run discipline, manager decision pending).

**Now:** `scripts/guard-receipt-manifest.sh` (bash+grep+git only, no network): gate 1 refuses staged edits to `.roadmap/*/dossier.json|GOAL.md|.hash-manifest.json` without `STC_WAIVER=1` (empty staged set → pass, no false positives on untouched/guarded dossiers); gate 2 requires this receipt to cite all 6 phase hashes (exact `grep -qF` per hash, only the receipt scanned); gate 3 refuses GPL/AGPL markers in `studio-core/src` (H1-H4 clean-room). Prints elapsed ms to prove the 15s prek budget.

**Proof:** `time scripts/guard-receipt-manifest.sh` → exit 0, elapsed ~10–80ms (budget 15000ms); `STC_WAIVER=1` path skips only gate 1 with an explicit waiver note; unmodified tree passes with no dossier false positives.

## Acceptance checklist with evidence paths

- [x] `cargo test --workspace` green (exit 0, 274/274) — all prior tests stay green (192→194 lib: −1 removed in-memory test, +3 spike tests; every other suite count unchanged)
- [x] `cargo clippy --all-targets -p studio-core -- -D warnings` clean (exit 0, fixed `iter_cloned_collect` → `to_vec` + doc-comment blank line, no `#[allow]`)
- [x] `cargo fmt --check` clean (exit 0)
- [x] new spike tests kill-gated (each documents FAILS-pre-fix / PASSES-post-fix + falsifies clause in its doc comment)
- [x] no GPL/AGPL copy (`grep -rniE` exit 1; H1-H4 clean-room, SPDX MIT/Apache-2.0 per darkharvest REPORT `ses_ef74accf9ffeK2IsbsWDDibSsF`)
- [x] no new network deps (`git diff -- studio-core/Cargo.toml Cargo.lock` empty; `tree-sitter`/`acp-http-adapter` pins untouched)
- [x] no mobile/cloud/marketplace/voice (no such paths touched; `git status` shows only the 6 files above + this receipt)
- [x] hardening-only (no behavior widened: M12 narrowed, budget removed, DAG caged, guard additive)

## Guard note (dossiers untouched)

`git status` shows zero modifications under `.roadmap/*/dossier.json`, `.roadmap/*/GOAL.md`, `.roadmap/.hash-manifest.json`. The guard script (gate 1) enforces this in prek time; this receipt is the only `.roadmap` write (new file `harden-02-contract/PHASE-RECEIPT.md`, not a guarded dossier).

## Residual limitations (not overclaimed)

1. **M12 union survives for the race:** header-spoofed corrupt files and files swapped between the discriminant and `Connection::open` still surface `Sqlite` (fail-closed, typed). Total gates must accept `Sqlite(_) | NotV2{..}`; only steady-state garbage is narrowed.
2. **Empty file is a fresh DB, not garbage:** the discriminant deliberately returns `None` for 0-byte files so `open` creates the schema. An operator wanting empty-file refusal must add it as a separate policy (not this phase).
3. **DAG still deferred:** `next_transition` is caged single-node purity; no scheduler, edges, or persistence were added. Full DAG composition arrives with its phase (`deferred:dag-phase`).
4. **Prek `end-of-file-fixer` standing waiver unchanged:** the guard script does not modify `.pre-commit-config.yaml` (protected by `gate-config-integrity`); the whitespace waiver for `.roadmap/` remains as documented in phase 02.

## Commands run (exit codes)

- `sha256sum studio-core/src/state/mod.rs studio-core/src/verify/freeze.rs studio-core/src/verify/landing.rs studio-core/src/verify/mod.rs` → 0 (matched `6428fabc…`, `12ecdfec…`, `34ed1c89…`, `0aa69bbb…` pre-edit)
- `sha256sum studio-core/tests/mutation_calibration.rs` → 0 (matched `b153ec9c…` pre-edit)
- `sha256sum .roadmap/02-contract-runtime/PHASE-RECEIPT.md` → 0 (matched `44daa0ca…`)
- `cargo test -p studio-core --lib` → 0 (194 passed; intermediate 1 failed on `needle.len()` 22→21, fixed, re-ran → 0)
- `cargo test --workspace` → 0 (274 passed, 0 failed)
- `cargo clippy --all-targets -p studio-core -- -D warnings` → 0 after 2 fixes (doc blank line, `to_vec`); pre-fix clippy failed with 2 errors (documented, not hidden)
- `cargo fmt --check` → 0
- `grep -rn "CorrectionBudget::new" studio-core/src/` → 1 (zero hits, pass)
- `grep -rn "CorrectionBudget" studio-core/src/` → 1 (zero hits, pass)
- `grep -rniE 'general public license|affero|AGPL|GPL-3|GPLv3' studio-core/src` → 1 (zero hits, pass)
- `scripts/guard-receipt-manifest.sh` → 0 (PASS, elapsed <15000ms)
- `git diff --cached --name-only` → 0 (no guarded dossiers staged)

## Retry 1 — qa-b conditional, disclosure-only (no enforcement change)

**Scope:** bounded disclosure-only fix list per qa-b `conditional`
(`Conditional: foreign-valid-sqlite Ok-arm undocumented + read-only variant
split; all gates green, repros in /tmp/opencode/qa-b-harden02`). No logic
changed: `git diff` of the retry touches doc-comments only
(`state/mod.rs` module + `SQLITE_MAGIC` + `garbage_discriminant` +
`open` + `open_readonly` docs; `freeze.rs` `claim_correction` docs).
`landing.rs` / `verify/mod.rs` / `mutation_calibration.rs` /
`guard-receipt-manifest.sh` byte-identical (hashes below). No swarms or
other programmers invoked (single programmer retry).

**Pre-retry anchors re-verified this run** (`sha256sum` exit 0 before edit):

| Artifact (pre-retry) | sha256 | Status |
|---|---|---|
| `studio-core/src/state/mod.rs` | `5d63b2e6bfae68dfaee4e043d1012ae4fbe631b0a79376c7fe8464cf40f046f9` | matched prior post-phase hash |
| `studio-core/src/verify/freeze.rs` | `265006ea3b2aaaac89e81529b36408fb64be41095ddc749f58e35b78163a3ca1` | matched prior post-phase hash |
| `studio-core/src/verify/landing.rs` | `f9b17efe6e0b156e2b6afc6f726f7ce91c2c24a7a8a2883073960d1fadd44bda` | matched prior post-phase hash, unchanged by this retry |

**Post-retry hashes** (`sha256sum` final tree):

| Artifact | sha256 | Provenance |
|---|---|---|
| `studio-core/src/state/mod.rs` | `8498391238c3d7e6a861beba47727b61a349792fb883f8b66778af0307b7ee3b` | docs-only: third Ok-arm + readonly split + advisory-vs-gating + FIFO limit |
| `studio-core/src/verify/freeze.rs` | `2ed706d23a3aa32d47e7d50ee95f9273427a1c6ca78eb0fca53c5f787af597eb` | docs-only: `fresh budget object` → `fresh caller state` + discriminant advisory-vs-gating |
| `studio-core/src/verify/landing.rs` | `f9b17efe6e0b156e2b6afc6f726f7ce91c2c24a7a8a2883073960d1fadd44bda` | unchanged (no wording touched) |
| `studio-core/src/verify/mod.rs` | `532d8d487b71abaf3da1e6f63c115b1317379e2783fadbc6701e7402849c9286` | unchanged |
| `studio-core/tests/mutation_calibration.rs` | `acaf931e9d2b1d2df6d57ba95ff5dac64eb76c7d5bf11415e642c4ed67df8e1a` | unchanged |
| `scripts/guard-receipt-manifest.sh` | `5a9616f6aa97e02349520db2f3a0ed4ed957de250027fa9f2df291981235c206` | unchanged |
| `.roadmap/02-contract-runtime/PHASE-RECEIPT.md` | `44daa0ca810dd5834dffadfe8f88a1f994e645fe426d0578f5e8b5279fa69994` | unchanged, re-verified |

### Fix 1 — third Ok-arm documented (`open` + module docs)

`open()` no longer claims `fail closed if the existing file is not exactly
v2`. It now discloses three arms: (a) no-magic non-empty → early
`NotV2{found:0}`; (b) magic-present + well-formed SQLite → `Ok` adoption
(`init_schema` heals via `IF NOT EXISTS`, then `check_version` passes);
(c) version mismatch / corrupt body → typed fail-closed. Repro
`/tmp/opencode/qa-b-harden02/other-schema.db` (valid SQLite,
`schema_version = 2`, extra table `foo` per
`sqlite3 … "SELECT sql FROM sqlite_master WHERE name='foo'"` →
`CREATE TABLE foo(x)`) opens `Ok` via the probe
(`qab-probe other-schema.db => Ok [pred: header-present→None]`), while
`garbage100.db => NotV2{found:0}` and `spoof.db => Sqlite(NotADatabase)`
pin the other two arms. Module docs carry the same one-line adoption
disclosure superseding the old shorthand.

### Fix 2 — `open_readonly` split disclosed (exactness is per-path)

`open_readonly` docs now state the split: no `garbage_discriminant`
pre-check and no heal on that path, so the same garbage file surfaces
`Sqlite(NotADatabase)` via `open_readonly` where `open` returns early
`NotV2{found:0}`. Evidence: `readonly` probe →
`open_readonly(garbage) => Sqlite(SqliteFailure(NotADatabase))` vs
`qab-probe garbage100.db => NotV2{found:0}`; `open_readonly(valid) => Ok`.
`SQLITE_MAGIC` docs carry the matching `Exactness is per-path` paragraph.
Version gating still fail-closed via `check_version` on both paths.

### Fix 3 — `freeze.rs` wording + advisory-vs-gating

`claim_correction` docs: `even if the caller passes a fresh budget object
or none at all` → `even with fresh caller state or none at all (no budget
object exists to pass; the removed type has zero occurrences in
studio-core/src)`. Proof: `grep -rn "fresh budget object" studio-core/src/`
→ exit 1 (zero hits); `grep -rn "CorrectionBudget" studio-core/src/` →
exit 1 (removal intact). Added gating clarification: unlike
`garbage_discriminant` (advisory pre-read prediction in `state`), this
function GATES inside one `BEGIN IMMEDIATE` write txn (no TOCTOU split).
`state` docs carry the mirror half: discriminant is the advisory pre-read
prediction, `open` is the gate, both TOCTOU arms stay fail-closed (typed).

### Fix 4 — FIFO blocking limit (known limit, no file-type guard)

`SQLITE_MAGIC` + `garbage_discriminant` docs now note the known limit: the
pre-check does `File::open` + blocking read with no FIFO/socket/device
guard, so a FIFO path can block; callers must not point `open` at a FIFO
(or must isolate with a thread/timeout); a future phase may add a
file-type guard. No guard added here (disclosure-only per brief). Evidence:
code review — `garbage_discriminant` body is `metadata → File::open →
read` with no `file_type().is_fifo()` branch.

### Retry gates (commands actually run, final tree)

- `cargo test --workspace` → 0 (**274 passed, 0 failed**; per-suite:
  194 lib + 6 e2e + 13 mutants + 6 replay + 4 rolepack + 3 spawn-matrix +
  1 syntax_gate + 10 tool_gateway + 2 cli crash + 14 web lib + 18 web_contract)
- `cargo clippy --all-targets -p studio-core -- -D warnings` → 0 (0 warnings)
- `cargo fmt --check` → 0 (clean)
- `grep -rn "fresh budget object" studio-core/src/` → 1 (zero hits, pass)
- `grep -rn "CorrectionBudget" studio-core/src/` → 1 (zero hits, pass)
- `grep -rniE 'general public license|affero|AGPL|GPL-3|GPLv3' studio-core/src` → 1 (zero hits, pass)
- `scripts/guard-receipt-manifest.sh` → 0 (PASS, `hashes=6 elapsed=28ms (budget 15000ms)`)
- `qab-probe /tmp/opencode/qa-b-harden02/other-schema.db` → `Ok` (adoption arm repro)
- `qab-probe garbage100.db` → `NotV2{found:0}`; `spoof.db` → `Sqlite(NotADatabase)` (other arms pinned)
- `readonly` probe → `open_readonly(garbage) => Sqlite(NotADatabase)` (split repro)
- `sha256sum` pre-retry matched `5d63b2e6…`, `265006ea…`, `f9b17efe…`; post-retry `84983912…`, `2ed706d2…`, landing/mod/mutation/guard unchanged

### Residual limitations (retry deltas, not overclaimed)

1. FIFO limit stays a disclosed known limit (no guard added this retry).
2. Adoption arm heals only well-formed SQLite; corrupt/foreign-version files
   still refuse typed (`NotV2` / `NewerSchema` / `Sqlite`).
3. Read-only split is by design (`open`-only discriminant); unifying the
   arms would be an enforcement change, out of scope for this retry.
