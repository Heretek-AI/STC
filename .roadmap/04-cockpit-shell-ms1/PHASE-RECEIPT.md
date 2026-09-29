# PHASE-RECEIPT — 04-cockpit-shell-ms1 (initial implementation, carries MS-1)

**Program:** STC v2 greenfield rebuild · run `harvest-review-01` · `main` working tree (predecessors 02 signed off `2ff256b`, 03 signed off `6b2cb8d`; `v2-greenfield` merge is the user's)
**Phase brief:** `.roadmap/04-cockpit-shell-ms1/GOAL.md` · parent dossier `.roadmap/04-cockpit-shell-ms1/dossier.json` (verdict: pending — NOT modified, see guard note) · annex `.roadmap/04-cockpit-shell-ms1/ANNEX-harvest-review-01.md` (sha256 `659a3a42e2a4df3bd167a2abd51861030dfc38ae849e533e8c38e87e73558602`) · gate dossier `.roadmap/harvest-review-01/dossier.json` (`7cac8a00e6cdb0dba39e230e3f47d1bb91558eebc835f5d4cbe3f0ba4d100e5e`)
**Goal:** studio-web (axum+tower) serving the browser SPA + /api projection reads, ONE real view with staleness badges, MS-1 live browser gate. Server owns projection; browser reads only.

> Every claim below carries the command run and its exit code. Open questions and deferrals are under **Residual limitations**, not overclaimed. No swarms or other programmers invoked.

## Phase evidence hashes cited (from parent dossier.json — §-corrections apply)

| Claim | sha256 | Pointer | Check this run |
|---|---|---|---|
| frontier (settled decisions) | `86b6c81b505caccdffed71d0ca5139047cf4bd693e838791ab306025a52a9af7` | `file://.research/frontier.json` | see §E4.2 correction note (file absent in-tree; live frontier `.factory/frontier.json`) |
| v1 projection (read candidate) | `3600ef7d48ac6c1c325b49d3f976c1995b4fa75b62fdcf48aef5465c7c633c4f` | `file://studio-core/src/projection/mod.rs` | see §E4.3 revision (reproducible revision form, no caret syntax) |
| v1 state | `dbc6bf00233ebe76bf6ce397a36f7690adf22d04024f1d193d45402cd2db8503` | `file://studio-core/src/state/mod.rs` | see §E4.4 (v2 tree has its own evolved state; port-or-reference explicit) |
| beta dossier (alpha_completed=false → HYPOTHESIS) | `082c5c6e1bc543996d2372206e7ca8123ba02e2f60e5108f53fbbf6fa206dc1` | `file://.research/scratchpads/scope_01_engine_state_isolation/beta_dossier.json` | cited as HYPOTHESIS only; no design settled from it |
| gate dossier | `7cac8a00e6cdb0dba39e230e3f47d1bb91558eebc835f5d4cbe3f0ba4d100e5e` | `file://.roadmap/harvest-review-01/dossier.json` | read-only; 4 P04 adoption rows + V7/V8 + A1 extracted |
| phase annex | `659a3a42e2a4df3bd167a2abd51861030dfc38ae849e533e8c38e87e73558602` | `file://.roadmap/04-cockpit-shell-ms1/ANNEX-harvest-review-01.md` | read-only; implemented exactly |

### E4.2 — Frontier correction (P02/P03 lessons applied)

The parent dossier asserts `86b6c81b…` @ `file://.research/frontier.json` (VERIFIED_HASH — the dossier author's claim, guarded, NOT altered). That file is ABSENT in this tree (`ls .research/frontier.json` → No such file); the live frontier in this tree is `.factory/frontier.json`. I cite the dossier claim + this footnote and do not assert a `sha256sum` match against a file I cannot see.

### E4.3 — v1 projection citation revision (reproducible form, no caret syntax)

The v1 `studio-core/src/projection/mod.rs` (`3600ef7d…`, FILE_HASH_ONLY) lives in pre-greenfield history, not in the v2 tree. Reproducible command without caret syntax: `git log --all --format='%H' -- studio-core/src/projection/mod.rs` lists the containing revision(s); `git show <full-hash>:studio-core/src/projection/mod.rs | sha256sum` reproduces `3600ef7d48ac6c1c325b49d3f976c1995b4fa75b62fdcf48aef5465c7c633c4f`. The v2 projection consumed by `/api` is the greenfield `studio-core/src/projection/mod.rs` + `state::snapshot` already in-tree (P01 port rule); this phase adds only read-paths over it, no v1 code copied.

### E4.4 — v1 state port-or-reference (explicit)

v1 `studio-core/src/state/mod.rs` (`dbc6bf00…`, FILE_HASH_ONLY) is pre-greenfield history. The v2 `studio-core/src/state/mod.rs` is an evolved greenfield rewrite (WAL/single-writer/contract tables, phases 01–03). This phase PORTS nothing from the v1 file; it REFERENCES the v2 store via two additive read helpers (`changes_since`, `max_change_seq`) plus the existing `snapshot()` and `open_readonly()` paths. No schema change, no migration, `schema_version` stays 2.

## Starting state (repro)

`git log --oneline -1` → `6b2cb8d phase(03-tool-gateway): close MCP gateway`. The v2 tree had **no `studio-web/`**, no CDC cursor helper (`change_log` triggers existed but no reader), no approval-listing read (only single-row `load_request`), and a Tauri-coupled `cockpit/src/App.tsx` (`invoke("snapshot")` + `/snapshot` fallback). Axum in-tree was 0.7.9 (via `acp-http-adapter =0.4.2`, P02 pin, untouched). I built the shell on the P02/P03 predecessors without modifying their semantics.

## Gate / test summary (commands actually run, final tree)

| Gate | Command | Exit | Result |
|---|---|---|---|
| tests (workspace) | `cargo test --workspace` | 0 | **219 passed, 0 failed** — core lib 160 (158 P01–P03 intact + 2 new CDC/ask reads) · crash_recovery 2 · e2e 6 · mutants 13 · replay 6 · spawn-matrix 3 · syntax_gate 1 · tool_gateway 10 · web lib 10 (NEW) · web_contract 8 (NEW) |
| clippy | `cargo clippy --all-targets --workspace -- -D warnings` | 0 | 0 warnings, no `#[allow]` in source (3 doc-lint + 1 parse + stream fixes, no suppression) |
| format | `cargo fmt --check` | 0 | clean |
| frontend build | `(cd cockpit && npm run build)` (`tsc && vite build`) | 0 | clean, `dist/` rebuilt 4× (tauri-drop, latch, favicon, hook-split) |
| prek | `prek run --all-files` | 0 overall (2 hooks fail, see guard note) | 8/10 hooks Passed; `trailing-whitespace` + `end-of-file-fixer` fail ONLY on guarded dossiers (reverted); rustfmt/clippy/gitleaks/fallow/CODEOWNERS all Passed; **fallow gate passes with 0 new findings** (2 fixed in my files, see below) |
| V8 gate | `RUNS=10 EVIDENCE_DIR=/tmp/opencode/ms1-10runs ./scripts/ms1_gate.sh` | 0 | **10 runs, 0 flakes** (bar: ≤1); full log `evidence/gate-10runs.log` |
| MS-1 live browser | chrome-devtools vs `studio-web :3769` + real DB | n/a (interactive) | status view (5 tasks, fresh badge, daemon banner) + ask view (1 row, disabled buttons) snapshotted + screenshotted, **0 console errors** post-reload |

File hashes (via `sha256sum`, this run):

| Artifact | sha256 | Provenance |
|---|---|---|
| `studio-web/src/api.rs` (13 ts-rs DTOs + `export_ts`) | `8edb92ffa66206105cea4a4792701f56033b2c3612788c43b32b931ef462d0a8` | written this phase |
| `studio-web/src/staleness.rs` (fresh/stale/lost classifier + frozen clock) | `8873bc334d9909f04a93b4b66308a4b2870b5b349e1bdf298bd3cae1a9ffb103` | written this phase |
| `studio-web/src/daemon.rs` (PID/version probe, fail-closed typed) | `c7fec56d2a45abbf9252fd3fd82a5a4ba9eb5a2afeb9edab89e223a42e575ac8` | written this phase |
| `studio-web/src/hub.rs` (ring + history cursor + poller + status view) | `c349975992323556d341c5b305d811dc16a776e1d5271fcdfb61ee641c6d66c1` | written this phase |
| `studio-web/src/catalog.rs` (V7 table + kill evaluation, hand-write wins) | `64c2011eefb816f3978d15444f6cf90970bf0f11ee24e2d533c72767d5853f83` | written this phase |
| `studio-web/src/server.rs` (GET-only router + poll-driven SSE + 503 shapes) | `1e65755da89f9d9330098c5e57981e8b41d0f32c3cb23bcd0f1168de0a7e31ff` | written this phase |
| `studio-web/src/lib.rs`, `src/main.rs`, `src/bin/gen_ts.rs` | `8cf1c5e5…` / `fb23e389…` / `95adaddd…` | written this phase |
| `studio-web/tests/web_contract.rs` (8 contract tests) | `5c596a972128601336d4e134768781469bad7250fbbe441356efe41980607438` | written this phase |
| `studio-web/Cargo.toml` (+ lockfile note) | `5831324af631d0e2721d854b93b5e3e25628ec5746e1ccc968f57c8306207571` | written this phase |
| `studio-web/fixtures/status_{empty,loading,error}.json` | (3 files, V7 fixtures) | written this phase |
| `scripts/ms1_gate.sh` (V8 gate) | `73797491daf8ec3176132a7b4e551d1dde3ce95940447ba022b5b89807788882` | written this phase |
| `cockpit/src/App.tsx` (browser-first shell) | `a08beb1984fc36c5b988d309aa1ffc77bc0b92aca05ef2d10a3d8704c331cbfa` | rewritten this phase |
| `cockpit/src/api.ts`, `src/api-types.ts` (generated), `src/components/StalenessBadge.tsx`, `src/views/StatusView.tsx`, `src/views/AskView.tsx`, `src/hooks/useProjection.ts` | `a1b4fa3f…` / `f4095190…` / `f066c3c5…` / `223dbe4d…` / `a094e249…` / `e69a07b4…` | written this phase |
| `cockpit/index.html` (favicon stub), `cockpit/package.json` (tauri dropped) | `cdd9aac2…` / `8e6208cf…` | edited this phase |
| `studio-core/src/approvals/mod.rs` (+`list_requests` + test) | `24f2dc592823a7b03bd081a94d78abc6738c1a9ffbabcf38efded8ecd3768ebe` | additive only |
| `studio-core/src/state/mod.rs` (+`ChangeEntry`/`changes_since`/`max_change_seq` + test) | `e84382e272545252f24f36e16ff39faac4fee89f17736092c0b45b7b626632d9` | additive only |
| workspace `Cargo.toml` (+`studio-web` member) | `e1148117d3b0ca00b2a451ed6ec62c6ec354f1c500bf2278d1a03d06e8b27c6e` | +1 member line |

Lockfile impact (`Cargo.lock`, modified): axum 0.8.x + tower-http 0.6.x + ts-rs 12.x + futures-core/clap-transitives added; axum 0.7.9 retained (P02 `acp-http-adapter` pin, untouched) — two axum lines coexist, tokio 1.x shared, no conflicts. Full note in `studio-web/Cargo.toml`.

## What was built (4 annex adoptions + V7/V8 + A1)

- **f04-vk-stream (c027–c029, depend+clean-room):** axum 0.8 + tower-http 0.6 (starting-point line adjusted 0.8.4/0.5→0.8.x/0.6.x — tower-http 0.6 is the axum-0.8 pair; cache-available, documented). SSE = combined history+live per the vetted shape (replay missed pages, then poll-driven live at CDC cadence, `gap` markers on eviction). WS snapshot/json-patch deltas deferred to P05 (brief allows and/or; recorded residual).
- **f04-ts-rs (c030–c031, depend):** crates.io `ts-rs` **12.0.1** (MIT) — dossier pinned 11.x for vibe-kanban parity, but P02 deferred vendoring the upstream file so our export is greenfield; 12.x decided at implementation with lockfile note. NOT the xazukx fork. Generated in the test step (`gen_ts` bin + `ts_export_matches_committed` red-on-stale); file committed at `cockpit/src/api-types.ts`.
- **f04-herdr-api (c032–c033, clean-room):** schema-first hub — subscriptions (SSE), history cursor with `None`/`Lost` gap semantics (never silent truncation), event shape `{seq,tbl,row,op,old,new}` over `change_log`.
- **f04-sandbox-daemon (c034, clean-room):** PID/version-file ensure-running semantics inform the UX: `probe` reports Running/Missing/StalePid/VersionMismatch legibly on `/api/health` + cockpit banner. The server never starts/masks the daemon (read-only serving + banner = fail closed legibly).
- **V7 Projection Catalog Codegen:** kill criterion evaluated — macro-generated entry ≈130 LOC vs 41-line hand table + shared constructor → **hand-write wins, said here** (`catalog.rs` docs). Deliverable intact: 1 entry (`status`) → handler + TS type + 3 fixtures + red-on-stale contract test (`EXPECTED_STATUS_KEYS` + fixture-shape + ts-mirror tests).
- **V8 Chronofault MS-1 Evidence Gate:** `scripts/ms1_gate.sh` — seeded ledger (10 tasks + 1 approval, change_seq asserted), frozen clock (exact fresh label), CDC pause (chmod-000 → stale → recovery), burst + ring-overflow gap (Lost + 4-row resync), badge text asserted, DB-only source (`mock` grep + provenance + task-identity checks), read-only (POST 405) and ask-disabled pins. **10 runs, 0 flakes** (`evidence/gate-10runs.log`); 3 headless screenshots per run (`evidence/fresh|stale|lost.png`).
- **A1 Ask surface (read scope):** `/api/ask` over `list_requests` (new read-only P02-store reader); `decision_enabled:false` on every row; buttons render disabled with the decide-later reason, never fake-enabled. No decide endpoint exists (P05 wiring).

## Acceptance checklist with evidence paths

- [x] studio-web serves SPA + /api real projection — `server::build_router` (ServeDir + index fallback; legible 503 when dist absent); `/api/status|ask|events|events/stream|health` all read `studio.db` via `open_readonly`/`snapshot`/`changes_since`/`list_requests`; `web_contract::status_response_matches_dto_shape` (10 seeded tasks over HTTP)
- [x] MS-1 live browser renders real data + staleness badge + zero mock, screenshots — chrome-devtools vs `:3769`: status snapshot (5 tasks ms1-t0…t4, `fresh · source: studio.db · updated 45ms ago · seq 5`, daemon banner) + `evidence/ms1-live-status.png`; ask snapshot (apr-ms1-live pending, both buttons `disabled`) + `evidence/ms1-live-ask.png`; **0 console errors** post-reload; gate screenshots `evidence/fresh|stale|lost.png` (distinct md5s)
- [x] browser read-only — GET-only routes; `web_contract::browser_is_read_only` (5 routes × 4 methods → 405); live `POST /api/status → 405`, `POST /api/ask → 405`; SSE `text/event-stream` replays real CDC rows
- [x] missing daemon fails closed legibly — `daemon::probe` 4 unit tests (missing/stale-pid/version-match/mismatch); `health_reports_missing_daemon_legibly` over HTTP (`detail` names `studio daemon`); live banner in browser snapshot; stale-phase keeps serving last-good with honest aging (never an error-as-data)
- [x] frontend build + cargo test green — `npm run build` exit 0 (tsc + vite); `cargo test --workspace` exit 0, 219/0; clippy `-D warnings` exit 0; `fmt --check` exit 0

## QA-driven fixes recorded (pre-receipt, all with regression pins)

1. **CDC seed wrong table:** hub tests seeded `events` (no `change_log` triggers) → cursor tests failed honestly. Fixed seeds to task rows with a comment naming the trigger coverage (`hub.rs`, `web_contract.rs`).
2. **Test-global env races:** `STUDIO_HUB_RING_CAP`/`STUDIO_RUN_DIR` mutated per-test → parallel flakes. Fixed with env-free constructors (`Hub::new_with_cap`, `daemon::probe_in`); env only read in production paths.
3. **Unsound SSE waker hack:** first SSE adapter used unsafe broadcast-waker plumbing. Replaced with a poll-driven ring stream (correct waker via `Sleep` poll, one tiny `futures-core` dep).
4. **ts-rs 12 API:** `decl(&Config)` + `export` prefix + `#[ts(type="number")]` for JSON-honest numerics (no `bigint` over the wire).
5. **fallow new-findings gate:** fixed `unused-type HistoryDto` (typed client), removed CSS orphaned by the rewrite, split `App` (6→≤4 per function: hook module + `ConnectedShell` + `PollError`), reaching exit 0.

## QA ROUND 1 RETRY (status `conditional`, retries=0/3) — bounded fix list, proofs

Manager tiebreak REQUIRED this round over qa-a's PASS / qa-b's CONDITIONAL (4 reproduced findings, none MS-1-blocking per QA-B, undisclosed — disposition below). All four FIXED with regression tests that fail pre-fix; no scope expansion, no swarms.

### F1 — Unknown /api/* served SPA HTML (api_fallback shadowed)

**Reproduced:** with a dist present, `GET /api/nope-xyz` → `404 text/html` with the `index.html` body — the outer `fallback_service(spa)` shadowed the API `fallback`, making `api_fallback` dead code exactly as reported.
**Fix:** the `/api` scope is now a NESTED router with its own JSON 404 (`unknown_api_route`); the SPA service is the outer fallback (lowest priority, cannot shadow). No-dist branch additionally pins an explicit `/api/{*api_rest}` JSON route because the `/{*path}` legible-503 wildcard would otherwise beat the nested fallback (explicit routes beat nested fallbacks — pinned in a code comment).
**Proof:** `web_contract::unknown_api_route_is_typed_json_never_spa` — with-dist typo → 404 + `application/json` + `unknown_api_route` (fails pre-fix with text/html); `/` still serves the SPA shell; no-dist typo → same JSON 404 (this leg caught the wildcard shadowing during the fix itself).

### F2 — Negative cursor spurious Lost

**Reproduced:** `/api/events?since=-5` → `Lost` with floor 0 (a nonsense cursor manufactured a gap).
**Fix:** `history()` clamps `since<0` to 0; `Lost` requires a real floor>0. Same clamp in `status_view`'s cursor param.
**Proof:** `hub::negative_cursor_clamps_to_zero_never_spurious_lost` (unit: clamp→`None`+rows; overflowed ring still `Lost`; clamped cursor on an overflowed ring correctly `Lost` since 0 genuinely predates the floor) + `web_contract::negative_cursor_is_not_spurious_lost_over_http`.

### F3 — DB-epoch split-brain (restore/re-init under live server)

**Reproduced (integrity-relevant):** replacing the DB file under a live server moved seq backwards with NO Lost and served phantom old-epoch rows (head stuck via `max()`, ring never invalidated).
**Fix (default taken, not the document-only option):** `Hub` tracks a DB generation guard — file identity `(dev, ino)` (mtime/size excluded: they change on every write) plus a max-seq retreat check (`snapshot.change_seq < head`). Either trip resets the ring (clear + floor 0 + head=new max), sets a one-shot `epoch_gap` consumed by the next `history()` as `Lost` with no fabricated rows, and backfills the new epoch's retained window so the post-Lost cursor pages real rows.
**Proof:** `hub::db_replace_resets_ring_and_surfaces_lost_once` (rename-restore leg: Lost exactly once, then new-epoch rows only, `change_seq` 1, no phantoms) + `hub::db_in_place_overwrite_with_retreating_seq_resets` (same-inode cp-overwrite leg via the retreat check). Caught-and-fixed during the round: the first reset dropped the new epoch's own history (empty ring under a live head) — the backfill was added with the failing assertion as pin.
**Remaining corner (documented residual R9):** same-inode in-place overwrite with NON-retreating seq cannot be distinguished from normal writes (requires an operator crafting newer-seq content onto the live file — outside the threat model); normal restores either replace the inode or retreat the seq, both covered. Non-unix targets rely on retreat detection only (no inode identity).

### F4 — /api/status could never emit Lost (FIX taken)

**Report:** `StalenessState::Lost` was unreachable over HTTP (`status_view` hardcoded `seq_gap=false`).
**Fix (not document-only):** `status_view(daemon, cursor)` threads the caller's event cursor into the taxonomy — `Some(c)` below the ring floor yields `Lost` (reachable via `/api/status?since=`); `None` keeps the age-only verdict. Epoch-Lost stays events-cursor-surfaced (one-shot flag); status-Lost reflects cursor-vs-floor. Rationale recorded at `server.rs`/`hub.rs` doc comments.
**Proof:** `hub::status_cursor_below_floor_is_lost_over_the_taxonomy` (Lost via cursor, Fresh without, Fresh at floor, clamped-negative consistency) + `web_contract::status_since_below_floor_is_lost` (HTTP: `?since=0`→Lost label `lost · …`, bare→Fresh). `EXPECTED_STATUS_KEYS` unchanged (no DTO shape change; fixtures + TS mirror untouched).

### Retry gate summary (final tree, commands actually run)

| Gate | Command | Exit | Result |
|---|---|---|---|
| tests (workspace) | `cargo test --workspace` | 0 | **226 passed, 0 failed** (219 intact + 7 new: 4 hub unit + 3 contract) |
| clippy | `cargo clippy --all-targets --workspace -- -D warnings` | 0 | 0 warnings, no `#[allow]` |
| format | `cargo fmt --check` (after `cargo fmt` on the appended tests) | 0 | clean |
| frontend build | `(cd cockpit && npm run build)` (no cockpit change this round) | 0 | clean |
| V8 gate | `RUNS=10 ./scripts/ms1_gate.sh` | 0 | **10 runs, 0 flakes** (`evidence/gate-r1-10runs.log`) |
| prek | `prek run --all-files` | 0 overall (same 2 guard-conflict hooks) | 8/10 Passed; dossiers reverted; fallow still 0 new findings |

Touched-file hashes recomputed this round (via `sha256sum`):

| Artifact | sha256 |
|---|---|
| `studio-web/src/hub.rs` (F2/F3/F4) | `12eb18205f5dd9e089eaf4dada65113c77d68959fded5f8f70cd51315bb9908a` |
| `studio-web/src/server.rs` (F1 nest + F4 param) | `dbc76348d73adb02289503642c489fabf090d7dd51fb370c1d727f3be78849a0` |
| `studio-web/tests/web_contract.rs` (+3 regression tests) | `5f2484c4bd62f029d8879a447608531ab35c7e7c1b15b8895a50b8d34b9978ef` |

All other artifact hashes in the table above unchanged (no other file touched).

## Guard note (do NOT modify GOAL.md / dossier.json — honoured)
`git status -- .roadmap/` shows zero tracked modifications (verified post-run after reverting the hooks below). New files only: `PHASE-RECEIPT.md` (this file) + `evidence/` (gate log, 10-run log, 5 screenshots).

**prek record:** 8/10 hooks pass (yaml, large-files, merge-conflict, rustfmt, studio-core clippy, gitleaks, fallow, CODEOWNERS-gate). `trailing-whitespace` + `end-of-file-fixer` FAIL for one reason only: they rewrite guarded dossiers (11 `.roadmap` files touched: trailing newlines). Same guard conflict as P02/P03 — reverted with `git checkout -- .roadmap/`, which deterministically re-arms the failure. Manager decision still required: bless the whitespace fix or waive the hooks for `.roadmap/**`.

## Residual limitations (not overclaimed)

1. **WS deferred:** SSE only; WS snapshot/json-patch deltas (vk c028–c029 second half) land with P05 views.
2. **Decide-later:** no POST decide endpoint; Ask buttons disabled by construction (P05 wiring against the approval service).
3. **Daemon PID files:** `studio daemon` does not yet write `studio-daemon.pid/.version` (no studio-cli change this phase); the probe semantics are defined and the missing path is the tested/seen one. Wiring the writer is runtime scope (P05 or daemon phase).
4. **Two axum lines:** 0.7.9 (P02 pin) + 0.8.x coexist in `Cargo.lock`; revisit if `acp-http-adapter` ships an axum-0.8 line.
5. **Pre-existing dirt untouched:** `plans/*/antigravity/*.md` + `scripts/stc_supervisor.py` prek whitespace mods predate this run; `--help*`, `cand.py`, `skills/`, `.roadmap/h1-*/`, `.roadmap/phase-01-cockpit-beta/`, `.roadmap/04-*/ANNEX-*` (untracked input) likewise left alone.
6. **`pages/Providers.tsx` unwired:** P05 scope (f05-openfang-providers); left in tree, not navigated, not mock-served.
7. **Stale screenshot timing:** chmod-000 pause is a real mid-run unreadability condition (also exercised: recovery to fresh). Frozen clock covers only the fresh-phase determinism.
8. **Branch:** work delivered in the `main` working tree (where P02/P03 closed); the `v2-greenfield` merge is the manager's call.
9. **F3 corner (QA-R1):** same-inode in-place DB overwrite with non-retreating seq is indistinguishable from normal writes (operator-crafted content — outside the threat model); non-unix targets get retreat-detection only (Linux target has full inode identity).
