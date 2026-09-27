# H-B review-loop protocol (issue #2)

Hypothesis: frozen-candidate + one bounded correction + burned ack gives
≥50% fewer review→fix→re-review loops on 20 tasks vs live-worktree review.
Kill: <25% reduction or defect-catch drop → cut freeze to evidence-only DoD.

## Harness (`scripts/hb_pilot.sh`)

4 micro-tasks, each with ONE seeded defect (mutable-default-arg, off-by-one,
`eval()` on input, swallowed exception). Generator + reviewer are live
`opencode run` calls (flat-rate Go model); scoring is deterministic grep on
the final file (seeded-defect pattern present = missed).

- Lane A (live-worktree): generate → review → fix, up to 2 loops.
- Lane B (freeze→burn): generate once, freeze sha, exactly one correction
  round, burn ack JSON `{candidate_sha, loops: 1, verdict}`, final verify.
- Metrics per task/lane: `loops`, `caught` (0/1).

## Pilot result (2026-09-27, model opencode-go/deepseek-v4-flash)

| Task | Live loops | Live caught | Frozen loops | Frozen caught |
|---|---|---|---|---|
| 1 mutable-default | 1 | 1 | 1 | 1 |
| 2 off-by-one | 1 | 1 | 1 | 1 |
| 3 eval-input | 1 | 1 | 1 | 1 |
| 4 swallowed-except | 1 | 1 | 1 | 1 |

Harness mechanics proven end to end (request→review→fix→verify→burn receipts,
double-tap refusal, grep scoring). **No discriminative signal**: the generator
fixed every seed first try and the reviewer approved with zero corrections, so
loops=1 in both lanes — this says the tasks were too easy, not that the lanes
tie. The 20-task verdict needs harder tasks (multi-defect, subtle logic) and a
stricter reviewer checklist. Infra note: `opencode run` server children ignore
TERM — harness uses `timeout -k 15 240`.

## Scale-up (for the 20-task verdict)

16 more tasks across defect classes (prompt injection, secret leak, race,
N+1, unhandled grammar, XSS, SSRF, ...); same harness `--tasks` filter.
Apply kill criterion on loop reduction + defect-catch delta only at n=20 —
do not close on pilot evidence alone.
