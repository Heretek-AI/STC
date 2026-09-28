# PHASE-RECEIPT — 01-engine-skeleton (REWORK, retry 2/3)

**Program:** STC v2 greenfield rebuild · run `stc-greenfield` · branch `v2-greenfield`
**Frontier sha256:** `86b6c81b505caccdffed71d0ca5139047cf4bd693e838791ab306025a52a9af7`
**Goal:** minimal durable core — Tokio daemon + greenfield SQLite v2 (WAL, single-writer) + `studio-cli` read projections. No UI.
**Rework trigger:** QA seat B (adversarial) returned **fail** on retry 1 with destruction/security defects A3/A4/B/C1/D plus non-blocking E/F/G/C2. This receipt is retry 2. Every defect below has (a) the reproduction I ran against the **pre-fix** binary, (b) the fix, (c) the command + exit code proving it is gone. Scratch dir: `/tmp/opencode/rework2` (my own; QA-B's was `/tmp/opencode/qa-b2`).

> No claim below without a command I actually ran. Where I could not fully close a vector I say so under **Residual limitations** instead of overclaiming.

## Gate / test summary (commands actually run, after fixes)

| Gate | Command | Exit | Result |
|---|---|---|---|
| format | `cargo fmt --check` | 0 | clean |
| fallow | `scripts/gate.sh` step | 0 | clean (cockpit TS change audited) |
| Semgrep | `./scripts/gate.sh` → semgrep step | 0 | `Findings: 0`, `Ran 2 rules on 7 files`, `SEMGREP COVERAGE: rules=2 targets=7` |
| clippy | `cargo clippy --all-targets -- -D warnings` | 0 | clean, no `#[allow]` |
| tests | `cargo test` | 0 | **37 passed, 0 failed** (34 lib + 2 crash integration + 1 tree-sitter) |
| tree-sitter | `cargo test --test syntax_gate` | 0 | 1 passed |
| gate driver | `./scripts/gate.sh` | 0 | prints `GATE ORDER: PASS` |
| crash storm | `./scripts/crash_storm.sh 50` | 0 | `CRASH-STORM: PASS … corrupt_rows=0 orphaned_worktrees=0` + 3 self-tests PASS |

Hashes (post-fix): `scripts/gate.sh` `85e3534a…`; `scripts/crash_storm.sh` `bb0b256f6d26dd95d5eaae3498b8da54396318561b66097dcaa13b182902328d`; `semgrep-rules/stc-guardian.yaml` `4b2425c1…`; `studio-core/src/lock.rs` `de4ed011c89aaa0a840a9d7ea93c6deb8c0333e0bf854b926dedd91c2b23307e`; `studio-core/src/recovery.rs` `2e7bdf3f2a3f78119cf546499a673009923aa7b9e8e4f0293cbd1c333405eced`; `studio-core/src/worktree/mod.rs` `c1db628669faae1cb3987cac5652ca7914528d0a28881b9bfdfaabc8a938db52`; `studio-core/src/state/mod.rs` `c4e989b48ec659446eb7d215422ed7ed57e7b9e58cac6dc9c7f74ab939eac488`; `studio-cli/src/main.rs` `63fbf0a7075c5200f79dc8fd571b8e834ca876aebabe39f50b97b6fb52e8d1a2`; `cockpit/src/App.tsx` `d2aba5e5c26c34248a1e112a26aaf8734de48836f571d7483034366e2c5cbae6`.

---

## BLOCKING defects

### A3 — DESTRUCTIVE SYMLINK ESCAPE (security) — FIXED
**QA-B repro (reproduced, pre-fix):** `ln -s <victim> <root>/evilbucket`, then `studio gc`. The sweep used `Path::is_dir()` (follows symlinks) and `std::fs::remove_dir_all` (follows *intermediate* symlink components), so it recursed through the link and deleted the victim's files outside the root, exit 0.
```
{"clean":false,…,"swept_dirs":["<root>/evilbucket/marker.txt","<root>/evilbucket/data","<root>/evilbucket/keep"]}
GC_EXIT=0
--- victim after gc ---  <only the now-empty victim dir>
```
Dir: `/tmp/opencode/rework2/a3`.

**Fix:** `recovery` now walks with `symlink_metadata`/`lstat` semantics (`ChildKind::{RealDir,RealFile,Symlink}`) and **never** follows a link. A symlink bucket or child directly under the root is refused (typed `RecoveryError::SymlinkRefused`) and reported; deletion is a recursive `remove_tree_no_follow` that refuses at any symlink node, preceded by `subtree_has_symlink` so nothing is partially removed. `verify` uses the same safe walk and reports refused symlinks. A refused symlink makes the report unclean (fail-closed).

**Proof (fixed):**
```
studio gc --db … --repo … --worktree-root …/wt   → exit 0
{"refused_symlinks":["…/wt/evilbucket"],"swept_dirs":[], …}
find …/victim -type f  → marker.txt, keep/k.txt, data/nested.txt  (intact)
studio verify …  → {"clean":false,"refused_symlinks":["…/wt/evilbucket"]}   exit 1
```
Dir: `/tmp/opencode/rework2/fix-a3`. Covered by `recovery::tests::sweep_refuses_a_symlink_bucket_and_preserves_its_target` and `…_nested_in_a_bucket`; also enforced by the crash-storm `symlink_self_test`.

### A4 — symlinked trash dir relocates a live worktree outside the root — FIXED
**QA-B repro (reproduced, pre-fix):** an orphan git worktree under `<root>/<hash>/`, with `<root>/<hash>/.studio-worktree-trash` a symlink to `<victim>`, made `trash_rename` `create_dir_all` through the link and `rename` the worktree **into** the victim:
```
gc → exit 0;  reaped_git_orphans:["…/orphan"];  swept_dirs:["…/.studio-worktree-trash"]
find <victim> → <victim>/wt-1790555409369/.git   (live worktree outside the root)
```
Dir: `/tmp/opencode/rework2/a4`.

**Fix:** `trash_rename` refuses a source that is not a real directory and calls `prepare_real_dir` on the trash dir instead of `create_dir_all`. `prepare_real_dir` refuses (typed `WorktreeError::SymlinkRefused`) if the final component **or its immediate parent** is a symlink, uses `create_dir` (fails rather than silently accepting a pre-existing symlink like `create_dir_all`), and re-checks after creation. `WorktreeManager::recover` now propagates a refuse instead of swallowing it. `create` refuses a symlinked target path too.

**Proof (fixed):**
```
studio gc … → studio: error: worktree: refusing to traverse symlink (never leaves the worktree root):
              …/wt/5a9cb6b5/.studio-worktree-trash
GC_EXIT=2
victim_entries=0            # nothing relocated into the victim
<root>/5a9cb6b5/orphan still present; .studio-worktree-trash still a symlink
```
Dir: `/tmp/opencode/rework2/fix-a4`. Covered by `worktree::tests::trash_rename_refuses_a_symlinked_trash_dir`.

### B — LOCK IDENTITY WRONG (data loss) — FIXED
**QA-B repros (all reproduced, pre-fix):**
- **B(i)** `rm -f <db>.studio-lock` while daemon-1 held it → daemon-2 started (exit 0) concurrently; daemon-1 still alive. (`/tmp/opencode/rework2/b_i`.)
- **B(ii)** daemon-A on `dbA`, then daemon-B on `dbB`, same repo/root: B's boot recovery destroyed A's live `ready` worktrees. A had **86** live worktrees → after B boot **3**; A `verify` → `{"clean":false,"missing_rows":114}` exit 1. (`/tmp/opencode/rework2/b_ii`.)
- **B(iii)** daemon-B alive; `studio gc --db dbA --repo <same> --worktree-root <same>` → exit 0 with `reaped_git_orphans` = all of B's worktrees; B `verify` → `{"clean":false,"missing_rows":117}` exit 1. (`/tmp/opencode/rework2/b_iii`.)

**Fix:** the lock is keyed to the **shared resource**, not the DB path. `StudioLock::acquire(repo, worktree_root)` takes a non-blocking `flock(2)` on two directories that are never replaced in normal operation: `<repo>/.git` and the worktree root. There is **no lock file**, so unlink-and-recreate (B(i)) has nothing to defeat; the lock is per open-file-description, so a second opener—same or different DB—is refused with a typed `LockError::AlreadyHeld` naming the resource. `daemon` and `gc` both acquire it; cross-DB runs on one repo/root serialize by refusal.

**Proof (fixed):**
```
B(i) rm -f <db>.studio-lock (file does not exist) → second daemon exit 2:
     "another studio engine already owns this repo/worktree root (lock held: …/repo/.git)"
     daemon-1 still alive=yes
B(ii) B_exit=2 (typed refusal); A live worktrees 77 → 78 (growing, none deleted)
B(iii) gc_A_exit=2 (typed refusal); B live worktrees 79 → 80 (none reaped)
```
Dirs: `/tmp/opencode/rework2/fix-bi`, `fix-bii`, `fix-biii`. Covered by `lock::tests::{second_engine_on_same_repo_and_root_is_refused, different_root_same_repo_is_refused, different_repo_same_root_is_refused, non_repo_is_refused}`, the cross-process integration test `crash_recovery::lock_is_keyed_to_shared_resource_not_db_path`, and the crash-storm `lock_self_test`.

### C1 — READ-ONLY LOCAL DoS KILLS THE ENGINE — FIXED
**QA-B repro (reproduced, pre-fix):** DB mode `644`; a local process holding an `O_RDONLY` snapshot (`BEGIN; SELECT …; sleep 25`) deterministically killed the daemon:
```
db_mode=644
daemon_exit=2 wall=12s
studio: error: WAL backpressure: 4198312 bytes exceeds bound 4194304 and no checkpoint could reduce it
```
Dir: `/tmp/opencode/rework2/c1`.

**Fix:**
1. **Graceful degradation.** On `WalBackpressure` the daemon no longer exits: it logs, stops writing, and `relieve_wal_pressure` retries checkpoints until the WAL is back under the soft bound. A reader that never leaves simply pauses the writer (no writes → the WAL cannot grow). Only genuinely unbounded growth past a separate **hard ceiling** fails closed.
2. **Configurable bound (C2).** Soft bound: `--wal-bound-bytes` > env `STUDIO_WAL_BOUND_BYTES` > default 4 MiB. Hard ceiling: `--wal-hard-ceiling-bytes` > env `STUDIO_WAL_HARD_CEILING_BYTES` > `16 × bound`.
3. **DB permissions (defence-in-depth).** `StateStore::open` sets the DB and its `-wal`/`-shm` sidecars to mode `0600`. This cannot stop a same-user reader (nothing local can); the graceful path is the real fix.

**Proof (fixed):**
```
db_mode=600
daemon with 200 000 ticks + 25 s held reader: STILL ALIVE after 15 s
  [daemon] wal soft bound=4194304 bytes hard ceiling=67108864 bytes
  [daemon] WAL pressure: 4198312 bytes > soft bound 4194304 (reader contention); pausing writes, engine stays up
after reader released: final WAL=0 bytes
```
Dir: `/tmp/opencode/rework2/fix-c1`.

### C2 — WAL bound was a hard-coded const with no override — FIXED
**Repro (pre-fix):** `const WAL_BOUND_BYTES = 4*1024*1024` in `studio-cli/src/main.rs`; no CLI/env override existed.
**Proof (fixed):**
```
studio daemon --help → --wal-bound-bytes <WAL_BOUND_BYTES> ; --wal-hard-ceiling-bytes <WAL_HARD_CEILING_BYTES>
--wal-bound-bytes 65536 → [daemon] wal soft bound=65536 bytes hard ceiling=1048576 bytes
STUDIO_WAL_BOUND_BYTES=131072 → [daemon] wal soft bound=131072 bytes hard ceiling=2097152 bytes
```

### C3 — soft bound / dishonest contract — FIXED (report + enforce)
**QA-B repro (reproduced, pre-fix):** transient reader, `--ticks 400`: daemon **exit 0** while a sampler observed a peak above the bound:
```
daemon_exit=0
sampled_peak_wal=4198312  bound=4194304  final_wal=0
```
Dir: `/tmp/opencode/rework2/c3`.

**Fix / honest contract:** the bound is checked **between ticks**, so a single tick's transaction can transiently exceed it; the daemon now prints that explicitly. It reports its sampled peak and both limits on exit, and the hard ceiling is the enforced fail-closed limit (a paused writer cannot grow the WAL into it).
**Proof (fixed):**
```
C3 daemon_exit=0
[daemon] WAL pressure: 4198312 bytes > soft bound 4194304 (reader contention); pausing writes, engine stays up
[daemon] WAL pressure relieved at 0 bytes; resuming writes
[daemon] peak WAL 4148872 bytes (soft bound 4194304, hard ceiling 67108864)
```
Dir: `/tmp/opencode/rework2/fix-c3`.

### D — `verify` FALSE-CLEAN — FIXED
**QA-B repro (reproduced, pre-fix):** five planted rows all reported `clean:true` exit 0:
```
{"clean":true,"missing_rows":0,"git_orphans":[],"leftover_dirs":[]}   VERIFY_EXIT=0
```
with rows: (1) path outside the root, (2) `repo` a different repo, (3) `ready` row whose dir is empty, (4) path a regular file, (5) dir that is not a git worktree. Dir: `/tmp/opencode/rework2/d`.

**Fix:** `verify` now checks every ledger row (via new `StateStore::worktree_records`, which also returns `repo`) for **containment** (`path.starts_with(root)`), **repo-match** (stored `repo` == verified repo), **kind** (`symlink_metadata` must be a real dir; symlink/file → `non_directory_rows`), **empty**, and **git registration** (`git worktree list` membership), each reported under a typed class. `IntegrityReport::clean()` requires all of them empty. `studio verify` emits all classes.
**Proof (fixed):**
```
studio verify …  → exit 1
{"clean":false,
 "outside_root_rows":["…/outside/keep"],
 "repo_mismatch_rows":["…/wt/5a9cb6b5/repo-mismatch"],
 "non_directory_rows":["…/wt/5a9cb6b5/regular-file"],
 "empty_rows":["…/wt/5a9cb6b5/empty"],
 "not_a_worktree_rows":["…/outside/keep","…/empty","…/not-a-worktree","…/repo-mismatch"]}
```
Dir: `/tmp/opencode/rework2/fix-d`. Covered by `recovery::tests::verify_flags_each_containment_repo_and_kind_defect`.

---

## NON-BLOCKING

### E — cockpit false provenance — FIXED (small, truthful)
**Repro:** `cockpit/src/App.tsx` hard-coded `source: studio.db` in `StaleBadge`, read removed top-level `snap.runtime_mode`/`snap.autonomy_mode`, and its badges' tooltips asserted `studio.db project_autonomy` / `studio.db snapshot.runtime_mode` — false provenance after the projection moved config to its own section.
**Fix:** `StaleBadge` renders `snap.source`; the badge helpers read `snap.config.{runtime_mode,autonomy_mode}` (top-level optional for back-compat); tooltips now say `config (env STUDIO_LANE_RUNTIME) — not a studio.db projection` / `config (… project_autonomy lands in phase 02)`. `Snapshot` gained optional `source` and `config`.
**Proof:** `grep -n 'studio\.db' cockpit/src/App.tsx` → only `DB_PATH` and generic connect/error text remain (no provenance claim). The gate's **fallow** TS/JS audit step passed (`./scripts/gate.sh` exit 0). *Honesty:* the cockpit `tsc && vite build` was **not** run — `cockpit/node_modules` is absent in this worktree and no network install was attempted; the change is type-local.

### F — single-writer discipline unguarded by tests — FIXED
**Repro:** mutating `with_write`'s `BEGIN IMMEDIATE` → `BEGIN` left all prior 24 lib tests passing.
**Fix:** added `state::tests::with_write_holds_immediate_lock_before_first_statement`, which runs a `with_write` transaction that blocks *before its first statement* and asserts a concurrent `BEGIN IMMEDIATE` is refused (the lock must already be held).
**Proof (mutant kill, actually run):** with `with_write` mutated to `BEGIN`:
```
test …with_write_holds_immediate_lock_before_first_statement ... FAILED
with_write must hold the IMMEDIATE (write) lock *before* its closure runs; a concurrent BEGIN IMMEDIATE must be refused, got Ok(())
```
restored → `test result: ok. 1 passed`.

### G — `source` hard-coded regardless of `--db` — FIXED
**Repro (pre-fix):** `studio status --db other-name.db` reported `"source":"studio.db"`.
**Fix:** `StateStore::snapshot` derives `source` from the DB file name.
**Proof (fixed):**
```
studio init --db other-name.db ; studio status --db other-name.db → {"source":"other-name.db", …}
studio status --db studio.db → {"source":"studio.db"}
```
Covered by `state::tests::snapshot_source_reflects_actual_db_file`; the existing crash test's `snap["source"] == "studio.db"` still holds.

---

## No regression on D1–D11
D1/D2/D3 (canonical paths, one-pass convergence, relative paths), D4 (lock — now stronger), D5 (checkpoint tuple + bounded WAL), D6 (invalid state), D7 (projection honesty), D8 (non-tautological tests), D9 (dead `ledger`/`sweep_trash` still removed), D10 (Semgrep `--error` + Rust rules), D11 (crash-storm metric can fail) remain satisfied: `cargo test` 37/37, `./scripts/gate.sh` exit 0, crash-storm `defaults` mode (relative paths) 50/50 clean, and the storm still runs `metric_self_test` (PASS) plus the two new self-tests.

## New regression coverage added
- `recovery::tests::{sweep_refuses_a_symlink_bucket_and_preserves_its_target, sweep_refuses_a_symlink_nested_in_a_bucket, verify_flags_each_containment_repo_and_kind_defect}`.
- `worktree::tests::trash_rename_refuses_a_symlinked_trash_dir`.
- `lock::tests::{second_engine_on_same_repo_and_root_is_refused, different_root_same_repo_is_refused, different_repo_same_root_is_refused, non_repo_is_refused}`.
- `state::tests::{with_write_holds_immediate_lock_before_first_statement, snapshot_source_reflects_actual_db_file}`.
- `crash_recovery::lock_is_keyed_to_shared_resource_not_db_path` (cross-process).
- `scripts/crash_storm.sh` self-tests: `metric_self_test` (existing), `symlink_self_test` (new), `lock_self_test` (new).

## Residual limitations (honest)
- **TOCTOU.** Symlink refusal is `lstat`-based with a post-create re-check, not fd-anchored (`openat`/`O_NOFOLLOW`) traversal. A same-user adversary racing a link swap *between* the check and the unlink could still race; the reported vector (a symlink statically planted under the root) is closed. A follow-up could pin traversal with `openat`/`O_NOFOLLOW`.
- **Lock durability.** The lock is `flock` on the `<repo>/.git` and root directories. A same-user who `rmdir`s and recreates a directory obtains a new inode; this is not the reported `rm -f <db>.studio-lock` vector and the repo/root dirs are normally non-empty. No lock *file* exists to unlink.
- **C graceful pause.** Under a permanently held reader the daemon pauses indefinitely (by design: alive, WAL bounded). A finite `--ticks` run will not complete until the reader leaves.
- **D repo-match** compares a lexically-normalized stored `repo` to the canonical verified repo; a repo referenced through a different symlink alias could false-positive.
- **E build.** Cockpit `tsc && vite build` not run (no `node_modules`); fallow audit and a type-local edit only.

## Constraints honoured
No commit. No swarms/other agents. `.roadmap/.hash-manifest.json` and `.roadmap/*/dossier.json`/`GOAL.md` untouched — the only `.roadmap/` file changed is this receipt. `adapters/`/`tui/` not reintroduced. No `#[allow]`/ignore suppressions.

**Verdict: A3, A4, B(i/ii/iii), C1, C2, C3, D fixed with reproduced reproductions and command+exit-code proofs; E, F, G fixed; residual limitations stated. Gate green, 37/37 tests, crash-storm 50×2 + 3 self-tests PASS.**
