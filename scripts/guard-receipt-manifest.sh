#!/usr/bin/env bash
# V02-D/H4: receipt hash-manifest guard (hardening-only, no new features).
#
# What it guards:
#  1. Guarded dossiers byte-pinned — staged edits to `.roadmap/*/dossier.json`,
#     `.roadmap/*/GOAL.md`, `.roadmap/.hash-manifest.json` fail closed unless
#     `STC_WAIVER=1` (manager sign-off). Unstaged/unmodified trees pass with
#     no false positives (empty staged set → exit 0).
#  2. Receipt cites its phase evidence hashes — the given PHASE-RECEIPT.md
#     must contain every required hash passed via --hash (repeatable) or the
#     harden-02-contract default set.
#  3. No GPL/AGPL copy in runtime source — `grep -ri` for GPL/AGPL markers in
#     `studio-core/src` must be empty (clean-room H1-H4: MIT/Apache-2.0 only).
#
# Budget: pure bash+grep+git, no network, target <15s prek budget (measures
# ~0.1s on this tree; prints elapsed to prove it).
# Usage:
#   scripts/guard-receipt-manifest.sh [--receipt PATH] [--hash H]...
# Exit 0 = guarded and clean; exit 1 = typed refusal naming the gate.
set -uo pipefail

RECEIPT=".roadmap/harden-02-contract/PHASE-RECEIPT.md"
HASHES=()
WAIVER="${STC_WAIVER:-0}"

while [ $# -gt 0 ]; do
  case "$1" in
    --receipt) RECEIPT="$2"; shift 2 ;;
    --hash) HASHES+=("$2"); shift 2 ;;
    *) echo "guard-receipt-manifest: unknown arg $1" >&2; exit 1 ;;
  esac
done

# Default harden-02-contract phase evidence set (from the phase brief).
if [ "${#HASHES[@]}" -eq 0 ]; then
  HASHES=(
    "6428fabc3f97768b147aac5c3eedf25d449113363443407d7108adee7360c20d"
    "12ecdfecb24f09aa95ae63d6808238cb024936d7dfb0859ffbe3e0a1e752a807"
    "34ed1c89b045510a716df904121dda35d028d57f2ef379278cb9f67f5783bc54"
    "0aa69bbb624ae9911e99f86113ee17657d419f8d86d7633aa2e7abb5de7254cb"
    "44daa0ca810dd5834dffadfe8f88a1f994e645fe426d0578f5e8b5279fa69994"
    "b153ec9c22362e761aeaff90b00a25ea065bcbc5cf82e47255dc060da108b49b"
  )
fi

T0=$(date +%s%N 2>/dev/null || date +%s)
fail() { echo "guard-receipt-manifest REFUSED ($1): $2" >&2; exit 1; }

# Gate 1: guarded dossiers must not be staged without waiver.
if [ "$WAIVER" != "1" ]; then
  staged=$(git diff --cached --name-only --diff-filter=ACM 2>/dev/null || true)
  hit=$(printf '%s\n' "$staged" | grep -E '^\.roadmap/(.*/dossier\.json|.*/GOAL\.md|\.hash-manifest\.json)$' || true)
  if [ -n "$hit" ]; then
    fail "guarded-dossier" "staged edits touch byte-pinned dossiers (waiver STC_WAIVER=1 required): $(printf '%s' "$hit" | tr '\n' ' ')"
  fi
else
  echo "guard-receipt-manifest: waiver active (STC_WAIVER=1), dossier gate skipped"
fi

# Gate 2: receipt cites every required hash (exact substring, no false positives:
# only the receipt file is scanned, guarded dossiers are never scanned).
if [ ! -f "$RECEIPT" ]; then
  fail "receipt-missing" "receipt not found: $RECEIPT"
fi
missing=0
for h in "${HASHES[@]}"; do
  if ! grep -qF "$h" "$RECEIPT"; then
    echo "guard-receipt-manifest REFUSED (hash-missing): receipt lacks $h" >&2
    missing=1
  fi
done
[ "$missing" -eq 0 ] || exit 1

# Gate 3: no GPL/AGPL markers in runtime source (clean-room, SPDX MIT/Apache-2.0).
if grep -rniE 'general public license|affero|AGPL|GPL-3|GPLv3' studio-core/src >/dev/null 2>&1; then
  fail "license" "GPL/AGPL marker found in studio-core/src (clean-room violation)"
fi

T1=$(date +%s%N 2>/dev/null || date +%s)
# Elapsed print best-effort (ns or s fallback).
if printf '%s' "$T0" | grep -qE '^[0-9]{12,}$'; then
  EL_MS=$(( (T1 - T0) / 1000000 ))
  echo "guard-receipt-manifest PASS: receipt=$RECEIPT hashes=${#HASHES[@]} elapsed=${EL_MS}ms (budget 15000ms)"
else
  echo "guard-receipt-manifest PASS: receipt=$RECEIPT hashes=${#HASHES[@]}"
fi
exit 0
