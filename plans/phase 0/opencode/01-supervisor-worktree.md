# Studio — 01 Supervisor, Worktree & State (Phase 2a)

## 1. Supervisor tree / orchestration loop

Roles: Manager (human-facing, scope→epics→DAG) → Dispatcher (capacity/budget/affinity) → Workers (Researcher/Implementer/Auditor). Single active controller per session (AO `CommitControllerEpoch` CAS).

- Spawn: `create row → provision worktree → launch runtime|chat → MarkSpawned` (AO `session_manager/manager.go:376-393,876-992`). Mode preflight with fallback; env `AO_SESSION_ID/AO_RUNTIME_LAUNCH_ID`.
- Dispatch order per sweep (agent-swarm `heartbeat.ts:340-365`): expire runtimes → release dead offers → remediate stalls (no-session 5m fail, stale-HB 15m, fresh 30m record) → health fix busy/idle → paginated assign (50 scan/500 cap/5 per sweep, `emptyPollCount<2` parks) → cleanup + escalate starved.
- Poll-claim atomicity: capacity + budget (`canClaim`) + `UPDATE WHERE unassigned` in one txn; `afterCommit` for telemetry only (`poll.ts:307-591`). Convergence only on taken `activeEdges` (no conditional-branch deadlock).
- Orkas anti-deadlock: per-conversation Mutex + `globalSlots=10` + `workerSlots=4`; nested skips global (parent holds slot) — safe because dispatch tools Commander-only (`locks.ts:132-153`).
- Paseo run-token: `settleForegroundRun(agent,token)` older cancel cannot settle newer turn; ambiguous interrupt blocks replacement/reload/Stop.
- Circuit breaker: 200 calls / 30min / 10x repeat / 5 consec per task; transient 429/503/529/timeout retried x5 before counting; action-local keyed session+invocation+digest+category. PRM L1 advise→L2 alert→L3 halt (repetition/ping-pong/drift/stuck/thrash).
- Budgets: `WORKFLOW_MAX_STEPS_PER_RUN=500`, `MAX_ITERATIONS=100`, per-step 30s `Promise.race`; Orkas `COMMANDER_MAX_TOOL_LOOPS=120/AGENT=100`; Paseo `RELOAD_CLOSE 3000/INTERRUPT 2000`.
- Nudges: `pendingNudge{key,sig,maxAttempts}` dedup `seen[key]==sig`, `reviewMaxNudge=3`, merge-conflict sole `urgent:true`; `cannotNudge=terminated|needsInput|exited` (AO `reactions.go:151-366,1007-1073`).

## 2. Worktree state machine

States: `provisioning → active → dirty-check → stash → rebase/merge → verify → teardown`. Interface (AO `ports/outbound.go:322-351`): `Create/Destroy(refuses dirty)/Restore/ForceDestroy(only after stash)/StashUncommitted→refs/preserved/<sess>/ApplyPreserved(cherry-pick --no-commit)/AddExclude(info/exclude via --git-common-dir)`.

- Identity: locator (`repoId::path`, Orca `id.ts`) ≠ occupant (`wt2:host:instance`, `reservationId+generation+branch`, `refs/ao/preserved/<sess>`). Rename/move preserves occupant.
- Ownership: private `<root>/<hash>/<slug>` (Paseo) or `<project>/.swarm-worktrees` (swarm) + metadata (`paseo/worktree.json`, `.swarm/*-owners`) + gitlink verify. Unknown/unreadable → `uncertain` fail-closed, never delete; refusal text is safety signal.
- Create: `prune` → `check-ref-format` → unique `-N` branch → `worktree add` → verify `rev-parse --git-dir` inside lane → materialize scope/env. Lock classify→cleanup→publish. Timeouts: fetch 10s, mutation 30s, Paseo add/remove 120s, Orca hook 120s. `stdin:ignore`, `LC_ALL:C`, array argv, `kill()` in `finally`.
- Dirty: `status --porcelain` + `ls-files --others` pair; probe-fail=dirty. Stash via temp `GIT_INDEX_FILE` (AO) or auto-commit + overlap fence (swarm snapshots `status v2 -z` + `ls-files --stage -z` + `diff --name-status -z --find-renames <base>..<lane>` + OID `^[0-9a-f]{40|64}$`, must match pre/post) or refuse+preserve (Paseo/munder `keep=dirty||ahead>0`).
- Remove: preflight (unlocked? clean? live PTY? archive hook `exited|unverifiable` both block?) → rename-to-trash/`.discarded` (same FS) → `prune` → verify deregistered → async unlink + sweep on boot. Default `remove` no `--force`; `--force` only after capture (AO), inside trusted base (swarm `isPathUnderSwarmWorktreeBase`), or explicit waiver (Orca). Locked → require `unlock`.
- Merge: canonical `BaseRef` once (`refs/remotes/origin/main`), origin-wins w/ local fallback; stored vs requested mismatch → hard error. `merge --no-edit / rebase / cherry-pick <base>..<branch> -x / squash-unstaged (merge --squash --no-commit → reset HEAD --incoming so staged survives)`. Conflict list `diff --diff-filter=U + ls-files -u + porcelain` → strategy `--abort` → `{conflict:true,files}`. `prune` before `branch -D`. Squash needs content-identity not ancestry (`matchesSourceHeadWorkingTree`).
- Hygiene: retry `rm` (AO 18x 50→500ms ~7.25s, Paseo 5x), background unlink + `WaitGroup`, `prunable` excluded from liveness but retried, migration via `move` never `rm -rf`, per-repo teardown mutex, WSL/SSH path normalize before compare.

## 3. Data & persistence

- SQLite WAL, `BEGIN IMMEDIATE`, `getDbClient().query/get/run/transaction` + `afterCommit` (never microtask). Forward-only `NNN_*.sql`; migrations immutable.
- `change_log(seq,table, row,op,old,new)` triggers → 100ms poller batch 512 → broadcaster sync fan-out panic-isolated → SSE/WS. Boot `SeekToHead`; clients catch up by offset. Janitor 7d/100k batch 10k x8 per 15m.
- Files: `agents/{sanitized-cwd}/{id}.json` atomic temp+rename (Paseo `AgentRecord`); `.swarm/plan-ledger.jsonl` authoritative append-only `seq/hash-chain/source` (`LEDGER_SCHEMA_VERSION 1.1.0`, 13 event types), `plan.json/md` projections, `plan-export/` checkpoints; WALs `PREPARED→COMMITTED|ABORTED` with `transitionId/generation/actor/recordedAt` (`CoderSettlementWal/TaskRepairWal/TaskTerminalWal`); telemetry JSONL fire-and-forget; `skill-usage.jsonl`, `trajectories/<sess>.jsonl`.
- DAG: `WorkflowDefinition{nodes[]{id,type,config,next:string|[]|port-map,inputs: upstream-only,inputSchema,retry}}`; single entry, reachable, executor-registered; `foreach.body.type==agent-task`, no `#` collision. Run/step: `workflow_runs(status running|waiting|completed|failed|cancelled|skipped)`, `steps(...idempotencyKey=run:node:iter UNIQUE)`, `wait_states(mode time|event, 64KB cap)`, `approval_requests`.
- Tokens: billing events `(session,harness,root)→sources(artifact+generation+offset)→model_usage_events(binding,source,provider,model,kind,input,cached,output,cost_nanos,pricing_version,source_event_key)` inferred→observed promotion, rehome on replace, `SUM GROUP BY harness,model`; admission `canClaim(agent,date)` + `budget_refused` trigger; planning `contextBudget()`; backstop 25M hard-stop next turn (in-mem, reset on user msg).
- Handoff artifact: private dir 0600/0700, hard-link immutable publish, sha256 verify after source stop, GC temp/unowned; Chat idempotency key; generation/launch-id fencing; gap outbox `Enqueue/ListPending/MarkDelivered` + `transitionDeliveryWake`.
