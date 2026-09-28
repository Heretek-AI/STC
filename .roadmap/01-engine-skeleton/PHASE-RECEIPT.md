# PHASE-RECEIPT — 01-engine-skeleton (FOURTH WAIVER · FINAL rework)

**Program:** STC v2 greenfield rebuild · run `stc-greenfield` · branch `v2-greenfield`
**Frontier sha256 (as recorded in phase GOAL/dossier):** `f282054839f73a67dedef41dfea50abdca0ab336dd55d046118b405173caf002`
*(Citation correction 2026-09-28: the frontier was amended for the adapters/tui ruling; the earlier `86b6c81b505caccdffed71d0ca5139047cf4bd693e838791ab306025a52a9af7` is superseded — caught by QA-A O1 / QA-B F.)*
**Goal:** minimal durable core — Tokio daemon + greenfield SQLite v2 (WAL, single-writer) + `studio-cli` read projections. No UI.
**Rework trigger:** QA seat B (r6) found two CRITICALs in the rework-6 code: **A-DB-COPY-01** (`cp studio.db backup.db` clones `engine_meta`, so the copy + forged `creating` row + same repo/root reaps the victim; `init` on the copy keeps the cloned id) and **C-UNLOCK-TRAP-01** (refused forged rows linger with no CLI recourse; the documented `git worktree unlock`-then-`gc` remedy lets `gc` REAP the foreign worktree). The manager granted a **fourth waiver** (programmer invocation #7) with prescribed fixes. This receipt is that attempt.

> Every claim below carries the command I ran and its exit code. Where a vector could not be fully closed I say so under **Residual limitations** instead of overclaiming.

Scratch dir: `/tmp/opencode/fix7` (repros `dbcopy_repro.sh`, `unlocktrap_repro.sh`; post-fix outputs `dbcopy_post.txt`, `unlocktrap_post.txt`; storm log `storm50.txt`).
Pre-fix binary: same path `target/debug/studio`, built from the unmodified tree — the pre-fix repro outputs below were captured before rebuilding.
Fixed binary: `/home/john/.local/share/opencode/worktree/5da7ec/sunny-otter/target/debug/studio` sha256 `883fb42f134a6856ec637284280cb9df62bd0e7f02162703afb2761e2c332241` (via `sha256sum`).

---

## Gate / test summary (commands actually run, after fixes)

| Gate | Command | Exit | Result |
|---|---|---|---|
| format | `cargo fmt --check` | 0 | clean |
| fallow | `./scripts/gate.sh` → fallow step | 0 | `Findings: 0 (0 blocking)` |
| Semgrep | `./scripts/gate.sh` → semgrep step | 0 | `Ran 2 rules on 7 files: 0 findings` |
| clippy | `cargo clippy --all-targets -- -D warnings` | 0 | clean, no `#[allow]` |
| tests | `cargo test` | 0 | **65 passed, 0 failed** (62 lib + 2 crash integration + 1 tree-sitter) |
| gate driver | `./scripts/gate.sh` | 0 | prints `GATE ORDER: PASS` |
| crash storm | `./scripts/crash_storm.sh 50` | 0 | `CRASH-STORM: PASS … corrupt_rows=0 orphaned_worktrees=0` + **10** self-tests PASS |

Post-fix hashes (computed with `sha256sum` this run):

| Artifact | sha256 |
|---|---|
| `studio-core/src/worktree/mod.rs` | `670244b77b7cca005e2c719de17a9abdedc5bae7bf534538b5e9a1acb3cffac2` |
| `studio-core/src/recovery.rs` | `d84abb0fdd053502b6b101645e06bc3f77f2916dc989780e8f9f7d6c6bdec37a` |
| `studio-core/src/state/mod.rs` | `5573969b1c738d86d1e6f553daae8511f14a90357d7c3c0513d01c8f62a7c10e` |
| `studio-cli/src/main.rs` | `e20686c719096e9fe12659e1638b05934944364ba9585434d00df6b041d6e9c4` |
| `studio-cli/tests/crash_recovery.rs` (unchanged) | `8a3fef82e9359b8575ebe1a289cd86fc7e54c5814bbaf0341d53b6af2da1d861` |
| `scripts/crash_storm.sh` | `a633dddd06c792fcc5db1ca610fb81109f4556203b387a9a322df6e64dbb7f00` |
| `scripts/gate.sh` (unchanged) | `85e3534a22e48acb834420b3ca9ff1f6585a5072888ea175e72e11f9f0a0e32f` |
| `semgrep-rules/stc-guardian.yaml` (unchanged) | `4b2425c144b63e5da9c9fb6a740087d587c7eff5f453bcc66a8b2696b1c1dc4e` |

Dependency ruling honoured: no new dependencies. `grep cap-std|rustix` on all `Cargo.toml`s → none. `Cargo.toml`/`Cargo.lock` unchanged by this rework.

---

## BLOCKING defects (this rework)

### A-DB-COPY-01 — `cp` clones engine identity; copy + forged row reaps victim (CRITICAL) — FIXED

**Defect & root cause.** `engine_meta.engine_id` lived in the DB file with no binding to the file itself, so `cp studio.db backup.db` produced two engines sharing one `engine_id`. The copy plus a forged `creating` row (same repo/root) satisfied the B-EXEMPT-FORGE-01 exemption (live lock reason `studio:<cloned-id>` matched) and reaped the victim. `init` on the copy kept the cloned id.

**Repro reproduced (pre-fix binary).** `/tmp/opencode/fix7/dbcopy_repro.sh`: victim engine (`daemon --ticks 8`) owns locked `wt-0`; `cp` → `copy engine_id == victim engine_id` (`CLONED`); attacker surgery on the COPY only (`UPDATE worktrees SET state='creating'`); `gc --db copy` exit 1 with:
```
"reaped_creating":["…/wt/5a9cb6b5/wt-0"],"reaped_git_orphans":["…/wt/5a9cb6b5/wt-0"],…,"refused_locked":[]
victim_dir_exists=no  victim_content_exists=no
```
Victim destroyed from the copy — the defect exactly as QA-B described.

**Fix (prescribed: bind engine identity to file identity).**
1. `engine_meta` gains `engine_dev`/`engine_ino` (`state/mod.rs`: `live_file_identity` via `std::os::unix::fs::MetadataExt`; non-unix/none → keep id, documented residual).
2. `StateStore::open` calls `engine_id()` on EVERY writable open, and `engine_id()` itself enforces the rule inside its write transaction (stat outside, re-read inside so concurrent openers of one copy converge): stored pair matches live `(dev,ino)` → keep (normal restart); differs → REGENERATE id + store new pair (copy/restore/replace diverges on first open); no stored pair yet (pre-fix DBs) → keep id, bind live pair (one-time migration); unstatable (in-memory/non-unix) → keep id, bind nothing.
3. `init` on an existing DB applies the same rule (it goes through `open`): same file → keep; different file identity → regenerate. Verified explicitly: `cp a.db b.db; studio init --db b.db` → `DIVERGED`, then `studio init --db a.db` → `ORIG-STABLE`.
4. Edge cases handled + documented in code: hardlinks share the inode → shared id is CORRECT (pinned by test); fresh `init` mints id + identity; in-place overwrite preserving the inode (plain `cp` truncating onto the same path) is indistinguishable from the original → keeps the id, ACCEPTED residual, said plainly in the `engine_id` docs.

**Proof — copy divergence (post-fix binary).** Same repro script (`dbcopy_post.txt`), copy `gc` exit 1 with:
```
"engine_id":"20889faa…"(≠ victim 2993cd83…),"reaped_creating":[],"reaped_git_orphans":[],"refused_locked":["…/wt-0"],…,"swept_dirs":[]
victim_dir_exists=yes  victim_content_exists=yes  victim_dir_present=yes
orig id now: 2993cd83… (stable)  orig victim row: ready  orig gc exit=0 (clean)
```
The copy diverged on first open; its forged row mismatched the victim's `studio:<original-id>` lock reason → refused. Original engine untouched (same id, row still `ready`, own `gc` exit 0).

**Regression tests added.** `state::tests::{db_copy_diverges_engine_id_while_original_keeps_its_id, hardlink_shares_the_engine_id, missing_file_identity_migrates_without_changing_the_id}`; `recovery::tests::db_copy_with_forged_row_refuses_and_original_is_unaffected` (full copy attack at library level incl. lock-reason + `verify`-clean assertions). Storm self-test `dbcopy_self_test` (CLI-level: `copy gc exit 1 refused=1 reaped=0 victim_intact=1 orig_id_stable=1 orig_gc_clean=1`).

### C-UNLOCK-TRAP-01 — refused paths linger; unlock-then-gc reaps the foreign worktree (CRITICAL) — FIXED

**Defect & root cause (two linked).** (1) Refused forged rows lingered forever with no CLI recourse (only hand-`sqlite3 DELETE`). (2) The documented remedy (`git worktree unlock` then `gc`) re-armed auto-reap: the unlocked foreign worktree became an ordinary unlocked orphan and `gc` reaped it — operator-mediated destruction.

**Repro reproduced (pre-fix binary).** `/tmp/opencode/fix7/unlocktrap_repro.sh`: forged row → `gc1 exit=1 refused_locked=[victim]`; `git worktree unlock victim`; `gc2 exit=1` with:
```
"reaped_creating":["…/victim"],"reaped_git_orphans":["…/victim"],…,"refused_*":[]…
victim_intact=no (DESTROYED)  victim_dir=no (REAPED)
```
plus `studio release` → `exit=2 error: unrecognized subcommand 'release'`. Both halves of the defect exactly as QA-B described.

**Fix (prescribed).**
1. Persisted refusals per DB: new `refused_paths(path, reason, at_ms)` table (`SCHEMA_SQL`, idempotent — pre-existing DBs gain it on next open); `StateStore::{record_refused, refused_set, refused_reason, clear_refused}` (`state/mod.rs`). First refusal wins (`INSERT OR IGNORE`).
2. `recover` loads the set and threads it through BOTH destructive passes: the git pass (`WorktreeManager::recover`, new 5th arg) honours a persisted refusal BEFORE the own-abandoned exemption and the unlocked-orphan reap (control-char check stays first); the fd-anchored sweep (`sweep_untracked`, new 3rd arg) refuses unregistered-but-refused directories into the new `refused_persisted` class (a still-registered worktree keeps its live `refused_locked`/`refused_worktrees` classes). Every refusal is (re-)recorded at end of pass (symlinks need none — re-refused live every pass). A ledger-tracked LIVE path is still skipped silently via exact match, so recording a refusal can never wedge our own future worktrees at that path.
3. Explicit recourse: `studio release --db <db> <path>` drops the stored refusal AND a lingering `creating` ledger row for the path (the wedge; `ready` rows never touched) — no hand-sqlite needed. Help text states the responsibility boundary (releasing a foreign LIVE worktree then unlocking it lets `gc` reap it).
4. `gc --help` / module docs (`recovery.rs`, `main.rs`): the `git worktree unlock`-then-`gc` remedy is replaced by the release story (unlock alone never re-arms; `release` then `gc` is the ONLY refused→reaped path). `gc` JSON gains `refused_persisted`; `RecoveryReport::clean` includes it (still exit 1 when unclean).

**Proof — unlock-trap closed (post-fix binary).** Same repro (`unlocktrap_post.txt`): `gc1 exit=1 refused_locked=[victim]`; unlock; `gc2 exit=1` with:
```
"reaped_creating":[],"reaped_git_orphans":[],"refused_persisted":["…/victim"],"refused_worktrees":["…/victim"],…,"swept_dirs":[]
victim_intact=yes  victim_dir=yes
```
then `studio release` → `exit=0 {"released":true,"prior_reason":"locked","ledger_creating_row_removed":true,…}`; follow-up `gc exit=1 reaped_git_orphans=[victim]` (ordinary orphan now), next `gc exit=0 clean:true`. Unlock-then-gc refuses; release-then-gc converges.

**Regression tests added.** `recovery::tests::{unlock_after_refuse_still_refuses_and_victim_survives (unlock + 3 gc rounds), release_clears_refusal_so_an_unlocked_stale_path_reaps, refused_row_is_never_auto_reaped_across_gc_runs (5 rounds, row kept as evidence)}`; `state::tests::refused_paths_roundtrip_persist_and_release`. Storm self-test `unlock_release_self_test` (refuse → unlock-still-refused ×2 → release clears refusal + creating row → reaped).

---

## NON-BLOCKING (retained residuals)

### A5-GC-DOS-01 — symlink injected mid-sweep aborts `gc` (RECORDED RESIDUAL, not fixed)
Repeatable same-user liveness DoS on `gc` (exit 2), non-destructive. Unchanged; user's explicit non-blocking ruling.

### A6-FALLBACK-PARITY-01 — `openat2` fallback follows an intermediate root symlink (RECORDED RESIDUAL, not fixed)
Only on kernels without `openat2`; dead code on the gate target. Unchanged.

---

## No regression on the previously verified wins

All prior tests retained and green: `cargo test` → **65 passed, 0 failed** (was 57; +8 new: 4 state + 4 recovery). Named anchors still passing: forged-row refusal (`forged_creating_row_never_reaps_a_foreign_locked_worktree`, plus storm `forge_self_test`), own-reap exemption incl. same-DB restart (`recovery_reaps_our_own_locked_half_created_worktree`, `restart_with_the_same_db_still_reaps_our_own_half_create`), `engine_ids_are_stable_per_db_and_unique_across_dbs`, newline refusal + exact-match skip, all SEC-A fd-anchored sweep tests, LOCK-B-iii `second_engine_never_reaps…` + exemption-direction tests, WAL/C/F/G state tests, lock tests, `crash_recovery` integration (5-round SIGKILL + lock-keyed-to-resource). `./scripts/gate.sh` exit 0; `./scripts/crash_storm.sh 50` PASS (`corrupt_rows=0 orphaned_worktrees=0 failures=0`) with **10** self-tests (8 prior + `dbcopy` + `unlock-release`). Honesty note: the old per-vector shell proofs (toctou_a5, HON-C3 samplers, RO-STATUS/RO-VERIFY one-liners) were not re-run individually this spawn; their permanent regression tests + the storm's self-tests (same invariants end-to-end against the fixed binary) are green.

## New regression coverage added (this rework)
- `state::tests::{db_copy_diverges_engine_id_while_original_keeps_its_id, hardlink_shares_the_engine_id, missing_file_identity_migrates_without_changing_the_id, refused_paths_roundtrip_persist_and_release}`.
- `recovery::tests::{db_copy_with_forged_row_refuses_and_original_is_unaffected, unlock_after_refuse_still_refuses_and_victim_survives, release_clears_refusal_so_an_unlocked_stale_path_reaps, refused_row_is_never_auto_reaped_across_gc_runs}`.
- `scripts/crash_storm.sh` self-tests `dbcopy_self_test`, `unlock_release_self_test` (now 10 total).

## API note
- `engine_meta` gains `engine_dev`/`engine_ino` (idempotent, no version bump: schema stays v2, `schema_version` still `2`); `StateStore::open` now enforces `engine_id()` binding on every writable open (read-only `open_readonly` never mutates identity).
- `refused_paths` table (new, idempotent); `StateStore::{record_refused, refused_set, refused_reason, clear_refused}` (new).
- `WorktreeManager::recover` takes a 5th arg `persisted_refused: &HashSet<String>`; `WorktreeRecovery::refused_persisted` (new field).
- `RecoveryReport::refused_persisted` (new field, in `clean()`); `gc` JSON gains `refused_persisted`.
- CLI: `studio release --db <db> <PATH>` (new subcommand; JSON `released/prior_reason/ledger_creating_row_removed/warning`).

## Residual limitations (honest)
- **Stolen engine id.** A deliberate same-user attacker who reads our DB's `engine_id` and re-locks the victim with `studio:<stolen>` + forges a row CAN still authorize the exemption. Out of scope per the brief (they can already `rm -rf`); nothing local can stop same-user.
- **In-place overwrite keeps the id.** ~~Overwriting the same DB path while preserving the inode (plain `cp` truncating onto the path) is indistinguishable from the original and keeps the id — accepted per the brief (it IS the same file by every local check).~~ **CORRECTED 2026-09-28 (D-MIGRATION-01, QA-B): reality diverges — a plain `cp src.db dst.db` truncating onto dst's inode does NOT keep the id.** The stored pair travels with the source content and mismatches dst's live identity, so dst mints a fresh (third) id on next open — the safe direction. Only a byte-identical self-rewrite of a file's own content keeps the id. Proven this spawn (inode kept `yes`, source stable, `b1≠b0`, `b1≠a1`; `engine_id` rustdocs corrected to match).
- **Non-unix / unstatable files.** No `(dev,ino)` available → id kept, nothing bound; the lock-reason check still refuses cross-engine forged rows (documented in `live_file_identity`).
- **Old-reason orphans now persist.** Worktrees refused once stay refused until `studio release` — fail-closed by design (operator story in `gc --help`). Live ledger-tracked paths are unaffected (exact-match skip precedes persistence).
- **Release is operator responsibility.** `release` on a foreign LIVE worktree + unlock + `gc` destroys it; the help text says so. A still-locked foreign worktree re-refuses after `release` (live lock check), so release alone cannot destroy a locked worktree.
- **Mid-create registration window.** Unchanged from rework-6 (sub-millisecond; own half-creates covered via the reason-gated path).
- **Non-`-z` toolchains / non-UTF8 names / platform / A5 / A6 / depth / HON-C3 peak / E build.** Unchanged from rework-6.
- **`verify` does not display refusals.** `verify` is read-only and unchanged; a refused-but-registered foreign worktree still shows as `git_orphans`/`leftover_dirs` (unclean), and the lingering `creating` row shows as `creating_rows` until `release`. No silent clean.
- **Lock identity.** Inode replacement still admits a second engine (accepted); destruction via every demonstrated path is now closed independently of lock identity.

## Constraints honoured
No commit (`git status` shows only working-tree modifications). No swarms/other agents (single spawn, no sub-spawns). `.roadmap/` untouched except this receipt (the `dossier.json` modification predates this spawn — not mine, not edited by me). No new dependency (`hex`/`libc` pre-existing). No `#[allow]`/ignore suppressions. No `adapters/`/`tui/` reintroduced (no such dirs). All 8 pre-existing crash-storm self-tests preserved, plus `dbcopy_self_test` + `unlock_release_self_test`.

**Verdict: A-DB-COPY-01 and C-UNLOCK-TRAP-01 genuinely closed.** Pre-fix repros destroy (copy gc reaps victim with cloned id; unlock-then-gc reaps with exit 1 and no recourse); post-fix the copy diverges on first open and refuses (`refused_locked=[victim]`, victim intact, original id stable + own gc clean), unlock-then-gc still refuses across runs (`refused_persisted`, victim intact), and `studio release` clears refusal + lingering row with the tree converging to clean. Gate green, 65/65 tests, crash-storm 50 PASS with 10 self-tests. A5/A6 recorded as explicit residuals.

---

# D-MIGRATION-01 — regenerate, never keep, when no stored pair exists (FIFTH WAIVER · programmer invocation #8)

**Scope:** one defect, prescribed fix. Out-of-scope items untouched: nonce mechanism, porcelain parsers, `refused_paths` table, release command, sweep, lock, WAL, verify, status — none modified this spawn (`git status` shows only the two source files below plus this receipt; every other modification predates this spawn).

> Every claim below carries the command I ran and its exit code.

Scratch dir: `/tmp/opencode/qa-b8` (repro `dmig_repro.sh`; pre-fix output `dmig_prefix.txt`; post-fix output `dmig_post.txt`; test log `cargotest.txt`; hashes `hashes.txt`).
Pre-fix binary: `target/debug/studio` built from the unmodified tree — the pre-fix repro below was captured before rebuilding.
Fixed binary: `/home/john/.local/share/opencode/worktree/5da7ec/sunny-otter/target/debug/studio` sha256 `d68cff5971fa8f05abb8fb7010cbce3bab0ca703b4c92ad925df36570502f245` (via `sha256sum`, exit 0).

## Defect & root cause

`StateStore::engine_id`'s no-stored-pair arm kept the id and bound the live pair (one-time migration). A DB in pre-fix shape (`engine_id` present, no `engine_dev`/`engine_ino`) copied BEFORE the first post-fix open therefore kept-and-bound the SAME id in both files, and the forged-row kill chain replayed. The shape is also attacker-reachable with one `sqlite DELETE` of the pair — not just legacy.

**Repro reproduced (pre-fix binary).** `/tmp/opencode/qa-b8/dmig_repro.sh` (exit 0): victim engine (`daemon --ticks 8`) owns locked `wt-0`; pair stripped via `sqlite3 "$DBV" "DELETE FROM engine_meta WHERE key IN ('engine_dev','engine_ino');"` (pair keys left: 0); `cp` before any post-fix open; first post-fix opens keep `22e84013…` in BOTH files (`CLONED`); attacker surgery on the COPY only (`UPDATE worktrees SET state='creating'`); `gc --db copy` exit 1 with `"reaped_creating":["…/wt-0"],"reaped_git_orphans":["…/wt-0"]`, `victim_dir_exists=no victim_content_exists=no` — victim destroyed from the copy, defect exactly as briefed.

## Atomicity finding (as required by the brief)

**Premise CONFIRMED — `init` writes id+pair atomically, no change needed.** `init` is `StateStore::open` + `checkpoint` (`studio-cli/src/main.rs`), and `open` calls `engine_id()`, whose entire read-modify-write runs inside `with_write` = exactly one `BEGIN IMMEDIATE … COMMIT` transaction (`studio-core/src/state/mod.rs`, `with_write`), with rollback on any error. Every `engine_meta` arm (including the new regenerate arm) executes inside that single transaction, so a crash can never leave a half-written pair behind: "no pair" truly implies "created by an older binary" (or attacker surgery), never a torn write.

## Fix (exactly as prescribed, nothing more)

`studio-core/src/state/mod.rs`, `StateStore::engine_id`: the `(Some(id), _, _, _)` keep-and-bind arm is split. When the live file IS statable, absent/incomplete stored pair → **REGENERATE** a fresh id + store new id + live pair (merged with the existing copy-divergence arm — one `(Some(_), _, _, Some((ld,li)))` arm, same transaction); when unstatable → keep the id, bind nothing (unchanged). `engine_id` rustdocs updated to state the regenerate rule, the safety argument (pre-fix binaries never issued `studio:<id>` lock reasons, so nothing live references the discarded id), and the corrected overwrite claim.

## Proof — pre-fix-copy divergence + victim refusal (post-fix binary)

Same repro script (`dmig_post.txt`, exit 0): victim abandons legacy id `5dd20aef… → 52369558…` on first post-fix open; copy mints its own `84f7b3db…` (`DIVERGED`); copy `gc` exit 1 with `"reaped_creating":[],"reaped_git_orphans":[],"refused_locked":["…/wt-0"]`, `victim_dir_exists=yes victim_content_exists=yes victim_dir_present=yes`; original engine untouched (id still `52369558…`, row still `ready`, own `gc` exit 0 clean).

## Doc correction (mandatory one-liner — done, with proof)

The receipt + code docs claimed in-place overwrite "keeps the id (ACCEPTED residual)". QA-B proved reality diverges. Corrected in both the `engine_id` rustdocs and the Residuals section above: a plain `cp src.db dst.db` truncating onto dst's inode carries the source's stored pair, which mismatches dst's live identity → dst mints a fresh **third** id (safe direction). Proven: inode kept `yes`, source stable `yes`, `b0=f202a5d0… b1=5cb9e728… a1=e57c7c6a…`, `b1≠b0`, `b1≠a1` → `THIRD-ID` (adhoc shell + `studio init`, exit 0).

## Regression tests added (required)

- `state::tests::missing_file_identity_regenerates_a_fresh_id_and_binds_the_pair` (replaces `missing_file_identity_migrates_without_changing_the_id`, which pinned the defective keep behavior): pre-fix shape → regenerate, stable on second open.
- `state::tests::prefix_shaped_copy_diverges_on_first_open_of_both_files`: pre-fix shape + `cp` before first post-fix open → both abandon the legacy id and diverge from each other; both stable afterwards.
- `recovery::tests::prefix_shaped_copy_with_forged_row_refuses_and_victim_survives`: full kill chain on the pre-fix shape — both files diverge, forged row from the copy refused (`refused_locked`), nothing reaped, victim intact, victim's own gc clean. (The victim's live lock is stamped with the post-fix id, matching the safety argument: pre-fix binaries never issued `studio:<id>` reasons.)

## Gate / test summary (commands actually run, after fix)

| Gate | Command | Exit | Result |
|---|---|---|---|
| format | `cargo fmt --check` | 0 | clean |
| clippy | `cargo clippy --all-targets -- -D warnings` | 0 | clean, no `#[allow]` |
| tests | `cargo test` | 0 | **67 passed, 0 failed** (64 lib + 2 crash integration + 1 tree-sitter; was 65, +2 new, 1 replaced) |
| gate driver | `./scripts/gate.sh` | 0 | prints `GATE ORDER: PASS` |
| crash storm | `./scripts/crash_storm.sh 50` (run twice) | 0 | `CRASH-STORM: PASS … corrupt_rows=0 orphaned_worktrees=0` + **10** self-tests PASS |

Post-fix hashes (via `sha256sum`, exit 0):

| Artifact | sha256 |
|---|---|
| `studio-core/src/state/mod.rs` | `4b07e09cdad2a49f7e065b8f28e72fefa7f2f9a72dbdd5f6b37615601` |
| `studio-core/src/recovery.rs` | `c54cc9616a2154841dcb69e14fbf507fea3d0ea00191c15faf8dca48c00897f8` |

No new dependencies (no `Cargo.toml`/`Cargo.lock` change; no new `use`). No commit (`git status` shows only working-tree modifications). No swarms/other agents (single spawn). `.roadmap/` touched only in this receipt (`dossier.json` modification predates this spawn — not mine).

**Verdict: D-MIGRATION-01 genuinely closed.** Pre-fix repro shares one id in both files and reaps the victim from the copy; post-fix both files abandon the legacy id, diverge from each other, the copy's forged row is refused, and the victim is intact with a clean own-gc. All prior wins intact (67/67 tests, gate + storm green, 10 self-tests).
