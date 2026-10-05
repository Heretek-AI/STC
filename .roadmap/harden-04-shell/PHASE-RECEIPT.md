# PHASE-RECEIPT — harden-04-shell (hardening-only, no new views/endpoints)

**Program:** STC v2 greenfield rebuild · run `harvest-harden-01` · phase `harden-04-shell`
**Phase brief:** `.roadmap/harden-04-shell/phase.json` (goal: Harden cockpit-shell MS-1: SSE parity + cursor races, WS Ready contract pre-reg, daemon PID/version + version-length bound, hub burst waiter guidance, limit clamp pin, codegen re-measure, chronofault robustness, Ask double-decide + contract (V1-V14+X1-X4, F1-F7 clean-room/keep))
**Mode:** hardening-only. No new views/endpoints, no new runtime deps, no version upgrades. No swarms or other programmers invoked (single programmer implementation).

> Every claim below carries the command run and its exit code. Open questions and deferrals are under **Residual limitations**, not overclaimed.

## Phase evidence hashes cited (from phase brief — all re-verified pre-phase)

| Claim | sha256 / REPORT | Pointer | Check this run |
|---|---|---|---|
| brainstorm fallback REPORT (V1 SSE gap+resync, V2 epoch fence, V3 limit threading, V4/V5 WS Ready+GapKind pre-reg, V6 PID-reuse, V7 version-length bound, V8 burst waiter, V9 limit clamp, V10 codegen re-measure, V11/V12 gate robustness, V13 double-decide, V14 decide-table defer, X1-X4 oracles) | `ses_ef31a3c64ffeZqpTtbF7zJTvRV` | session REPORT | vectors implemented §1–§13 in spike order |
| darkharvest fallback REPORT (F1 SSE clean-room VK 53c0d412/openrig 7810d559/agent-office cf99f145, F2 WS skip + ViewShell keep, F3 daemon clean-room lifecycle 1871e259/shutdown 1a67629ead/probe c7fec56d, F4 hub keep + wait skip + claude-squad AGPL skip, F5 ts-rs depend + codegen keep, F6 gate keep, F7 CAS keep) | `ses_ef318ec6effeasYojo2ScBLeJV` | session REPORT | no GPL/AGPL copy (§5, grep exit 1); depends axum 0.8 + tower-http 0.6 + ts-rs 12 only, pinned lines kept |
| anchor `studio-web/src/hub.rs` (pre-phase) | `12eb18205f5dd9e089eaf4dada65113c77d68959fded5f8f70cd51315bb9908a` | `file://studio-web/src/hub.rs` | `sha256sum` pre-edit matched exactly (see §E) |
| anchor `studio-web/src/server.rs` (pre-phase) | `89c54557a752c3ea14d4dad4061d9d5aa537bc946190e9bb0fda6305af513df4` | `file://studio-web/src/server.rs` | `sha256sum` pre-edit matched exactly |
| anchor `studio-web/src/daemon.rs` (pre-phase) | `c7fec56d2a45abbf9252fd3fd82a5a4ba9eb5a2afeb9edab89e223a42e575ac8` | `file://studio-web/src/daemon.rs` | `sha256sum` pre-edit matched exactly |
| anchor `studio-web/src/api.rs` (pre-phase) | `256b1be4f7b8f9ee168807e8382f53cd6f8f5e314eb8e065a06a8b3f7097a0d6` | `file://studio-web/src/api.rs` | `sha256sum` pre-edit matched exactly |
| anchor `studio-web/src/catalog.rs` (pre-phase) | `3c1008a8b3bb9db53bd6761d05838c3af6d990606e330c6f388a276a8eb82c47` | `file://studio-web/src/catalog.rs` | `sha256sum` pre-edit matched exactly |
| anchor `studio-web/src/staleness.rs` (pre-phase) | `8873bc334d9909f04a93b4b66308a4b2870b5b349e1bdf298bd3cae1a9ffb103` | `file://studio-web/src/staleness.rs` | `sha256sum` post-phase unchanged (untouched) |
| anchor `studio-web/tests/web_contract.rs` (pre-phase) | `efa1d80bad7232d4a464282ccc422a86054b5776d2c9995aa34ef36133bd646f` | `file://studio-web/tests/web_contract.rs` | `sha256sum` pre-edit matched exactly |
| anchor `scripts/ms1_gate.sh` (pre-phase) | `45a9baf2f5f03e5d30d3204c94b4268d649747f9bbfaf0c4e6ec9394dba8031f` | `file://scripts/ms1_gate.sh` | `sha256sum` post-phase unchanged (untouched) |
| anchor PHASE-RECEIPT 04 (MS-1 initial + R1) | `abebc7553fe4812618e6a0a473dcc4de73e0cb382996f96fa71dd9fb4f8e7ca3` | `file://.roadmap/04-cockpit-shell-ms1/PHASE-RECEIPT.md` | `sha256sum` this run matched exactly |

§E — pre-edit anchor verification (this run, before any edit):
`sha256sum studio-web/src/*.rs studio-web/tests/*.rs scripts/ms1_gate.sh` printed all eight anchors exactly as above (hub `12eb1820…`, server `89c54557…`, daemon `c7fec56d…`, api `256b1be4…`, catalog `3c1008a8…`, staleness `8873bc33…`, web_contract `efa1d80b…`, ms1_gate `45a9baf2…`) plus receipt `abebc755…` for `.roadmap/04-cockpit-shell-ms1/PHASE-RECEIPT.md`. No other file touched.

## Gate / test summary (commands actually run, final tree)

| Gate | Command | Exit | Result |
|---|---|---|---|
| tests (workspace) | `cargo test --workspace` | 0 | **308 passed, 0 failed** (293 baseline intact + 15 new spike pins: daemon 3 + hub 4 + server 2 + catalog 2 + contract 4) |
| clippy | `cargo clippy --all-targets --workspace -- -D warnings` | 0 | 0 warnings, no `#[allow]` in source |
| format | `cargo fmt --check` | 0 | clean |
| frontend build | `(cd cockpit && npm run build)` (`tsc && vite build`) | 0 | clean, `dist/` rebuilt (169.95 kB JS) |
| V11/V12 gate | `RUNS=10 EVIDENCE_DIR=/tmp/opencode/harden04-gate ./scripts/ms1_gate.sh` | 0 | **10 runs, 0 flakes** (bar ≤1); log copied to `evidence/gate-10runs.log` |
| license grep | `grep -rniE 'general public license\|affero\|AGPL\|GPL-3\|GPLv3' studio-web/src` | 1 (no hits = pass) | no GPL/AGPL copy (F1-F7 clean-room MIT/Apache-2.0) |
| deps pin | `git diff -- Cargo.toml studio-web/Cargo.toml Cargo.lock` | 0 diff (empty = pass) | no new runtime deps; axum 0.8 + tower-http 0.6 + ts-rs 12 lines kept, no upgrades |
| endpoints pin | `grep -n "route(" studio-web/src/server.rs` | 0 new | 12 routes unchanged (11 GET + 1 POST decide); no new endpoints |
| TS mirror | `cargo test -p studio-web --test web_contract ts_export_matches_committed` | 0 | `cockpit/src/api-types.ts` untouched (reverted after accidental gen-ts run; DTOs unchanged) |

File hashes (via `sha256sum`, post-phase final tree):

| Artifact | sha256 | Provenance |
|---|---|---|
| `studio-web/src/api.rs` | `6236fc3039836c221f7cdcf256e8091cd275d74654c47ed3fce4dc3dcd5be74a` | + V4/V5 WS pre-reg docs only (no DTO change) |
| `studio-web/src/catalog.rs` | `03701f97a52e50f198ce8a2673d54b8068a6d938886aac2beca2bef4851c0191` | + V10 re-measure + V14 defer docs + 2 pin tests |
| `studio-web/src/daemon.rs` | `07de8db912d2cd5acd9aabfe2186958d10c40ff1e0e707988f579c2e7ceffee9` | V6 pid≤1 reject + V7 length bound + 3 pin tests (REWORK) |
| `studio-web/src/hub.rs` | `bc892b7262884a876e9606978142b6e98878b9c0a356fd4ebf55955cb972418c` | + V2/V8 docs + 4 pin tests (X1/V9/V8/V2) |
| `studio-web/src/server.rs` | `e09048c36407895a70c6eda7bd485d9833bd4b9c4d2075c050cbab3164a53845` | + V1 SSE parity tests (2) |
| `studio-web/src/staleness.rs` | `8873bc334d9909f04a93b4b66308a4b2870b5b349e1bdf298bd3cae1a9ffb103` | untouched |
| `studio-web/src/lib.rs` | `8cf1c5e59c257a77b93ab2237359f88e12d014119daddd597fba00759d576050` | untouched |
| `studio-web/src/main.rs` | `9724694c04f7a66591e8b6502a22ea0e5a435639b1eece9b08a5e1bffa8edc06` | untouched |
| `studio-web/tests/web_contract.rs` | `fb437a54b919a7e8e9f35e66d48735ad09d3d1d7f4af6b165c3a0a012a950e3b` | + V3/V13/V4-V5/V10 pins (4 tests) |
| `scripts/ms1_gate.sh` | `45a9baf2f5f03e5d30d3204c94b4268d649747f9bbfaf0c4e6ec9394dba8031f` | untouched |
| `cockpit/src/api-types.ts` | `9e4e233f438fdb6df2d64bd7c40b733ca21a919344d1f3c22f5e1ae14cc60820` | untouched (DTOs unchanged; accidental gen-ts churn reverted) |

## What was hardened (spike order, cheapest first, stop-at-kill; rework only on confirmed bypasses)

### X1/X2 oracles (cheapest — docs+tests, no rework)

**X1 history gap oracle** (`hub::x1_history_gap_oracle_empty_on_lost_contiguous_on_none`): total property — Lost ⟺ empty page (never silent truncation, never fabricated rows); None ⟺ contiguous `(since, head]` in seq order. Wide-ring (6 into 512) and overflow (10 into 4) legs. PASSES (no bypass; existing semantics hold).

**X2 probe oracle** (`daemon::x2_probe_oracle_all_arms_legible`): all six probe arms (missing/unparseable/stale/mismatch/reachable/degraded) report legible typed detail with named files, fail-closed. PASSES (no bypass).

### V9 limit clamp pin (cheap — pin, no rework)

**Was:** `history()` already clamped `limit.clamp(1,1024)` (and `changes_since` likewise), but unpinned — a future change dropping the clamp would silently empty pages (limit 0) or OOM on huge limits.

**Now:** `hub::v9_limit_clamp_pin_zero_and_huge` pins limit 0→1 row, limit 1→seq 1, huge 999999→10 retained, Lost-still-empty on overflow, resync 4 rows. PASSES (no bypass; clamp already correct).

### V6 probe — PID-reuse (CONFIRMED BYPASS → REWORK)

**Was:** `pid_alive` checked only `/proc/<pid>` existence. Forged pid file with `1` reported reachable when `/proc/1` exists + version matched (init is never the daemon). PID-reuse window likewise undocumented (existence ≠ identity).

**Now:** `probe_in` rejects `pid <= 1` as stale by construction (`never a daemon`), with legible detail. Reuse window documented on `pid_alive`: liveness requires live-pid AND version-file match; reused pid with stale/missing version still fails closed. **Proof:** `daemon::v6_pid_zero_and_one_are_stale_never_reachable` — FAILS pre-fix (pid 1 reachable), PASSES post-fix.

### V1 SSE parity — since-floor gap+resync (docs+tests, no rework)

**Was:** `PollStream` replayed `hub.history` but had no parity pin — a future change could resume mid-gap silently or mis-track cursor.

**Now:** `server::v1_sse_parity_gap_marker_then_live` pins stale cursor → leading `{"gap":"Lost","head_seq":H}` only, cursor==head; floor cursor → 4 contiguous rows, no gap, cursor==head, seqs ordered to head. `v1_sse_since_floor_never_spurious_gap` pins since<0 clamps (F2) with no spurious gap on wide ring. PASSES (no bypass; parity holds). Note: tests are `#[tokio::test]` (PollStream::new needs a reactor for the sleep waker).

### V3 limit threading (pin, no rework)

**Was:** `?limit=` flowed into `hub.history` in `events_handler` and `agent_stream_handler`, but unpinned over HTTP — ignoring `?limit=` or panicking on 0/huge would be silent.

**Now:** `web_contract::v3_limit_threading_clamped_over_http` pins limit=1→1 row seq 1, limit=0→1 row (clamped), huge→10 retained, default→10, agent-stream limit=1→1. PASSES (no bypass; threading already correct).

### V13 concurrent decides — double-decide (pin, no rework)

**Was:** `decide_stored` CAS (`WHERE status='pending'`) is the single-shot guard, but unpinned under concurrency — double-apply or 500s under contention would be a ledger fork.

**Now:** `web_contract::v13_concurrent_decides_single_shot` fires 10 concurrent `POST /api/ask/apr-race/decide` (mixed approve/deny) — exactly 1×200 wins, 9×409 `already_decided`, ledger keeps winner, GET shows decided+disabled. PASSES (no bypass; CAS holds under contention via `BEGIN IMMEDIATE` + busy-timeout).

### V8 burst waiter guidance (docs+pin, no rework)

**Was:** burst overflow (10 into cap-4) reported Lost but waiter guidance lived only in the gate script — slow consumers could retry the stale cursor forever.

**Now:** hub module docs carry the guidance (on Lost, resync from `floor_seq`, page with limit, never retry stale cursor); `hub::v8_burst_waiter_resync_guidance` pins Lost-empty, stale-retry-stays-Lost, resync→exactly cap rows, limit=2 walks contiguously to head 10. PASSES.

### V11/V12 gate robustness (runs, no rework)

`RUNS=10 …/ms1_gate.sh` → exit 0, 10/10 PASS, 0 flakes (fresh label + 10 tasks + ask live + POST 405 + screenshots per run; stale pause→recovery; lost gap + resync 4 rows). Log `evidence/gate-10runs.log`. No flakes, no rework.

### V7 version-length bound (CONFIRMED BYPASS → REWORK)

**Was:** version file read unbounded and echoed in full into `/api/health` detail — multi-KB operator file bloats the health response (DoS surface).

**Now:** `VERSION_MAX_LEN=128`; longer content is fail-closed mismatch with truncated 64-char preview (`…[truncated N chars]`), never echoed in full. Boundary MAX_LEN compares normally. **Proof:** `daemon::v7_version_length_bound_is_fail_closed_truncated` — FAILS pre-fix (full echo, `detail.contains(&huge)`), PASSES post-fix. Initial pin assertion (`detail.len() < huge.len()`) was too strict (fixed text exceeds 228-char fixture); corrected to `!detail.contains(&huge)` before green.

### V10 counts + codegen re-measure (docs+pin, no rework)

**Counts:** `web_contract::v10_counts_lane_totals_pin` re-measures lane totals with one task per lane (building/validating/in-review/done → 1/1/1/1 + 4 rows). PASSES (mapping matches `projection::lane_for`).

**Codegen:** catalog docs re-measure at 7 entries — hand table 60 lines (~8.6/entry, declarative) vs rejected `catalog_entry!` ~130 expanded LOC/entry (P04) → ~15× holds; `catalog::v10_hand_table_still_beats_codegen` pins 7 rows + shape. Re-measure trigger documented (new envelope primitive, not more rows). No generator added.

### V2 epoch fan-out fence (docs+pin, DEFERRED contract change)

**Was:** DB-epoch one-shot Lost (F3) consumed by first `history()` after turnover — correct single-consumer, but concurrent fan-out consumers miss per-consumer Lost (stateless `since` cannot name the epoch; seq namespaces collide on restore).

**Now:** hub docs record the residual explicitly; `hub::v2_epoch_one_shot_and_fanout_residual` pins one-shot fires once then new-epoch rows serve (old-4 → new-1), with the fan-out miss documented as the assertion (`second.gap==None` with new rows). Full per-consumer fencing needs `?epoch=` + `HistoryDto.epoch` (contract change, V14-adjacent) — deferred, not this phase. No rework (would widen the cursor contract).

### V4/V5 WS Ready+GapKind pre-reg (docs+pin, no transport)

F2 holds (SSE only, no WS endpoint). `api.rs` GapKind docs pre-register the future WS shape: first frame `{"ready":{"head_seq":N,"floor_seq":M,"gap":<GapKind>}}`, then `event` frames identical to SSE `EventDto` JSON; ViewShell stays transport-agnostic. `web_contract::v4_v5_ws_prereg_gapkind_serde` pins `GapKind` wire values (`"None"`/`"Lost"`) + SSE content-type (`text/event-stream`, never 101 upgrade). No WS code added.

### V14 decide-route table defer (docs+pin, no widening)

Catalog docs record the deferral (read-view table with empty/loading/error triples cannot host the POST decide write with 200/400/404/409/503 outcomes). `catalog::v14_decide_route_deferred` pins no `decide` name/route in `CATALOG`. Write-surface contract stays owned by `write_surface_is_decide_only` + `decide_round_trip_is_single_shot` + V13.

## Acceptance checklist with evidence paths

- [x] `cargo test --workspace` green (exit 0, 308/308) — 293 baseline intact + 15 spike pins
- [x] `cargo clippy --all-targets --workspace -- -D warnings` clean (exit 0)
- [x] `cargo fmt --check` clean (exit 0)
- [x] `RUNS=10 …/ms1_gate.sh` green (exit 0, 0 flakes, `evidence/gate-10runs.log`)
- [x] new spike tests kill-gated (V6/V7 document FAILS-pre-fix / PASSES-post-fix; all others document the bypass sought + the hold)
- [x] no GPL/AGPL copy (`grep -rniE` exit 1; F1-F7 clean-room, SPDX MIT/Apache-2.0 per darkharvest REPORT `ses_ef318ec6effeasYojo2ScBLeJV`)
- [x] no new runtime deps (`git diff -- Cargo.toml studio-web/Cargo.toml Cargo.lock` empty; axum 0.8 + tower-http 0.6 + ts-rs 12 pinned, no upgrades)
- [x] no new endpoints (`grep route(` shows 12 routes unchanged; V4/V5 add docs only, V14 explicitly defers)
- [x] hardening-only (2 narrow reworks: pid≤1 reject + version bound; everything else docs+tests; no behavior widened)

## Guard note (dossiers untouched)

`git status` shows modifications only in `studio-web/src/{api,catalog,daemon,hub,server}.rs` + `studio-web/tests/web_contract.rs` + this receipt + `evidence/gate-10runs.log`. Zero modifications under `.roadmap/*/dossier.json`, `.roadmap/*/GOAL.md`, `.roadmap/.hash-manifest.json`. `cockpit/src/api-types.ts` left untouched (DTOs unchanged; accidental `gen-ts` churn reverted, `ts_export_matches_committed` still green).

## Residual limitations (not overclaimed)

1. **V2 fan-out:** per-consumer epoch Lost needs `?epoch=` + `HistoryDto.epoch` (contract change) — deferred; single-consumer gate + rename-restore / same-inode-retreat legs covered.
2. **V6 reuse window:** `/proc` existence + version match is the fence; a reused pid with a *matching* version still reports reachable (requires operator rotating the version file on daemon restart — outside the probe).
3. **WS deferred:** SSE only (F2); V4/V5 pin the wire shape for the later transport, no WS code here.
4. **Decide table:** POST decide stays out of the catalog (V14) by design.
5. **prek:** `prek run --all-files` exceeds 120s in this tree (hook budget); rustfmt/clippy/fmt/gitleaks-equivalent gates above are green, dossiers reverted. Manager decision on the prek budget still pending (same guard conflict as P02/P03).
6. **Branch:** work delivered in the `main` working tree (where P02/P03 closed); merges are the manager's call.

## Retry 1 — bounded enforcement (qa-b FAIL 14 bypasses + qa-a conditional typo)

**Retry brief anchors (pre-retry, re-verified):** hub `bc892b72` (`bc892b7262884a876e9606978142b6e98878b9c0a356fd4ebf55955cb972418c`), server `e09048c3` (`e09048c36407895a70c6eda7bd485d9833bd4b9c4d2075c050cbab3164a53845`), daemon `07de8db9` (`07de8db912d2cd5acd9aabfe2186958d10c40ff1e0e707988f579c2e7ceffee9`), api `6236fc30` (`6236fc3039836c221f7cdcf256e8091cd275d74654c47ed3fce4dc3dcd5be74a`) — `sha256sum` pre-edit matched exactly. No swarms or other programmers invoked.

**QA verdicts driving this retry:** qa-a `conditional` (only staleness full-hash doc typo 56ch vs 64ch, no code change needed) + qa-b `FAIL` (14 live bypasses, gates green 308; repros `/tmp/opencode/qa-b-harden04/probe*.sh`). Fixes below are bounded enforcement, hardening-only, no new views/endpoints, no new runtime deps, no GPL.

### Fix list with proof (all FAILS-pre-fix → PASSES-post-fix, live repros re-run)

1. **Future-cursor (fix 1):** `since>head` now `Lost` empty (was silent `None` empty); resync guidance from `floor_seq`. Proof: `hub::future_cursor_is_lost_not_silent_none` + `web_contract::fix1_future_cursor_is_lost_over_http` (probe1 `since=999999` now `Lost`, resync heals, `since==head` stays `None` empty). Headers `X-Resync-From` + `Retry-After` on Lost.
2. **Epoch stale-cursor (fix 2):** old-head `since` after turnover now `Lost` (was `None`/Fresh); `status?since=` mirrors Lost. Proof: `hub::epoch_stale_cursor_after_turnover_is_lost` + `fix2_epoch_stale_cursor_is_lost_over_http` (probe6 `since=3/4` after turnover to head-1 now `Lost`, status Lost; `OK` not `BUG`).
3. **SSE limit (fix 3):** `HistoryParams.limit` (`i64`, signed) threads into `PollStream::new(hub,since,limit)` (no 512 hardcode); negative clamps to 1 like HTTP; non-numeric rejects typed 400. Proof: `server::sse_limit_threads_into_pollstream` (limit=1→1 row, 0→1, future Lost) + `fix3_sse_limit_threads_like_http` (probe3 stream count 1 not 10, `limit=-1` 200/1 row, `limit=abc` 400 JSON).
4. **Method oracle (fix 4):** HEAD/OPTIONS/TRACE/CONNECT on `/api/*` are 405-or-safe (HEAD on GET routes 200 headers no-body safe, never 200-with-body; everything else 405; decide POST-only all 405). Proof: `fix4_method_oracle_extended_is_safe` (extends `write_surface_is_decide_only` beyond 4 methods; probe2 HEAD 200 empty, OPTIONS/TRACE 405).
5. **Typed errors (fix 5):** query-reject 400 (`bad_query`), body 415 (`unsupported_media_type`)/400 (`bad_request`)/422 (`unprocessable_entity`) are JSON typed (were `text/plain`); F1 `unknown_api_route` promise kept. Proof: `fix5_query_and_body_rejects_are_typed_json` (probe1/2 `limit=abc`/`since=abc`/`1.5`/empty/overflow now 400 JSON, decide no-CT 415 JSON, malformed 400 JSON, missing/wrong-type 422 JSON; `CT:application/json`).
6. **PID-file bound (fix 6):** pid echo truncates like version (128 + `[truncated]`); reads bounded via `PROBE_READ_CAP=4096` (`read_bounded`, never full 10M `read_to_string`); strict decimal digits only (`+123` no longer reachable). Proof: `daemon::pid_file_bound_truncates_huge_echo` (1M pid detail<10k, no full echo) + `version_huge_file_is_bounded_without_full_read` (100KB truncated, `exceeds`); probe4b huge-10M 0.1s truncated 255B, plus-pid now unparseable.
7. **TS nullability (fix 7):** `#[ts(type="number")]` erasure on `Option` fields replaced with `#[ts(type="number | null")]` (`decided_ms`×2, `ts_ms`, `schema_version`) — honest `number|null`; mirror regenerated (`cargo run -p studio-web --bin studio-web-gen-ts`); `ts_export_matches_committed` green. Proof: `fix7_ts_nullability_matches_live` (pending `decided_ms` null, agent `ts_ms` null, mirror contains all three `number | null`); probe7 grep shows fixed.
8. **Badge parity (fix 8):** `Hub::seq_gap_for` (floor/future/epoch, `None`=age-only by design) threaded into war-room/audit/mcp/providers/runs via `?since=`; omitted stays honest Fresh; Lost carries `X-Resync-From` + `Retry-After`. Proof: `fix8_badge_parity_threads_seq_gap_with_retry_headers` (overflow `?since=0` all Lost+headers, omitted Fresh; agent-stream gap Lost); probe3 headers `retry-after:1` + `x-resync-from:6`.
9. **db_source (fix 9):** badge keeps short basename by design (documented on `db_source`); absolute `db_path` in `/api/health` distinguishes same-name DBs; gate asserts basename+path. Proof: `fix9_db_source_keeps_basename_but_health_distinguishes` (two `studio.db` same source, different `db_path`); probe7 `db_path` differs; gate health `db_path` check.
10. **Reason length (fix 10):** `chars().count()` not `len()` (bytes); 1..=1024 chars kept. Proof: `fix10_reason_counts_chars_not_bytes` (1024×`é`=2048B →200, 1025→400); probe9 unicode 1024 now 200.
11. **Route parsing (fix 11):** consistent 404 JSON for encoded slashes; `DECIDE_ID_MAX_CHARS=256` bound; `/`/NUL/control invalid without echo (no `%00` leak). Proof: `fix11_route_parsing_is_consistent_404_without_echo` (`a%2Fb` 404 `unknown_request` no `/`, `%00` 404 no NUL/`%00`, 500×`x` 404 no echo); probe8 `%00`/500 now sanitized, `a/b/c` fallback `unknown_api_route` JSON.
12. **WS Ready (fix 12):** explicitly deferred with `ws_ready_deferred_no_upgrade` (Upgrade never 101, still 200 `text/event-stream`; SSE carries no `ready` frame — Ready is WS-only, `gap` marker instead). Proof: `fix12` + `server::sse_limit_threads_into_pollstream` Ready-absent leg + `api.rs` WS-Ready deferral docs; probe2 Upgrade 200 SSE, probe3 `no ready frame`.
13. **Receipt staleness hash typo (fix 13, qa-a conditional):** line 20 (and file-hash line 51) fixed from 56ch `8873bc334d9909f04a93b4b2870b5b349e1bdf298bd3cae1a9ffb103` to 64ch `8873bc334d9909f04a93b4b66308a4b2870b5b349e1bdf298bd3cae1a9ffb103` (inserted missing `6308a4b2`; `sha256sum studio-web/src/staleness.rs` matches; staleness.rs untouched).
14. **Gate root guard (fix 14):** `is_root`/`pause_db`/`resume_db` detect EUID 0 and substitute rename-away pause leg for chmod leg (cleanup restores `.paused`; never false red/green). Proof: gate log `is_root` branch + `RUNS=10` green as non-root (1000) and leg present for root; probe9 grep shows guard.

### Gate / test summary (retry tree, commands actually run)

| Gate | Command | Exit | Result |
|---|---|---|---|
| tests (workspace) | `cargo test --workspace` | 0 | **324 passed, 0 failed** (308 prior intact + 16 new pins: hub 2 + daemon 2 + server 1 + contract 11) |
| clippy | `cargo clippy --all-targets --workspace -- -D warnings` | 0 | 0 warnings, no `#[allow]` in source |
| format | `cargo fmt --check` | 0 | clean |
| frontend build | `(cd cockpit && npm run build)` (`tsc && vite build`) | 0 | clean, `dist/` rebuilt (169.95 kB JS) |
| ms1_gate | `RUNS=10 EVIDENCE_DIR=/tmp/opencode/harden04-retry-gate10 ./scripts/ms1_gate.sh` | 0 | **10 runs, 0 flakes**; log `evidence/gate-10runs-retry1.log` (fresh+health db_path, stale pause→recovery incl. root leg, lost gap+resync+retry headers) |
| license grep | `grep -rniE 'general public license\|affero\|AGPL\|GPL-3\|GPLv3' studio-web/src` | 1 (no hits = pass) | no GPL/AGPL copy |
| deps pin | `git diff -- Cargo.toml studio-web/Cargo.toml Cargo.lock` | 0 diff (empty = pass) | no new runtime deps; axum 0.8 + tower-http 0.6 + ts-rs 12 kept |
| endpoints pin | `grep -n "route(" studio-web/src/server.rs` | 0 new | 12 routes unchanged (11 GET + 1 POST decide); no new endpoints |
| TS mirror | `cargo test -p studio-web --test web_contract ts_export_matches_committed` | 0 | `cockpit/src/api-types.ts` regenerated honestly nullable, test green |

File hashes (via `sha256sum`, retry final tree):

| Artifact | sha256 | Provenance |
|---|---|---|
| `studio-web/src/api.rs` | `f45bae0c0400fdb55e6e53ad0ddba716772c30b8e589143d1e494a3e52310247` | fix 7 nullable `number\|null` ×4 + fix 12 WS-Ready deferral docs |
| `studio-web/src/catalog.rs` | `03701f97a52e50f198ce8a2673d54b8068a6d938886aac2beca2bef4851c0191` | unchanged (retry) |
| `studio-web/src/daemon.rs` | `74ca000ac6cba365b85c84c9e3758ac44c574a48d2a988583546aca1665b3ede` | fix 6 bounded reads + pid truncation + strict digits + 2 pin tests |
| `studio-web/src/hub.rs` | `1b1112845e035280911c1734188cfbaa38989d24faf99e249c7679dec993a2b6` | fixes 1-2 future/epoch Lost + `seq_gap_for` + 2 pin tests |
| `studio-web/src/server.rs` | `5ea9556cc9e8659fae9f3ab6921687a13f132f9ab2117acae3cbf2d2b7e9087a` | fixes 1-5,8-12 (signed limit, typed rejects, threaded gaps/headers, chars, id bound, PollStream limit) + 1 pin test |
| `studio-web/src/staleness.rs` | `8873bc334d9909f04a93b4b66308a4b2870b5b349e1bdf298bd3cae1a9ffb103` | untouched (hash typo fixed in receipt only) |
| `studio-web/src/lib.rs` | `8cf1c5e59c257a77b93ab2237359f88e12d014119daddd597fba00759d576050` | untouched |
| `studio-web/src/main.rs` | `9724694c04f7a66591e8b6502a22ea0e5a435639b1eece9b08a5e1bffa8edc06` | untouched |
| `studio-web/tests/web_contract.rs` | `a3c3e6f5e058987b7c4f5f388ad563b109540b3263548025c0f52eeadc129052` | fixes 1-5,7-12 pins (11 tests) |
| `scripts/ms1_gate.sh` | `eaa29963877c3306760ab0b002631b3bbd7c7ce4f80cdfc07d1130506d69b162` | fix 9 health db_path assert + fix 8 retry-header asserts + fix 14 root guard |
| `cockpit/src/api-types.ts` | `2d0e5054b981349ad5638543daf4fcf77b29d85b4f42e88a0fddcc0ed81d7203` | regenerated honestly nullable (`number\|null` ×4) |

### Residual limitations (retry, not overclaimed)

1. V2 fan-out per-consumer fencing still needs `?epoch=` (deferred contract change); single-consumer + future fence covered.
2. V6 reuse with matching version still reachable by construction (operator must rotate version on restart).
3. WS still deferred (SSE only); Ready shape pinned for later transport.
4. prek budget still pending manager decision (same as P02/P03); rustfmt/clippy/gate green.

### Retry 1 re-verification (this spawn, 2026-10-05)
- Pre-retry anchors re-verified from receipt: hub `bc892b72` (`bc892b7262884a876e9606978142b6e98878b9c0a356fd4ebf55955cb972418c`), server `e09048c3` (`e09048c36407895a70c6eda7bd485d9833bd4b9c4d2075c050cbab3164a53845`), daemon `07de8db9` (`07de8db912d2cd5acd9aabfe2186958d10c40ff1e0e707988f579c2e7ceffee9`), api `6236fc30` (`6236fc3039836c221f7cdcf256e8091cd275d74654c47ed3fce4dc3dcd5be74a`). No swarms or other programmers invoked.
- `cargo test --workspace` exit 0 — **324 passed, 0 failed** (308 prior + 16 new pins).
- `cargo clippy --all-targets --workspace -- -D warnings` exit 0; `cargo fmt --check` exit 0.
- `(cd cockpit && npm run build)` exit 0 — 169.95 kB JS.
- `RUNS=10 EVIDENCE_DIR=/tmp/opencode/harden04-retry-gate10 ./scripts/ms1_gate.sh` exit 0 — 10 runs, 0 flakes; log copied to `evidence/gate-10runs-retry1.log`.
- `grep -rniE GPL studio-web/src` exit 1 (no hits); `git diff -- Cargo.toml studio-web/Cargo.toml Cargo.lock` empty; `grep -n "route(" studio-web/src/server.rs` 12 routes unchanged.
- `cargo test -p studio-web --test web_contract ts_export_matches_committed` exit 0.
- Live repros: `probe1.sh` future `since=999999` now `Lost` empty + typed 400 JSON; `probe4b` huge-10M 0.1s truncated + `+pid` unparseable. All 14 fixes hold. No new runtime deps, no GPL.
- Retry final tree hashes re-verified via `sha256sum` — unchanged from table above (hub `1b111284…`, server `5ea9556c…`, daemon `74ca000a…`, api `f45bae0c…`, contract `a3c3e6f5…`, gate `eaa29963…`, ts `2d0e5054…`, staleness `8873bc33…` untouched).
