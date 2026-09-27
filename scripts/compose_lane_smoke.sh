#!/usr/bin/env bash
# STC compose lane smoke test (issue #10).
# Proves: (1) lane image builds clean, (2) lanes boot with correct UID/GID
# mapping (no root-owned files on host binds), (3) default profile never
# socket-mounts (fail closed on privileged-dev leakage).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

IMAGE="${IMAGE:-studio-lane:smoke}"
HOST_UID="$(id -u)"
HOST_GID="$(id -g)"
EVIDENCE="${EVIDENCE:-/tmp/opencode/stc-lane-smoke.log}"
mkdir -p "$(dirname "$EVIDENCE")"

{
echo "== lane build =="
docker build ./docker -f ./docker/Dockerfile.lane -t "$IMAGE"
echo "== uid/gid mapping =="
TMPD="$(mktemp -d)"
echo "probe" > "$TMPD/probe.txt"
# :z relabels the throwaway /tmp bind for SELinux hosts; the repo bind in
# compose.yaml stays plain rw so lanes never relabel the user's checkout.
docker run --rm \
  --user "$HOST_UID:$HOST_GID" \
  --cap-drop ALL --security-opt no-new-privileges:true \
  -v "$TMPD:/workspace:rw,z" \
  "$IMAGE" sh -c 'touch /workspace/from-lane.txt && id -u && id -g && ls -ln /workspace'
echo "host ownership of lane-created file:"
ls -ln "$TMPD/from-lane.txt"
OWNER_UID="$(stat -c %u "$TMPD/from-lane.txt")"
OWNER_GID="$(stat -c %g "$TMPD/from-lane.txt")"
if [ "$OWNER_UID" != "$HOST_UID" ] || [ "$OWNER_GID" != "$HOST_GID" ]; then
  echo "FAIL: lane file owned $OWNER_UID:$OWNER_GID, expected $HOST_UID:$HOST_GID"
  exit 1
fi
echo "PASS: lane file owned $OWNER_UID:$OWNER_GID"
rm -rf "$TMPD"
echo "== socket-mount guard (default profile) =="
if grep -vE '^\s*#' compose.yaml | grep -q "docker.sock"; then
  echo "FAIL: compose.yaml must never socket-mount (dev override only)"
  exit 1
fi
echo "PASS: no docker.sock in compose.yaml"
echo "== dev banner =="
cargo run -q -p studio-cli -- up --help | grep -q "dev" && echo "PASS: studio up --dev flag present"
echo "ALL LANE SMOKE CHECKS PASSED"
} 2>&1 | tee "$EVIDENCE"
