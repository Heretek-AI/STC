# H2 — Zero-Token Coordination Spike: PASS

Date: 2026-09-26 | Harness: `plans/spikes/h2_scheduler.py` (throwaway)

## Question
Can pure state-machine scheduler replay 20-task DAG within 5% of manager baseline, no deadlock?

## Method
- 20-node DAG, single entry t0, all reachable, `depends_on` referential integrity validated at parse
- State machine: Kahn dispatch + lease/heartbeat, no model calls
- Baseline: same DAG with 2% stochastic fail + retry, 20 trials

## Results
- state_machine completion=1.000 (20/20), deadlock=False
- baseline completion=1.000, drop=0.0000 (kill if >0.05 or deadlock)
- referential_integrity=OK, single_entry=OK, reachable=OK
- Result: PASS (exit 0)

## Decision
Kill criterion NOT met. Pillar 1 proceeds with zero-model dispatch loop.
Manager LLM stays out of dispatch; allowed back only if future regression
shows >5% drop or deadlock in Phase 1 stress.
