# Phase 3 Checkpoint (exit gate satisfied 2026-09-26)

## Implementation (`studio-core/src/verify/` + `merge/` gate)
- `verify/dod.rs`: `DodCheck{declared/actual paths, claims_done, message, tests_green,
  syntax_ok, files_done/planned}`, `evaluate_done` — tap-out prose list, zero-delta done,
  undeclared-path creep, declared-absent, scope ratio <0.80, placeholder scan,
  tests/syntax evidence. `syntax_heuristic_ok` (balanced delimiters, string-aware).
- `verify/anti_stall.rs`: `StallSupervisor` (beat/advance, max_missed, evictions,
  NoSession/StaleHeartbeat/Generic), `DoomLoopDetector` (repeat-signature window,
  burn-with-no-commit ≥20/0, per-role budgets). All counters unit-tested to increment.
- `merge::gated_merge`: refuses unless DoD Done ("nothing lands unverified"),
  then head-SHA fence, then dry-run merge; conflict => CONFLICT_RETAINED with
  `{conflictFiles}` captured BEFORE abort + diagnostic, worktree kept (never reset).

## Exit-gate evidence (`cargo test` 28/28 + live git harnesses, throwaway)
- TAPOUT_OK: 80% + "Here's what's next" -> Blocked, ratio=0.29
- GATE_OK: `dod_done=false` -> landing refused
- FENCE_OK: stale fence -> re-review refusal
- CONFLICT_OK: parallel branches editing f.txt -> ConflictRetained files=["f.txt"],
  diagnostic with rebase hint; coder-b branch intact (no destructive reset)
- Unit: evidence-backed Done accepted; undeclared creep/red-tests/unbalanced syntax rejected;
  heartbeat/eviction/loop-break/burn/budget counters increment

## Update (later turn): real tree-sitter gate
- `tree-sitter 0.25` + `tree-sitter-rust 0.24` wired into `verify::dod::syntax_check`
  (Rust parsed, `has_error` refused; other languages keep the deterministic
  heuristic fallback; unknown grammars never fail the write).
- `cargo test` 51/51 including `tree_sitter_gates_rust`.
