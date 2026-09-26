# H3 — Tool Lens Spike: PASS

Date: 2026-09-26 | Harness: `plans/spikes/h3_tool_lens.py` (throwaway)

## Question
Does schema-slicing proxy approach ~35x token cut with completion within 5%?

## Method
- Full schema ~678 tok/tool (118 tools ≈ 80K, per report)
- Compact index ~20 tok/tool; task needs k=3 tools via `tool_open`
- Suites 50/100/200 tools; manifest `{allow,on-demand,deny}` resolution check

## Results
- n=50: 33900 → 3034 (11.17x); n=100: 16.81x; n=200: 22.47x
- ref118: scripts-only 80000→2200 = 36.4x; per-task slice 80000→4394 = 18.2x
- completion full=1.0 sliced=1.0 drop=0.0 (kill if >0.05)
- Result: PASS (exit 0)

## Decision
Kill criterion NOT met. Proceed with progressive disclosure (Tool Lens) +
role manifests + least-privilege enforcement. Scripts-only fallback not
triggered but retained as contingency. Per-role token scoreboard required
in Phase 2 exit.
