# H1 — Write-Intent Fabric Spike: PASS

Date: 2026-09-26 | Harness: `plans/spikes/h1_claim_broker.py` (throwaway, no product code)

## Question
Can 5 parallel edits on one shared tree coordinate via scope claims + SQLite broker?

## Method
- `scope_hash(file|fn|symbol)` (sha256, file-granularity fallback for unknown grammars)
- SQLite WAL + `BEGIN IMMEDIATE` claim broker, `claims(scope_hash, owner)`
- 5 threads, 10 disjoint `(file,fn)` claims (non-overlapping by construction)
- Baseline: same edits in isolated temp copies (worktree analogue)

## Results
- shared_success=1.00, baseline_success=1.00, drop=0.000 (kill if >0.05)
- collisions_on_non_overlapping=0 (kill if >0)
- overlap_reject_check: same-scope second claim correctly rejected (deterministic)
- Result: PASS (exit 0)

## Decision
Kill criterion NOT met. Phase 5 write-intent moat stays gated ON.
Contingency (worktree-only fallback) not triggered. Tree-sitter extractor
proceeds in Phase 1/5 with TS/Python/Go/Rust first, file-granularity fallback.

## Caveat
Harness is Python simulation of tree-sitter scope semantics, not Rust tree-sitter
bindings. Semantics (hash, broker, BEGIN IMMEDIATE) match spec; production
extractor still required in Phase 5.
