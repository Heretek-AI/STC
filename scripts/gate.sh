#!/usr/bin/env bash
# STC v2 gate order (phase 01). Runs the enforced gate sequence on the v2 tree:
#   format -> fallow (new-findings-only) -> Semgrep Guardian -> tests + tree-sitter
# Clippy is included because the port rule requires ported modules to be
# clippy-clean in the v2 tree. Fail-fast: the first failing gate stops the run.
#
# Exit 0 = all gates clean.
set -uo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT"

step() { echo; echo "=== gate: $1 ==="; }
fail() { echo "GATE FAILED: $1"; exit 1; }

step "format (cargo fmt --check)"
cargo fmt --check || fail "format"

step "fallow audit (new findings only, TS/JS)"
if command -v fallow >/dev/null 2>&1; then
    fallow audit --format compact --quiet || fail "fallow"
else
    echo "fallow not installed; skipping (documented in receipt)"
fi

step "Semgrep Guardian (v2 source tree)"
# `--error` makes any finding exit non-zero (without it, findings exit 0).
SEMGREP_OUT="$(semgrep --config semgrep-rules/stc-guardian.yaml --error studio-core studio-cli 2>&1)"
SEMGREP_RC=$?
printf '%s\n' "$SEMGREP_OUT"
if [ "$SEMGREP_RC" -ne 0 ]; then
    fail "semgrep (exit $SEMGREP_RC: findings or config error)"
fi
# Make the coverage honest: a green PASS must not hide "0 targets scanned".
if printf '%s' "$SEMGREP_OUT" | grep -qE 'Ran [0-9]+ rules on 0 files|Targets scanned: 0|Nothing to scan'; then
    echo "SEMGREP COVERAGE WARNING: 0 targets scanned for studio-core/studio-cli — Rust is NOT covered by these rules"
fi
SEMGREP_TARGETS="$(printf '%s' "$SEMGREP_OUT" | sed -nE 's/.*Rules run: ([0-9]+).*/rules=\1/p; s/.*Targets scanned: ([0-9]+).*/targets=\1/p' | tr '\n' ' ')"
if [ -n "$SEMGREP_TARGETS" ]; then
    echo "SEMGREP COVERAGE: $SEMGREP_TARGETS (studio-core + studio-cli)"
fi
if ! printf '%s' "$SEMGREP_OUT" | grep -qE 'Ran [0-9]+ rules'; then
    echo "SEMGREP COVERAGE: no scan summary parsed — treating as uncovered (informational)"
fi

step "clippy -D warnings (all targets)"
cargo clippy --all-targets -- -D warnings || fail "clippy"

step "tests + tree-sitter syntax gate"
cargo test || fail "tests"

echo
echo "GATE ORDER: PASS"
