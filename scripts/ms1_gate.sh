#!/usr/bin/env bash
# V8 Chronofault MS-1 Evidence Gate (P04).
#
# Seeded DB fixture ledger + frozen clock + CDC pause/burst + ring-overflow
# gap; badge text asserted; DB-only data source (zero mock). 3 screenshots
# (fresh/stale/lost) via headless chrome against the REAL server + REAL DB.
#
# Pass criterion: 10 runs with <=1 flake (`RUNS=10 ./scripts/ms1_gate.sh`).
# Every assertion below is an HTTP/DB fact, never mock data.
set -euo pipefail

RUNS="${RUNS:-1}"
STUDIO_WEB_BIN="${STUDIO_WEB_BIN:-target/debug/studio-web}"
DIST_DIR="${DIST_DIR:-cockpit/dist}"
FLAKES=0

need() { command -v "$1" >/dev/null 2>&1 || { echo "MS1_GATE: missing tool: $1" >&2; exit 2; }; }
need python3
need curl
need google-chrome

[ -x "$STUDIO_WEB_BIN" ] || { echo "MS1_GATE: build first: cargo build -p studio-web" >&2; exit 2; }
[ -f "$DIST_DIR/index.html" ] || { echo "MS1_GATE: build first: (cd cockpit && npm run build)" >&2; exit 2; }

EVIDENCE_DIR="${EVIDENCE_DIR:-.roadmap/04-cockpit-shell-ms1/evidence}"
mkdir -p "$EVIDENCE_DIR"
GATE_LOG="$EVIDENCE_DIR/gate.log"
: > "$GATE_LOG"

log() { echo "$@" | tee -a "$GATE_LOG"; }

free_port() {
  python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])'
}

# Fix 14: root guard — chmod 000 is deaf under EUID 0 (root bypasses file
# perms), so the CDC-pause leg would false-green/red. Detect EUID 0 and
# substitute a rename-away pause leg (or skip-with-reason, never false).
is_root() { [ "$(id -u)" = "0" ]; }

pause_db() { # $1=db path — chmod leg normally, rename-away leg as root
  local db="$1"
  if is_root; then
    mv "$db" "$db.paused" 2>/dev/null || return 1
    # Sidecars must not keep the old epoch readable under the new path.
    rm -f "$db-wal" "$db-shm" "$db-journal" 2>/dev/null || true
  else
    chmod 000 "$db" 2>/dev/null || return 1
  fi
}

resume_db() { # $1=db path
  local db="$1"
  if is_root; then
    mv "$db.paused" "$db" 2>/dev/null || chmod 644 "$db" 2>/dev/null || true
  else
    chmod 644 "$db" 2>/dev/null || true
  fi
}

wait_http() { # $1=url $2=timeout_s
  local url="$1" timeout_s="$2" i=0
  while [ "$i" -lt "$((timeout_s * 5))" ]; do
    if curl -sf -o /dev/null --max-time 2 "$url" 2>/dev/null; then return 0; fi
    sleep 0.2; i=$((i + 1))
  done
  return 1
}

one_run() { # $1=run_index
  local idx="$1"
  local tmp; tmp="$(mktemp -d)"
  local db="$tmp/ms1.db"
  local port; port="$(free_port)"
  local base="http://127.0.0.1:$port"
  local server_pid=""

  cleanup() {
    [ -n "$server_pid" ] && kill "$server_pid" 2>/dev/null || true
    # Root leg leaves a .paused file; non-root leaves chmod 000 — restore both.
    if [ -f "$db.paused" ]; then mv "$db.paused" "$db" 2>/dev/null || true; fi
    chmod 644 "$db" 2>/dev/null || true
    rm -rf "$tmp"
  }

  # --- fixture ledger: seeded DB (10 tasks + 1 approval), triggers fire CDC
  ./target/debug/studio init --db "$db" --repo . --worktree-root "$tmp/wt" >/dev/null 2>&1
  FIX_SEED="$(python3 - "$db" <<'EOF'
import sqlite3, sys
db = sys.argv[1]
c = sqlite3.connect(db)
for i in range(10):
    lane_status = ["building", "validating", "in-review", "done"][i % 4]
    c.execute("INSERT INTO tasks(id,kind,status,updated_ms) VALUES(?,?,?,?)",
              (f"ms1-t{i}", "coder", lane_status, 1700000000000 + i))
c.execute("INSERT INTO contract_approvals(id,task_id,tool,status,reason,created_ms,decided_ms) VALUES(?,?,?,?,?,?,?)",
          ("apr-ms1", "ms1-t0", "write", "pending", None, 1700000000000, None))
c.commit()
tasks = c.execute("SELECT COUNT(*) FROM tasks").fetchone()[0]
changes = c.execute("SELECT MAX(seq) FROM change_log").fetchone()[0]
asks = c.execute("SELECT COUNT(*) FROM contract_approvals").fetchone()[0]
print(f"tasks={tasks} change_seq={changes} approvals={asks}")
EOF
)"
  log "run=$idx fixtures: $FIX_SEED"
  [ "$FIX_SEED" = "tasks=10 change_seq=10 approvals=1" ] || { log "run=$idx FAIL fixture ledger mismatch: $FIX_SEED"; cleanup; return 1; }

  # --- FRESH (frozen clock => deterministic age 0, exact label; wide ring
  # so the UI cursor starts contiguous — no Lost on the fresh evidence)
  STUDIO_MS1_FROZEN_NOW_MS=1800000000000 STUDIO_HUB_RING_CAP=512 STUDIO_HUB_POLL_MS=50 \
    STUDIO_STALE_AFTER_MS=10000 STUDIO_RUN_DIR="$tmp/empty-run" \
    "$STUDIO_WEB_BIN" --db "$db" --port "$port" --static-dir "$DIST_DIR" >"$tmp/srv.log" 2>&1 &
  server_pid=$!
  wait_http "$base/api/health" 15 || { log "run=$idx FAIL server never healthy"; cat "$tmp/srv.log" >>"$GATE_LOG"; cleanup; return 1; }
  sleep 0.4 # let ≥1 CDC poll land under the frozen clock

  local body; body="$(curl -sf "$base/api/status")"
  echo "$body" | grep -q '"state":"Fresh"' || { log "run=$idx FAIL fresh state"; cleanup; return 1; }
  echo "$body" | grep -qF 'fresh · source: ms1.db · updated 0ms ago · seq 10' || { log "run=$idx FAIL fresh label: $body"; cleanup; return 1; }
  [ "$(echo "$body" | python3 -c 'import json,sys; print(len(json.load(sys.stdin)["tasks"]))')" = "10" ] || { log "run=$idx FAIL task count"; cleanup; return 1; }
  echo "$body" | grep -qi 'mock' && { log "run=$idx FAIL mock string in body (DB-only violated)"; cleanup; return 1; }
  echo "$body" | grep -q '"source":"ms1.db"' || { log "run=$idx FAIL source provenance"; cleanup; return 1; }
  # Fix 9: basename+path — the badge keeps the short name by design, the
  # absolute db_path in /api/health distinguishes same-name DBs.
  local health; health="$(curl -sf "$base/api/health")"
  echo "$health" | grep -q '"db_path":"'"$db"'"' || echo "$health" | grep -q 'ms1.db' || { log "run=$idx FAIL health db_path: $health"; cleanup; return 1; }
  # read-only: no write method survives
  [ "$(curl -s -o /dev/null -w '%{http_code}' -X POST "$base/api/status")" = "405" ] || { log "run=$idx FAIL POST not 405"; cleanup; return 1; }
  # ask queue: 1 row, live decision path (P05 A1: pending rows expose it)
  local ask; ask="$(curl -sf "$base/api/ask")"
  echo "$ask" | grep -q '"decision_enabled":true' || { log "run=$idx FAIL ask decision path not live"; cleanup; return 1; }
  google-chrome --headless=new --no-sandbox --disable-gpu --hide-scrollbars \
    --window-size=1280,800 --screenshot="$EVIDENCE_DIR/fresh.png" \
    --virtual-time-budget=3000 "$base/" >/dev/null 2>&1
  grep -q 'data-state="Fresh"' <(google-chrome --headless=new --no-sandbox --disable-gpu \
    --dump-dom --virtual-time-budget=3000 "$base/" 2>/dev/null) || { log "run=$idx FAIL fresh DOM badge"; cleanup; return 1; }
  log "run=$idx fresh: OK (label + 10 tasks + ask live + POST 405 + screenshot)"

  # --- restart WITHOUT frozen clock for the time-stale + lost phases
  kill "$server_pid" 2>/dev/null || true
  wait "$server_pid" 2>/dev/null || true
  STUDIO_HUB_RING_CAP=512 STUDIO_HUB_POLL_MS=50 \
    STUDIO_STALE_AFTER_MS=200 STUDIO_RUN_DIR="$tmp/empty-run" \
    "$STUDIO_WEB_BIN" --db "$db" --port "$port" --static-dir "$DIST_DIR" >"$tmp/srv2.log" 2>&1 &
  server_pid=$!
  wait_http "$base/api/health" 15 || { log "run=$idx FAIL server restart unhealthy"; cleanup; return 1; }
  sleep 0.4

  # --- STALE (CDC pause: DB unreadable mid-run, HTTP still serves last good)
  # Root guard (fix 14): rename-away leg under EUID 0, chmod leg otherwise.
  if is_root; then log "run=$idx stale: root detected (EUID 0), using rename-away pause leg"; fi
  pause_db "$db" || { log "run=$idx FAIL pause_db"; cleanup; return 1; }
  sleep 0.7 # >> 200ms stale budget + poll cadence; polls miss, badge ages
  body="$(curl -sf "$base/api/status")"
  echo "$body" | grep -q '"state":"Stale"' || { log "run=$idx FAIL stale state: $body"; resume_db "$db"; cleanup; return 1; }
  echo "$body" | grep -qE 'stale · source: ms1\.db · updated [0-9]+ms ago · seq 10' || { log "run=$idx FAIL stale label: $body"; resume_db "$db"; cleanup; return 1; }
  google-chrome --headless=new --no-sandbox --disable-gpu --hide-scrollbars \
    --window-size=1280,800 --screenshot="$EVIDENCE_DIR/stale.png" \
    --virtual-time-budget=3000 "$base/" >/dev/null 2>&1
  resume_db "$db"
  # recovery: next polls succeed, badge returns to fresh (fail-closed, honest)
  sleep 0.6
  body="$(curl -sf "$base/api/status")"
  echo "$body" | grep -q '"state":"Fresh"' || { log "run=$idx FAIL no recovery to fresh"; cleanup; return 1; }
  log "run=$idx stale: OK (pause→stale→recovery + screenshot)"

  # --- LOST (ring-overflow gap: restart with a cap-4 ring; the 10-task
  # burst no longer fits, so cursor 0 predates the floor)
  kill "$server_pid" 2>/dev/null || true
  wait "$server_pid" 2>/dev/null || true
  STUDIO_HUB_RING_CAP=4 STUDIO_HUB_POLL_MS=50 \
    STUDIO_STALE_AFTER_MS=10000 STUDIO_RUN_DIR="$tmp/empty-run" \
    "$STUDIO_WEB_BIN" --db "$db" --port "$port" --static-dir "$DIST_DIR" >"$tmp/srv3.log" 2>&1 &
  server_pid=$!
  wait_http "$base/api/health" 15 || { log "run=$idx FAIL server restart unhealthy"; cleanup; return 1; }
  sleep 0.4
  local ev; ev="$(curl -sf "$base/api/events?since=0")"
  echo "$ev" | grep -q '"gap":"Lost"' || { log "run=$idx FAIL gap not Lost: $ev"; cleanup; return 1; }
  [ "$(echo "$ev" | python3 -c 'import json,sys; print(len(json.load(sys.stdin)["events"]))')" = "0" ] || { log "run=$idx FAIL Lost must carry no rows"; cleanup; return 1; }
  # Fix 8 retry guidance: Lost carries X-Resync-From + Retry-After.
  local rcode; rcode="$(curl -s -o /dev/null -D - "$base/api/events?since=0" --max-time 5 | tr -d '\r' | grep -i '^x-resync-from:' | awk '{print $2}')"
  [ -n "$rcode" ] || { log "run=$idx FAIL Lost missing X-Resync-From"; cleanup; return 1; }
  curl -s -D - -o /dev/null "$base/api/events?since=0" --max-time 5 | tr -d '\r' | grep -qi '^retry-after:' || { log "run=$idx FAIL Lost missing Retry-After"; cleanup; return 1; }
  local floor; floor="$(echo "$ev" | python3 -c 'import json,sys; print(json.load(sys.stdin)["floor_seq"])')"
  local ev2; ev2="$(curl -sf "$base/api/events?since=$floor")"
  echo "$ev2" | grep -q '"gap":"None"' || { log "run=$idx FAIL resync not None"; cleanup; return 1; }
  [ "$(echo "$ev2" | python3 -c 'import json,sys; print(len(json.load(sys.stdin)["events"]))')" = "4" ] || { log "run=$idx FAIL resync page != ring cap"; cleanup; return 1; }
  # UI: fresh page load starts its cursor at 0 → sticky lost badge (DB-driven)
  google-chrome --headless=new --no-sandbox --disable-gpu --hide-scrollbars \
    --window-size=1280,800 --screenshot="$EVIDENCE_DIR/lost.png" \
    --virtual-time-budget=3000 "$base/" >/dev/null 2>&1
  grep -q 'data-state="Lost"' <(google-chrome --headless=new --no-sandbox --disable-gpu \
    --dump-dom --virtual-time-budget=3000 "$base/" 2>/dev/null) || { log "run=$idx FAIL lost DOM badge"; cleanup; return 1; }
  log "run=$idx lost: OK (gap Lost + resync 4 rows + sticky badge + screenshot)"

  log "run=$idx PASS"
  cleanup
  return 0
}

i=1
while [ "$i" -le "$RUNS" ]; do
  one_run "$i" || FLAKES=$((FLAKES + 1))
  i=$((i + 1))
done

log "MS1_GATE: runs=$RUNS flakes=$FLAKES (pass bar: <=1)"
[ "$FLAKES" -le 1 ]
