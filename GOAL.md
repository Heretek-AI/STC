# GOAL.md — Standing Operating Goals for Agents

These goals apply to every agent working in this repo, on every task, unless the user explicitly overrides them for that task.

## 1. Evidence before claims

Never report done without machine-checkable evidence: test commands + exit codes, artifact paths, gate outputs. Prose completion without evidence is a tap-out — expect a `blocked` receipt and rewind. If blocked twice on the same task, escalate to the manager with a precise blocker statement.

## 2. Smallest verifiable increment

Prefer the smallest change that moves the objective forward with proof: one workstream, one gate, one test run at a time. Temporary rough edges are acceptable while moving in the right direction; completion requires the end state true *and* verified.

## 3. Fail closed, never around

When uncertain — unknown tool, unresolvable path, missing manifest, ambiguous scope — deny with a typed reason and ask. Never guess credentials, never widen permissions to unblock yourself, never downgrade a denial into an approval.

## 4. Leave the tree better than the brief

New code ships with tests. Fixed bugs ship with regression tests. Touched modules stay clippy-clean and rustfmt-clean. Findings that reveal systemic gaps (phantom drift, backend quirks, gate holes) get documented where the next agent will find them — code comments for why, commit messages for what.

## 5. Respect the autonomy dial

- **Advisory mode**: pause for approval at scope boundaries, dispatch, and merge. Never proceed past a gate requiring human tap.
- **Full-authority mode**: act, but the audit log is the safety net — every consequential action must leave a traceable receipt (ledger event, DoD record, or burned ack).
- Destructive operations (deploy, secret rotation, infra mutation, incident-resolve) always require explicit human approval, regardless of mode.

## 6. Scope discipline

Stay inside the four pillars and the tasked workstream. No marketplace, cloud hosting, mobile, or voice code. No harness OAuth reimplementation. No new dependencies without checking the lockfile impact. Kill-gated spikes that miss get cut, not extended.

## 7. Verify against reality, not memory

Re-read files before asserting about them. Run commands instead of predicting output. Treat a green test suite as the floor, not the ceiling. Uncertainty, missing evidence, or weak coverage counts as *not achieved*.
