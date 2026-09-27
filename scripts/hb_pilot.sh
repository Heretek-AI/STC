#!/usr/bin/env bash
# H-B pilot harness (issue #2): freeze→burn lane vs live-worktree review lane.
# 4 micro-tasks, each with ONE seeded defect. Generator + reviewer are live
# model calls (opencode run); scoring is deterministic grep on the final file.
# Metrics per task/lane: review loops + seeded-defect caught (0/1).
# Usage: hb_pilot.sh [--model ID] [--tasks 1,2,3,4] [--out DIR]
set -uo pipefail
MODEL="opencode-go/deepseek-v4-flash"
TASKS="1,2,3,4"
OUT=""
while [ $# -gt 0 ]; do
  case "$1" in
    --model) MODEL="$2"; shift 2;;
    --tasks) TASKS="$2"; shift 2;;
    --out) OUT="$2"; shift 2;;
    *) echo "unknown $1"; exit 1;;
  esac
done
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
WORK=$(mktemp -d)
LOG="${OUT:-/tmp/opencode/hb-pilot-$(date +%s)}"
mkdir -p "$LOG"
echo "model=$MODEL tasks=$TASKS log=$LOG" | tee "$LOG/run.log"

gen() { # $1=taskfile $2=spec $3=outfile $4=extra-context
  timeout 240 opencode run --model "$MODEL" --format json \
    "Rewrite the attached Python file so it satisfies this spec: $2. $4 Output ONLY the corrected file in a single python code fence, no prose." \
    -f "$1" > "$WORK/gen.json" 2>"$WORK/gen.err" || return 1
  python3 - "$WORK/gen.json" "$3" <<'EOF'
import json,sys,re
texts = []
for line in open(sys.argv[1]):
    line = line.strip()
    if not line:
        continue
    try:
        d = json.loads(line)
    except Exception:
        continue
    part = d.get("part", {}) if isinstance(d, dict) else {}
    if isinstance(part, dict) and part.get("type") == "text" and part.get("text"):
        texts.append(part["text"])
text = "\n".join(texts) if texts else open(sys.argv[1]).read()
m = re.search(r"```python\n(.*?)```", text, re.S) or re.search(r"```\n(.*?)```", text, re.S)
open(sys.argv[2],"w").write(m.group(1) if m else text)
EOF
}

review() { # $1=candidate $2=classes -> prints APPROVE or FINDINGS
  timeout 240 opencode run --model "$MODEL" \
    "Review the attached Python file for exactly these defect classes: $2. Reply with EXACTLY the word APPROVE if none are present, else lines starting with FINDINGS: describing each." \
    -f "$1" 2>/dev/null | tail -n 20
}

seed_task() { # $1=id -> prints spec|classes|badpattern (pipe-separated)
  case "$1" in
    1) echo "append_to returns acc plus x; default call append_to(1) twice must give [1] then [1]|mutable default argument (def f(x, acc=[]))|acc=\[\]" ;;
    2) echo "total(xs) returns the sum of ALL elements|off-by-one in range() bound|len(xs)-1" ;;
    3) echo "run_expr evaluates arithmetic like '2+3' without executing arbitrary code|eval() on untrusted input|eval(" ;;
    4) echo "load(path) returns file text or raises a clear error, never silently empty|bare except swallowing errors|except.*pass" ;;
  esac
}

write_seed() { # $1=id $2=file
  case "$1" in
    1) printf 'def append_to(x, acc=[]):\n    acc.append(x)\n    return acc\n' > "$2" ;;
    2) printf 'def total(xs):\n    s = 0\n    for i in range(len(xs)-1):\n        s += xs[i]\n    return s\n' > "$2" ;;
    3) printf 'def run_expr(user):\n    return eval(user)\n' > "$2" ;;
    4) printf 'def load(path):\n    try:\n        return open(path).read()\n    except Exception:\n        pass\n' > "$2" ;;
  esac
}

CLASSES="mutable default argument (def f(x, acc=[])); off-by-one in range() bound; eval() on untrusted input; bare except swallowing errors (except: pass)"

for T in 1 2 3 4; do
  case ",$TASKS," in *",$T,"*) ;; *) continue;; esac
  IFS='|' read -r SPEC _ BADPAT <<< "$(seed_task "$T")"
  echo "=== task $T ===" | tee -a "$LOG/run.log"
  # Lane A: live-worktree, up to 2 review loops
  D="$WORK/t$T-A"; mkdir -p "$D"; write_seed "$T" "$D/cand.py"
  LOOPS=0; FEEDBACK=""
  for ROUND in 1 2; do
    gen "$D/cand.py" "$SPEC $FEEDBACK" "$D/cand.py" "" || { echo "task$T A gen FAILED" >> "$LOG/run.log"; break; }
    V=$(review "$D/cand.py" "$CLASSES" || echo "REVIEW-FAILED")
    echo "$V" > "$D/review$ROUND.txt"
    LOOPS=$ROUND
    echo "$V" | grep -q "^APPROVE" && break
    FEEDBACK="Address exactly this review feedback and nothing else: $(grep "^FINDINGS:" "$D/review$ROUND.txt" | head -n 5)"
  done
  if grep -Eq "$BADPAT" "$D/cand.py"; then CAUGHT_A=0; else CAUGHT_A=1; fi
  echo "task$T lane=live loops=$LOOPS caught=$CAUGHT_A" | tee -a "$LOG/run.log"
  # Lane B: freeze -> one bounded correction -> burn
  D="$WORK/t$T-B"; mkdir -p "$D"; write_seed "$T" "$D/cand.py"
  gen "$D/cand.py" "$SPEC" "$D/cand.py" "" || echo "task$T B gen FAILED" >> "$LOG/run.log"
  sha256sum "$D/cand.py" | cut -d' ' -f1 > "$D/frozen.sha"
  V=$(review "$D/cand.py" "$CLASSES" || echo "REVIEW-FAILED")
  echo "$V" > "$D/review.txt"
  if echo "$V" | grep -q "^APPROVE"; then
    echo "task$T B: approved with zero corrections" >> "$LOG/run.log"
  else
    FINDINGS=$(grep "^FINDINGS:" "$D/review.txt" | head -n 5)
    gen "$D/cand.py" "$SPEC Address exactly this review feedback and nothing else: $FINDINGS" "$D/cand.py" "Do not refactor." || echo "task$T B fix FAILED" >> "$LOG/run.log"
  fi
  VERDICT=$(review "$D/cand.py" "$CLASSES" || echo "REVIEW-FAILED")
  echo "$VERDICT" > "$D/verify.txt"
  printf '{"candidate_sha":"%s","loops":1,"verdict":"%s"}\n' "$(cat "$D/frozen.sha")" "$(echo "$VERDICT" | head -n 1)" > "$D/burn.json"
  if grep -Eq "$BADPAT" "$D/cand.py"; then CAUGHT_B=0; else CAUGHT_B=1; fi
  echo "task$T lane=frozen loops=1 caught=$CAUGHT_B" | tee -a "$LOG/run.log"
done
echo "=== summary ===" | tee -a "$LOG/run.log"
grep -E "^task" "$LOG/run.log"
cp -r "$WORK" "$LOG/work" 2>/dev/null || true
echo "H-B PILOT DONE log=$LOG"
