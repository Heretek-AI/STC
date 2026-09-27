#!/usr/bin/env bash
# STC Semgrep Guardian trial (issue #15): one-lane generation-time gate.
# Seeds a vulnerable file + hallucinated dep + hardcoded secret, verifies all
# three blocked with typed reasons, and measures false positives on legit work.
# Kill gate: blocks legit work -> cut. Evidence log for the issue receipt.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

CONFIG="${STC_SEMGREP_CONFIG:-$ROOT/semgrep-rules/stc-guardian.yaml}"
EVIDENCE="${EVIDENCE:-/tmp/opencode/stc-semguard-trial.log}"
mkdir -p "$(dirname "$EVIDENCE")"

{
echo "== Guardian config =="
echo "config: $CONFIG"
semgrep --version 2>/dev/null | head -n 1
LANE="$(mktemp -d)"
echo "lane: $LANE"
printf 'import os\nAPI_KEY = "sk-live-0123456789abcdef"\nimport totally_made_up_pkg_xyz\nuser = input()\neval(user)\n' > "$LANE/seed.py"
printf 'import os\nimport sys\nfrom pathlib import Path\n\ndef legit(path):\n    return Path(path).read_text()\n\nif __name__ == "__main__":\n    print(legit("a.txt"))\n' > "$LANE/legit.py"

echo "== seeded scan (must block x3) =="
SEED_JSON="$LANE/seed.json"
# NOTE: semgrep exits 0 with findings unless --error; the JSON is the verdict.
semgrep --config "$CONFIG" --json --quiet "$LANE/seed.py" > "$SEED_JSON" 2>/dev/null || true
python3 - "$SEED_JSON" <<'EOF'
import json, sys
d = json.load(open(sys.argv[1]))
found = sorted({r["check_id"].split(".")[-1] for r in d.get("results", [])})
print("blocked:", found)
want = ["stc-dangerous-sink", "stc-hallucinated-dep", "stc-hardcoded-secret"]
assert found == want, f"TRIAL FAIL: want {want}, got {found}"
for r in d["results"]:
    print(f"  reason: {r['check_id'].split('.')[-1]} {r['path']}:{r['start']['line']}")
EOF
echo "PASS: seeded vuln + hallucinated dep + hardcoded secret all blocked with typed reasons"

echo "== legit scan (must hold budget) =="
LEGIT_JSON="$LANE/legit.json"
semgrep --config "$CONFIG" --json --quiet "$LANE/legit.py" > "$LEGIT_JSON" 2>/dev/null
python3 - "$LEGIT_JSON" <<'EOF'
import json, sys
d = json.load(open(sys.argv[1]))
n = len(d.get("results", []))
print(f"legit findings: {n}")
assert n == 0, f"TRIAL FAIL: false positives on legit work: {d['results']}"
EOF
echo "PASS: 0 false positives on legit work"

echo "== verdict =="
echo "ADOPT as standard lane gate (scoped trial ruleset; 3/3 blocked, 0 FP)"
rm -rf "$LANE"
echo "SEMGUARD TRIAL PASSED"
} 2>&1 | tee "$EVIDENCE"
