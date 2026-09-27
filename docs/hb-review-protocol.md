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

## Pilot result

Task 1 smoke: both lanes 1 loop, caught=1 (generator fixed first try;
reviewer approved with zero corrections). Full 2–4 run in background;
results appended to the issue on completion.

## Scale-up (for the 20-task verdict)

16 more tasks across defect classes (prompt injection, secret leak, race,
N+1, unhandled grammar, XSS, SSRF, ...); same harness `--tasks` filter.
Apply kill criterion on loop reduction + defect-catch delta only at n=20 —
do not close on pilot evidence alone.
