# H-C fidelity protocol (issue #3)

Hypothesis: compiled targets stay within 5% of native (omp-authored)
role-fidelity across 6 roles. Kill: >15% gap → omp sole substrate,
multi-emission deleted.

## Structural leg (DONE, deterministic, zero model spend)

`fidelity_report()` scores each emitted target 0–100: tools exact 40,
prompt verbatim 30, model slot 15, output schema 15. Test
`hc_structural_fidelity_within_hypothesis_band` asserts ≥95 per target
over the flagship + pi library (6 roles).

Measurement history (the spike working as designed):

1. First run: Opencode mirror scored 85 — `model_slot` dropped from the
   mirror front matter (kill-line adjacent). Fixed by emitting the slot.
2. Schema leg added pre-emptively: output schemas now travel in all three
   targets (shim `pi.output_schema`, fenced `output-schema` block in md).
3. Current: 100/100/100 on all 6 roles × 3 targets.

## Behavioral leg (OPEN — live-fire)

Protocol for the model-spend run: per role, N scripted tasks executed once
under the native omp definition and once under each compiled target
(same model, same seed); blind-score task success + constraint adherence;
delta per role. Run when live-fire budget is allocated; record deltas and
the kill decision on issue #3. Structural 100 does not imply behavioral
parity — do not close on structural evidence alone.
