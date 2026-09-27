#!/usr/bin/env bash
# Approval-relay roundtrip spike (issue #5): Ask-policy push -> tap -> ledger.
# Legs: (1) studio-cli approval request (ledger pending), (2) ntfy push publish
# (the phone surface), (3) ntfy poll proving delivery + latency, (4) decision
# recorded to ledger (the one-tap stand-in until the mobile client lands).
# No secrets in any message: approval metadata only.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

CLI="$ROOT/target/debug/studio-cli"
# Default to ntfy.sh (public relay); override with a self-hosted instance,
# e.g. NTFY_SERVER=http://127.0.0.1:18090 (docker: binwiederhier/ntfy serve).
# ntfy.sh throttles cache replays, so delivery proof runs against self-hosted.
NTFY="${NTFY_SERVER:-https://ntfy.sh}"
EVIDENCE="${EVIDENCE:-/tmp/opencode/stc-approval-roundtrip.log}"
mkdir -p "$(dirname "$EVIDENCE")"

{
echo "== build cli =="
cargo build -q -p studio-cli
D=$(mktemp -d)
DB="$D/spike.db"
TOPIC="stc-approval-$(date +%s)-$RANDOM"

echo "== 1. request (ledger pending) =="
APPR=$("$CLI" approval --db "$DB" --task t-spike-1 --action merge.land --detail "2 files")
echo "approval id: $APPR"

echo "== 2. push via ntfy =="
T0=$(date +%s%3N)
curl -s -m 15 -d "STC approval requested: $APPR (merge.land, t-spike-1). Decide: approve|deny." \
  -H "Title: STC approval" -H "Tags: studio,approval" \
  "$NTFY/$TOPIC" >/dev/null
T1=$(date +%s%3N)
echo "publish leg: $((T1 - T0))ms"

echo "== 3. poll proving phone delivery =="
T2=$(date +%s%3N)
# poll=1 returns cached messages immediately and closes (no hanging stream).
MSG=$(timeout 15 curl -s -m 12 "$NTFY/$TOPIC/json?since=all&poll=1" 2>/dev/null | python3 -c \
  "import json,sys
for l in sys.stdin:
    try:
        d = json.loads(l)
    except Exception:
        continue
    if d.get('event') == 'message':
        print(d.get('message', ''))
        break" || true)
T3=$(date +%s%3N)
echo "received: $(echo "$MSG" | head -c 80)..."
echo "poll leg: $((T3 - T2))ms (includes long-poll wait)"
test -n "$MSG" || { echo "FAIL: no push delivered"; exit 1; }
echo "PASS: push delivered to topic"

echo "== 4. one-tap decision to ledger =="
"$CLI" approval-decide --db "$DB" --id "$APPR" --decision approve --comment "spike tap"
"$CLI" approval-decide --db "$DB" --id "$APPR" --decision deny 2>&1 | tail -n 1 || true
echo "== ledger receipts =="
STUDIO_REPO=. "$CLI" snapshot --db "$DB" | python3 -c \
  "import json,sys; d=json.load(sys.stdin); [print(r['id'], r['kind']) for r in d['receipts'] if r['kind'].startswith('approval')]"
rm -rf "$D"
echo "ROUNDTRIP SPIKE PASSED"
} 2>&1 | tee "$EVIDENCE"
