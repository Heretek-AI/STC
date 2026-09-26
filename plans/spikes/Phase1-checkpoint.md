# Phase 1 Checkpoint (exit gate satisfied 2026-09-26)

## Completed
- Phase 0 H1/H2/H3 PASS with receipts in `plans/spikes/`
- `studio-core` modules: `worktree, state, dag, scheduler, mcp, session, ledger, merge` + `bin/studio`
- `cargo test`: 9 passed
- WorktreeManager: per-repo `WorktreeOperationLock` (tokio Mutex), hashed `<root>/<hash8>/<slug>`,
  create `--detach --no-checkout` -> `reset --hard` -> `lock --reason`,
  destroy fail-closed (`WorkspaceDirty`), `stash_uncommitted` -> `refs/preserved/<sess>`,
  trash-rename + `unlock` + `prune`, `sweep_trash`, `recover` (list-porcelain minus ledger), `list_worktrees`
- State: SQLite WAL, `BEGIN IMMEDIATE`, append-only `token_ledger` (cache_hit separate), `change_log` trigger, `poll_changes`
- DAG: strict `depends_on`/`inputs`/`next` integrity, single entry, reachability
- Scheduler: zero-model state machine, lease/heartbeat, `lease_expired` reap
- Ledger: hash-chain JSONL (`seq`, `plan_hash_before/after`, `payload_hash`, `source`), `plan_created` root, tamper guard
- Merge: Bors turnstile (tokio Mutex), head-SHA fence, `CONFLICT_RETAINED` diagnostic (never destructive reset)
- CLI: `studio status --repo`, `studio logs --db`

## Exit-gate evidence (throwaway harnesses in /tmp, cleaned)
- 10 concurrent creates: CONCURRENT_OK 10/10, 0 `.git/index.lock` collisions
- Dirty destroy refused: FAIL_CLOSED_OK (`WorkspaceDirty`)
- Crash recovery: BEFORE 4 -> REAPED 3 -> AFTER 1, RECOVER_OK 0 orphans; trash sweep clears dirs
- Scheduler: no model calls (pure state machine, H2 PASS)

## Remaining polish (non-blocking for Phase 2 start)
- `plan.json` projection from ledger replay
- Full ACP v1 NDJSON bridge (types only: `SessionBinding`)
- `studio logs --follow` streaming (currently snapshot poll)
