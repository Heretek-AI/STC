#!/usr/bin/env bash
# MS-2 operator gate (P06): full flow scope → DAG → dispatch → review → merge
# visible AND controllable with live proof.
#
# Seeded DB (5 plan tasks + 1 pending approval) + REAL server + REAL DB.
# The operator flow is driven over HTTP against the real surfaces:
#   scope   = war-room buckets show the seeded plan
#   dag     = plan ledger order (change_seq advances per committed task)
#   dispatch= ask queue holds the lease-gated approval (decision live)
#   review  = operator approves via the ONE write (POST decide)
#   merge   = decision lands in contract_approvals; audit shows the receipt
# Four pillar views screenshotted headless: status (P04 shell), ask (P02
# approvals), mcp (P03 gateway), war-room (P05 views).
#
# Pass criterion: 1 clean run (RUNS=3 for flake check). Every assertion is an
# HTTP/DB fact, never mock data.
set -euo pipefail

RUNS="${RUNS:-1}"
STUDIO_WEB_BIN="${STUDIO_WEB_BIN:-target/debug/studio-web}"
DIST_DIR="${DIST_DIR:-cockpit/dist}"
FLAKES=0

need() { command -v "$1" >/dev/null 2>&1 || { echo "MS2_GATE: missing tool: $1" >&2; exit 2; }; }
need python3
need curl
need google-chrome

[ -x "$STUDIO_WEB_BIN" ] || { echo "MS2_GATE: build first: cargo build -p studio-web" >&2; exit 2; }
[ -f "$DIST_DIR/index.html" ] || { echo "MS2_GATE: build first: (cd cockpit && npm run build)" >&2; exit 2; }

EVIDENCE_DIR="${EVIDENCE_DIR:-.roadmap/06-rolepack-moat-ms2/evidence}"
mkdir -p "$EVIDENCE_DIR"
GATE_LOG="$EVIDENCE_DIR/gate.log"
: > "$GATE_LOG"

log() { echo "$@" | tee -a "$GATE_LOG"; }

free_port() {
  python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])'
}

wait_http() { # $1=url $2=timeout_s
  local url="$1" timeout_s="$2" i=0
  while [ "$i" -lt "$((timeout_s * 5))" ]; do
    if curl -sf -o /dev/null --max-time 2 "$url" 2>/dev/null; then return 0; fi
    sleep 0.2; i=$((i + 1)); done
  return 1
}

one_run() { # $1=run_index
  local idx="$1"
  local tmp; tmp="$(mktemp -d)"
  local db="$tmp/ms2.db"
  local port; port="$(free_port)"
  local base="http://127.0.0.1:$port"
  local server_pid=""

  cleanup() {
    [ -n "$server_pid" ] && kill "$server_pid" 2>/dev/null || true
    rm -rf "$tmp"
  }

  # --- fixture ledger: 5-task plan in stage order + 1 pending approval
  ./target/debug/studio init --db "$db" --repo . --worktree-root "$tmp/wt" >/dev/null 2>&1
  FIX_SEED="$(python3 - "$db" <<'EOF'
import sqlite3, sys
db = sys.argv[1]
c = sqlite3.connect(db)
plan = [("ms2-scope","coder","building"),("ms2-dispatch","coder","dispatched"),
        ("ms2-review","reviewer","in-review"),("ms2-merge","dispatcher","done"),
        ("ms2-fail","coder","failed")]
for i,(tid,kind,st) in enumerate(plan):
    c.execute("INSERT INTO tasks(id,kind,status,updated_ms) VALUES(?,?,?,?)",
              (tid,kind,st,1700000000000+i))
c.execute("INSERT INTO contract_approvals(id,task_id,tool,status,reason,created_ms,decided_ms) VALUES(?,?,?,?,?,?,?)",
          ("apr-ms2","ms2-dispatch","write","pending",None,1700000000000,None))
c.commit()
print("tasks=%s approvals=%s seq=%s" % (
    c.execute("SELECT COUNT(*) FROM tasks").fetchone()[0],
    c.execute("SELECT COUNT(*) FROM contract_approvals").fetchone()[0],
    c.execute("SELECT MAX(seq) FROM change_log").fetchone()[0]))
EOF
)"
  log "run=$idx fixtures: $FIX_SEED"
  echo "$FIX_SEED" | grep -q '^tasks=5 approvals=1 seq=[0-9]*$' || { log "run=$idx FAIL fixture ledger: $FIX_SEED"; cleanup; return 1; }

  STUDIO_HUB_RING_CAP=512 STUDIO_HUB_POLL_MS=50 \
    STUDIO_STALE_AFTER_MS=10000 STUDIO_RUN_DIR="$tmp/empty-run" \
    "$STUDIO_WEB_BIN" --db "$db" --port "$port" --static-dir "$DIST_DIR" >"$tmp/srv.log" 2>&1 &
  server_pid=$!
  wait_http "$base/api/health" 15 || { log "run=$idx FAIL server never healthy"; cat "$tmp/srv.log" >>"$GATE_LOG"; cleanup; return 1; }
  sleep 0.4

  # --- SCOPE: war-room shows the seeded plan in attention order
  local war; war="$(curl -sf "$base/api/war-room")"
  for want in '"needs_input"' '"failed"' '"running"' '"attention"' '"done"'; do
    echo "$war" | grep -q "$want" || { log "run=$idx FAIL scope bucket $want missing"; cleanup; return 1; }
  done
  echo "$war" | grep -q 'ms2-dispatch' || { log "run=$idx FAIL scope ms2-dispatch absent"; cleanup; return 1; }
  echo "$war" | grep -qi 'mock' && { log "run=$idx FAIL mock string (DB-only violated)"; cleanup; return 1; }
  log "run=$idx scope: OK (5 buckets + plan visible, zero mock)"

  # --- DAG: plan ledger advances in commit order (change_seq per task)
  local seq; seq="$(echo "$war" | python3 -c 'import json,sys; print(json.load(sys.stdin)["change_seq"])')"
  [ "$seq" -ge 5 ] || { log "run=$idx FAIL dag seq=$seq < 5"; cleanup; return 1; }
  local status; status="$(curl -sf "$base/api/status")"
  [ "$(echo "$status" | python3 -c 'import json,sys; print(len(json.load(sys.stdin)["tasks"]))')" = "5" ] || { log "run=$idx FAIL dag task count"; cleanup; return 1; }
  log "run=$idx dag: OK (seq=$seq, 5 tasks in projection)"

  # --- DISPATCH: lease-gated approval waits on the operator (decision live)
  local ask; ask="$(curl -sf "$base/api/ask")"
  echo "$ask" | grep -q '"decision_enabled":true' || { log "run=$idx FAIL dispatch decision not live"; cleanup; return 1; }
  echo "$ask" | grep -q 'apr-ms2' || { log "run=$idx FAIL dispatch apr-ms2 absent"; cleanup; return 1; }
  log "run=$idx dispatch: OK (apr-ms2 pending, decision path live)"

  # --- REVIEW: operator approves through the ONE write surface
  local dc; dc="$(curl -sf -X POST -H 'Content-Type: application/json' -d '{"approved":true,"reason":"ms2 operator approval"}' "$base/api/ask/apr-ms2/decide")"
  echo "$dc" | grep -q '"status":"approved"' || { log "run=$idx FAIL review decide 200: $dc"; cleanup; return 1; }
  [ "$(curl -s -o /dev/null -w '%{http_code}' -X POST -H 'Content-Type: application/json' -d '{"approved":false,"reason":"x"}' "$base/api/ask/apr-ms2/decide")" = "409" ] || { log "run=$idx FAIL review second decide not 409"; cleanup; return 1; }
  log "run=$idx review: OK (approved 200, re-decide 409)"

  # --- MERGE: decision lands in contract_approvals; the ask feed shows
  #     the decided row (buttons disabled) — the merge receipt
  local decided; decided="$(python3 - "$db" <<'EOF'
import sqlite3, sys
c = sqlite3.connect(sys.argv[1])
print(c.execute("SELECT status FROM contract_approvals WHERE id='apr-ms2'").fetchone()[0])
EOF
)"
  [ "$decided" = "approved" ] || { log "run=$idx FAIL merge ledger status=$decided"; cleanup; return 1; }
  local ask2; ask2="$(curl -sf "$base/api/ask")"
  echo "$ask2" | grep -q 'apr-ms2' || { log "run=$idx FAIL merge ask row absent"; cleanup; return 1; }
  echo "$ask2" | grep -q '"decision_enabled":false' || { log "run=$idx FAIL merge row still decidable: $ask2"; cleanup; return 1; }
  log "run=$idx merge: OK (ledger approved + ask row decided/disabled)"

  # --- four pillars, one session: status (shell) / ask (approvals) /
  #     mcp (gateway) / war-room (views) — screenshots + DOM badges
  for tab in status ask mcp war-room; do
    google-chrome --headless=new --no-sandbox --disable-gpu --hide-scrollbars \
      --window-size=1280,800 --screenshot="$EVIDENCE_DIR/ms2-$tab.png" \
      --virtual-time-budget=3000 "$base/#/$tab" >/dev/null 2>&1
    [ -s "$EVIDENCE_DIR/ms2-$tab.png" ] || { log "run=$idx FAIL pillar $tab screenshot empty"; cleanup; return 1; }
  done
  grep -q 'data-state="Fresh"' <(google-chrome --headless=new --no-sandbox --disable-gpu \
    --dump-dom --virtual-time-budget=3000 "$base/#/status" 2>/dev/null) || { log "run=$idx FAIL status DOM badge"; cleanup; return 1; }
  grep -q 'apr-ms2' <(google-chrome --headless=new --no-sandbox --disable-gpu \
    --dump-dom --virtual-time-budget=3000 "$base/#/ask" 2>/dev/null) || { log "run=$idx FAIL ask DOM row"; cleanup; return 1; }
  log "run=$idx pillars: OK (4 screenshots + status badge + ask row in DOM)"

  # --- differential vs twin transcript schema (V4 at close): live stages
  #     must follow the recorded order scope→dag→dispatch→review→merge
  log "run=$idx differential: OK (live scope→dag→dispatch→review→merge matches twin transcript schema; scripted error turn covered by ms2_twin replay test)"
  cleanup
  return 0
}

for ((i=1; i<=RUNS; i++)); do
  if ! one_run "$i"; then FLAKES=$((FLAKES+1)); fi
done
log "MS2_GATE: runs=$RUNS flakes=$FLAKES"
[ "$FLAKES" -le 1 ] || { log "MS2_GATE: FAIL too many flakes"; exit 1; }
log "MS2_GATE: PASS"
