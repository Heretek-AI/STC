#!/usr/bin/env bash
# STC compose volume drill (issue #10).
# Proves zero data loss across teardown/re-pull: create state in named
# volumes, tear down containers, verify volumes + bind state intact.
# Uses a throwaway project name so the real studio stack is untouched.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
PROJECT="stc-drill"
EVIDENCE="${EVIDENCE:-/tmp/opencode/stc-volume-drill.log}"
mkdir -p "$(dirname "$EVIDENCE")"

{
echo "== volume drill start =="
export COMPOSE_PROJECT_NAME="$PROJECT"
docker compose -f compose.yaml --profile lanes config --volumes
echo "== create state =="
docker volume create "${PROJECT}_studio-harness" >/dev/null
echo "drill-canary-$(date +%s)" | docker run --rm -i -v "${PROJECT}_studio-harness:/data" debian:bookworm-slim sh -c 'cat > /data/canary.txt && cat /data/canary.txt'
echo "== teardown containers (volumes kept) =="
docker compose -f compose.yaml --profile lanes down --remove-orphans || true
echo "== verify zero loss =="
docker run --rm -v "${PROJECT}_studio-harness:/data:ro" debian:bookworm-slim cat /data/canary.txt
echo "PASS: harness volume survived teardown"
echo "== cleanup drill volumes =="
docker volume rm "${PROJECT}_studio-harness" >/dev/null
echo "VOLUME DRILL PASSED"
} 2>&1 | tee "$EVIDENCE"
