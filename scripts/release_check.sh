#!/usr/bin/env bash
# Release preflight (issue #16): machine checks runnable WITHOUT owner certs.
# Fails closed on: version drift, private key material in repo, unsigned
# artifacts not building. Cert wiring + signed-bundle verification stay
# PENDING-OWNER (see docs/release-checklist.md).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
EVIDENCE="${EVIDENCE:-/tmp/opencode/stc-release-check.log}"
mkdir -p "$(dirname "$EVIDENCE")"

{
echo "== version consistency =="
VERS=$(grep -h '^version = ' Cargo.toml studio-core/Cargo.toml adapters/cli/Cargo.toml tui/Cargo.toml cockpit/src-tauri/Cargo.toml | sort -u)
echo "$VERS"
NVS=$(grep -h '"version"' cockpit/package.json cockpit/src-tauri/tauri.conf.json | grep -o '[0-9][0-9.]*' | sort -u)
echo "$NVS"
if [ "$(echo "$VERS" | wc -l)" != "1" ]; then
  echo "FAIL: Cargo version drift"; exit 1
fi
if [ "$(echo "$NVS" | wc -l)" != "1" ]; then
  echo "FAIL: cockpit version drift"; exit 1
fi
CV=$(echo "$VERS" | grep -o '[0-9][0-9.]*')
NV=$(echo "$NVS" | head -n 1)
if [ "$CV" != "$NV" ]; then
  echo "FAIL: cargo ($CV) != cockpit ($NV)"; exit 1
fi
echo "PASS: all manifests at $CV"

echo "== no private key material in repo =="
# Scoped to release-relevant paths: review/ + .research/ are vendored
# third-party/research data, and the script's own patterns are excluded.
if grep -rIl --exclude-dir=target --exclude-dir=node_modules --exclude-dir=.git \
    --exclude-dir=review --exclude-dir=.research \
    --exclude=release_check.sh \
    -e "BEGIN .*PRIVATE KEY" -e "BEGIN OPENSSH PRIVATE KEY" . ; then
  echo "FAIL: private key material tracked"; exit 1
fi
echo "PASS: no private key material"

echo "== unsigned artifacts build =="
cargo build -q --release -p studio-cli -p studio-tui
echo "PASS: release CLI/TUI binaries"
(cd cockpit && npm run build --silent >/dev/null)
echo "PASS: cockpit frontend dist"

echo "== signing config state =="
if grep -q '"signing"' cockpit/src-tauri/tauri.conf.json; then
  echo "INFO: tauri signing stanza present (owner certs expected)"
else
  echo "INFO: no tauri signing stanza (PENDING-OWNER, blocked:owner)"
fi
echo "RELEASE PREFLIGHT PASSED (signing pending owner certs)"
} 2>&1 | tee "$EVIDENCE"
