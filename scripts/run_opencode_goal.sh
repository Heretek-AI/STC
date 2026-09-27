#!/usr/bin/env bash
# run_opencode_goal.sh — Launches OpenCode v2 in recursive self-improvement goal mode on STC.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

GOAL_FILE="${SCRIPT_DIR}/OPENCODE_RECURSIVE_GOAL.md"
MODEL="opencode-go/muse-spark-1.3-contributor"

if [ ! -f "${GOAL_FILE}" ]; then
  echo "Error: Goal file not found at ${GOAL_FILE}" >&2
  exit 1
fi

echo "================================================================="
echo " Launching OpenCode v2 Recursive Autonomous Self-Improvement Loop"
echo " Model:    ${MODEL}"
echo " Repo:     ${REPO_ROOT}"
echo " Goal Doc: ${GOAL_FILE}"
echo "================================================================="

cd "${REPO_ROOT}"

# Run OpenCode with auto-approved permissions and standalone server
exec opencode run \
  --auto \
  --standalone \
  -m "${MODEL}" \
  "$(cat "${GOAL_FILE}")"
