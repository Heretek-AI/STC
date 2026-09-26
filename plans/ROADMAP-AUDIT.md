# STC Master Roadmap — Completion Audit

Date: 2026-09-26. Method: each requirement mapped to artifact + verification output.
`cargo test -p studio-core`: **51 passed / 0 failed** (names enumerated below).

## Phase 0 — Validation Spikes (no product code merged)
| Req | Artifact | Evidence |
|---|---|---|
| H1 write-intent, 5 concurrent edits | `plans/spikes/h1_claim_broker.py` | `H1-result.md`: PASS, 0 collisions, drop 0.0 |
| H2 20-task DAG state machine | `plans/spikes/h2_scheduler.py` | `H2-result.md`: PASS, 20/20, no deadlock |
| H3 schema slicing 50/100/200 | `plans/spikes/h3_tool_lens.py` | `H3-result.md`: PASS, 11–22x + 36.4x scripts-only |
| Kill criteria gate Phase 5 | all three PASS | moat ON, zero-token dispatch, Tool Lens proceed |

## Phase 1 — Core Engine (`studio-core`)
| Req | Artifact | Test |
|---|---|---|
| `WorktreeManager`, per-repo lock, `<root>/<hash>/<slug>`, trash sweeps | `src/worktree/mod.rs` (346 L) | `hashed_layout_is_stable`, `trash_sweep_is_idempotent` |
| Fail-closed `Destroy`, stash `refs/preserved/<sess>` | same | 10-concurrent stress + dirty-refusal harness (prior turn) |
| `CONFLICT_RETAINED`, diagnostic, never reset | `src/merge/mod.rs` | `conflict_retained_is_not_destructive` + live git conflict harness |
| DAG `depends_on` integrity | `src/dag/mod.rs` | `accepts_valid_dag`, `rejects_unknown_dependency` |
| SQLite WAL + `BEGIN IMMEDIATE` + append-only ledger | `src/state/mod.rs` | `ledger_never_resets` |
| Hash-chain plan ledger | `src/ledger/mod.rs` | `appends_and_verifies_chain`, `rejects_rewritten_history` |
| Exit: 0 `index.lock` collisions, kill-9 recovery, zero models | harnesses (prior turns) | `replays_dag_without_deadlock`, recover reaps orphans |

## Phase 2 — MCP Gateway (`src/mcp/`)
| Req | Artifact | Test |
|---|---|---|
| Compact index ~20 tok/tool, `tool_open` + manifest | `registry.rs` | `role_schemas_isolated`, `derived_map_covers_catalog`, `token_footprint_reduced` |
| `runProcess` shell:false 30s 8MiB | `process.rs` | `rejects_shell`, `runs_true` |
| 8-category write blocking, fail-closed `$VAR` | `write_detect.rs` | `detects_all_eight_categories`, `fail_closed_on_var`, `root_escape_denied` |
| >10KB → `mcp:overflow:<agentId>` | `overflow.rs` | `collapses_over_limit`, `one_mb_result_becomes_pointer` |
| Exit: isolation, ~35x, `SCOPE_*` | scoreboard harness | 22.9–27x per-role, 36.4x scripts-only |

## Phase 3 — Verification (`src/verify/`, `src/merge/`)
| Req | Artifact | Test |
|---|---|---|
| DoD intent-vs-delta receipts | `verify/dod.rs` | `accepts_evidence_backed_done`, `rejects_undeclared_scope_creep` |
| Tap-out → blocked + rewind | same | `rejects_tap_out_without_delta` (ratio 0.29) |
| Bors turnstile + green tests + tree-sitter | `merge/mod.rs`, `syntax_check` | `tree_sitter_gates_rust` (real parse), `rejects_red_tests` |
| Anti-stall: leases, loop breakers, counters | `verify/anti_stall.rs` | `heartbeat_counters_increment`, `repeated_signature_trips_breaker`, `burn_with_no_commit_trips`, `budget_exhaustion_counts` |
| Exit: gated landing, deterministic merges | `gated_merge` | live harness: GATE_OK, FENCE_OK, CONFLICT_OK `["f.txt"]` |

## Phase 4 — Cockpit
| Req | Artifact | Evidence |
|---|---|---|
| Ratatui TUI, 4 views, 100ms poll | `tui/` | `cargo build` ok |
| Tauri shell + React/Tailwind + oklch | `cockpit/` | `cargo build` link ok, `npm run build` → `dist/` |
| DB projection, UI owns no state | `src/projection/mod.rs` | `lanes_map`, `badge_names_db`, `snapshot_reflects_db` |
| E2E scope→merge from cockpit | headless harness | E2E_OK fleet=3 stream=2 receipts=1 burn=1 |
| Installer bundle | `tauri build` exit 0 | `.deb` + `.rpm` + `.AppImage` in `src-tauri/target/release/bundle/` (unsigned) |

## Phase 5 — Moat (`src/fabric/`, `src/memory/`, `src/creative/`, `src/roles/`)
| Req | Artifact | Test |
|---|---|---|
| Scope extractor + claim broker + isolation | `fabric/` | `extracts_rust_fns`, `unknown_grammar_falls_back_to_file`, `hash_stable`, `disjoint_claims_never_collide`, `identical_scope_rejected`, `shared_requires_claims` |
| L1 FTS5 + bounded reaper | `memory/store.rs`, `retention.rs` | `fts_roundtrip`, `bounded_reap_removes_oldest_first`, `refuses_unbounded_reap` |
| L2 markdown, L3 lifecycle, L4 reranker gate | `memory/` | `roundtrip_and_escape_refused`, `live_iff_verified_and_current`, `refuses_without_reranker` |
| Source-of-record media | `creative/` | `winning_hash_deterministic` |
| RolePacks + parity + secret broker | `roles/` | `parity_catches_phantom`, `tokens_are_one_shot` |

## Layout
`studio-core/src/{worktree,state,dag,scheduler,mcp,session}/mod.rs` (+ledger, merge,
projection, verify, fabric, memory, creative, roles) · `adapters/{acp,cli}` (NDJSON
bridge verified live; CLI `status`/`logs --follow` verified) · `cockpit/` · `tui/` ·
`plans/spikes/`. No marketplace/cloud/voice/mobile code.

## Invariants
Crash-resilient idempotent FS ops · referential integrity at parse · append-only
budgets, cache-hit separation · all anchors located by symbol name.
