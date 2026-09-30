#!/usr/bin/env bash
# V8 Chronofault P05 Evidence Gate (phase 05-cockpit-views).
#
# Extends the ms1_gate.sh pattern per view (V8): seeded DB fixture ledger +
# frozen clock + screenshots; fresh/stale/lost assertions on the streamed
# views (status, agent-stream), fail-closed 503 + recovery on the direct
# views, and the A1 decide round-trip (200 → 409 → 404 → 400 typed).
# DB-only data source (zero mock): any "mock" string in a body fails.
# Screenshots: one per operator view via hash routes (#/war-room …).
#
# Pass criterion: 3 runs with 0 flakes (`RUNS=3 ./scripts/p05_gate.sh`).
set -euo pipefail

RUNS="${RUNS:-1}"
STUDIO_BIN="${STUDIO_BIN:-./target/debug/studio}"
STUDIO_WEB_BIN="${STUDIO_WEB_BIN:-target/debug/studio-web}"
DIST_DIR="${DIST_DIR:-cockpit/dist}"
FLAKES=0

need() { command -v "$1" >/dev/null 2>&1 || { echo "P05_GATE: missing tool: $1" >&2; exit 2; }; }
need python3
need curl
need google-chrome

[ -x "$STUDIO_WEB_BIN" ] || { echo "P05_GATE: build first: cargo build -p studio-web" >&2; exit 2; }
[ -x "$STUDIO_BIN" ] || { echo "P05_GATE: build first: cargo build" >&2; exit 2; }
[ -f "$DIST_DIR/index.html" ] || { echo "P05_GATE: build first: (cd cockpit && npm run build)" >&2; exit 2; }

EVIDENCE_DIR="${EVIDENCE_DIR:-.roadmap/05-cockpit-views/evidence}"
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
    sleep 0.2; i=$((i + 1))
  done
  return 1
}

shot() { # $1=base $2=hash $3=out
  google-chrome --headless=new --no-sandbox --disable-gpu --hide-scrollbars \
    --window-size=1280,800 --screenshot="$3" \
    --virtual-time-budget=4000 "$1/$2" >/dev/null 2>&1
}

dom_has() { # $1=base $2=hash $3=grep-pattern
  grep -q "$3" <(google-chrome --headless=new --no-sandbox --disable-gpu \
    --dump-dom --virtual-time-budget=4000 "$1/$2" 2>/dev/null)
}

one_run() { # $1=run_index
  local idx="$1"
  local tmp; tmp="$(mktemp -d)"
  local db="$tmp/p05.db"
  local port; port="$(free_port)"
  local base="http://127.0.0.1:$port"
  local server_pid=""

  cleanup() {
    [ -n "$server_pid" ] && kill "$server_pid" 2>/dev/null || true
    chmod 644 "$db" 2>/dev/null || true
    rm -rf "$tmp"
  }

  # --- fixture ledger: tasks across buckets + approvals + receipts +
  # --- frozen fences + ledger spend + worktree runs + agent events
  "$STUDIO_BIN" init --db "$db" --repo . --worktree-root "$tmp/wt" >/dev/null 2>&1
  FIX_SEED="$(python3 - "$db" <<'EOF'
import sqlite3, sys
db = sys.argv[1]
c = sqlite3.connect(db)
c.execute("INSERT INTO tasks(id,kind,status,updated_ms) VALUES(?,?,?,?)", ("wr-need", "coder", "building", 1700000000000))
c.execute("INSERT INTO tasks(id,kind,status,updated_ms) VALUES(?,?,?,?)", ("wr-fail", "coder", "failed", 1700000000001))
c.execute("INSERT INTO tasks(id,kind,status,updated_ms) VALUES(?,?,?,?)", ("wr-run", "coder", "building", 1700000000002))
c.execute("INSERT INTO tasks(id,kind,status,updated_ms) VALUES(?,?,?,?)", ("wr-attn", "reviewer", "in-review", 1700000000003))
c.execute("INSERT INTO tasks(id,kind,status,updated_ms) VALUES(?,?,?,?)", ("wr-done", "coder", "done", 1700000000004))
c.execute("INSERT INTO contract_approvals(id,task_id,tool,status,reason,created_ms,decided_ms) VALUES(?,?,?,?,?,?,?)",
          ("apr-p05a", "wr-need", "write", "pending", None, 1700000000000, None))
c.execute("INSERT INTO contract_approvals(id,task_id,tool,status,reason,created_ms,decided_ms) VALUES(?,?,?,?,?,?,?)",
          ("apr-p05b", "wr-run", "runProcess", "pending", None, 1700000000001, None))
c.execute("INSERT INTO events(kind,payload,ts_ms) VALUES(?,?,?)", ("message", '{"text":"agent hello"}', 1700000000002))
c.execute("INSERT INTO events(kind,payload,ts_ms) VALUES(?,?,?)", ("hook", '{"provider":"rogue-hook","protocol_version":99}', 1700000000003))
c.execute("INSERT INTO receipts(id,task_id,kind,evidence) VALUES(?,?,?,?)", ("rc-p05", "wr-attn", "review", "diff ok; freeze fh-p05-001"))
c.execute("INSERT INTO frozen_candidates(freeze_hash,lineage_hash,revision,target_ref,targets_json,contents_json,tier,created_ms) VALUES(?,?,?,?,?,?,?,?)",
          ("fh-p05-001", "lin", 1, "refs/heads/x", "[]", "{}", "quick", 1700000000000))
c.execute("INSERT INTO frozen_candidates(freeze_hash,lineage_hash,revision,target_ref,targets_json,contents_json,tier,created_ms) VALUES(?,?,?,?,?,?,?,?)",
          ("fh-p05-002", "lin", 2, "refs/heads/x", "[]", "{}", "full", 1700000000001))
c.execute("INSERT INTO token_ledger(session,amount,cache_hit,billed_kind,ts_ms) VALUES(?,?,?,?,?)", ("gate-sess", 100, 0, "api_key", 1700000000000))
c.execute("INSERT INTO token_ledger(session,amount,cache_hit,billed_kind,ts_ms) VALUES(?,?,?,?,?)", ("gate-sess", 40, 1, "api_key", 1700000000001))
c.execute("INSERT INTO worktrees(path,repo,slug,state,created_ms) VALUES(?,?,?,?,?)", ("/tmp/run-a", "repo", "run-a", "ready", 1700000000000))
c.commit()
tasks = c.execute("SELECT COUNT(*) FROM tasks").fetchone()[0]
changes = c.execute("SELECT MAX(seq) FROM change_log").fetchone()[0]
asks = c.execute("SELECT COUNT(*) FROM contract_approvals").fetchone()[0]
print(f"tasks={tasks} change_seq={changes} approvals={asks}")
EOF
)"
  log "run=$idx fixtures: $FIX_SEED"
  # 5 task inserts + 2 event inserts (P05 trigger fans events into the ring) = 7
  [ "$FIX_SEED" = "tasks=5 change_seq=7 approvals=2" ] || { log "run=$idx FAIL fixture ledger mismatch: $FIX_SEED"; cleanup; return 1; }

  STUDIO_MS1_FROZEN_NOW_MS=1800000000000 STUDIO_HUB_RING_CAP=512 STUDIO_HUB_POLL_MS=50 \
    STUDIO_STALE_AFTER_MS=10000 STUDIO_RUN_DIR="$tmp/empty-run" \
    "$STUDIO_WEB_BIN" --db "$db" --port "$port" --static-dir "$DIST_DIR" >"$tmp/srv.log" 2>&1 &
  server_pid=$!
  wait_http "$base/api/health" 15 || { log "run=$idx FAIL server never healthy"; cat "$tmp/srv.log" >>"$GATE_LOG"; cleanup; return 1; }
  sleep 0.4

  # --- WAR ROOM: bucket order + permission notifications, zero mock
  local wr; wr="$(curl -sf "$base/api/war-room")"
  echo "$wr" | grep -qi 'mock' && { log "run=$idx FAIL mock in war-room"; cleanup; return 1; }
  [ "$(echo "$wr" | python3 -c 'import json,sys; print([b["name"] for b in json.load(sys.stdin)["buckets"]])')" = "['needs_input', 'failed', 'running', 'attention', 'done']" ] || { log "run=$idx FAIL bucket order: $wr"; cleanup; return 1; }
  [ "$(echo "$wr" | python3 -c 'import json,sys; print(len(json.load(sys.stdin)["notifications"]))')" = "2" ] || { log "run=$idx FAIL notifications"; cleanup; return 1; }
  echo "$wr" | grep -q '"state":"Fresh"' || { log "run=$idx FAIL war-room fresh"; cleanup; return 1; }
  log "run=$idx war-room: OK (5 buckets ordered + 2 permission notifications)"

  # --- AGENT STREAM: normalization + unknown-version refusal
  local st; st="$(curl -sf "$base/api/agent-stream")"
  echo "$st" | grep -qi 'mock' && { log "run=$idx FAIL mock in stream"; cleanup; return 1; }
  [ "$(echo "$st" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(sum(1 for e in d["events"] if e["refused"]))')" = "1" ] || { log "run=$idx FAIL refusal count: $st"; cleanup; return 1; }
  echo "$st" | grep -q '"provider":"rogue-hook"' || { log "run=$idx FAIL rogue provider"; cleanup; return 1; }
  echo "$st" | grep -q '"compatible":false' || { log "run=$idx FAIL rogue incompatible"; cleanup; return 1; }
  echo "$st" | grep -q '"gap":"None"' || { log "run=$idx FAIL stream gap"; cleanup; return 1; }
  log "run=$idx agent-stream: OK (1 refused rogue v99 + gap None)"

  # --- AUDIT: kanban evidence + head fence
  local au; au="$(curl -sf "$base/api/audit")"
  echo "$au" | grep -q '"current_head":false' || { log "run=$idx FAIL fence old"; cleanup; return 1; }
  echo "$au" | grep -q '"current_head":true' || { log "run=$idx FAIL fence head"; cleanup; return 1; }
  echo "$au" | grep -q '"task_id":"wr-attn"' || { log "run=$idx FAIL fence linkage"; cleanup; return 1; }
  echo "$au" | grep -q '"evidence":\["rc-p05"\]' || { log "run=$idx FAIL card evidence"; cleanup; return 1; }
  log "run=$idx audit: OK (fence superseded+current + evidence linkage)"

  # --- MCP: detection + explain + flags
  local mc; mc="$(curl -sf "$base/api/mcp")"
  echo "$mc" | grep -q '"manifest_source":"Bundled"' || { log "run=$idx FAIL bundled"; cleanup; return 1; }
  echo "$mc" | grep -q '"manifest_source":"Remote"' || { log "run=$idx FAIL remote status"; cleanup; return 1; }
  [ "$(echo "$mc" | python3 -c 'import json,sys; print(all(a["explain"] for a in json.load(sys.stdin)["agents"]))')" = "True" ] || { log "run=$idx FAIL explain"; cleanup; return 1; }
  log "run=$idx mcp: OK (bundled + remote + explain)"

  # --- PROVIDERS: probe cache + budgets + auth
  local pr; pr="$(curl -sf "$base/api/providers")"
  [ "$(echo "$pr" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(d["lifetime_total"], [b["total"] for b in d["budgets"] if b["session"]=="gate-sess"])')" = "100 [100]" ] || { log "run=$idx FAIL budgets: $pr"; cleanup; return 1; }
  echo "$pr" | grep -q '"ttl_ms":30000' || { log "run=$idx FAIL probe ttl"; cleanup; return 1; }
  echo "$pr" | grep -q '"method":"credential-file"' || { log "run=$idx FAIL auth probes"; cleanup; return 1; }
  # F2: first read re-probes (fresh=false); immediate second read serves the
  # TTL cache (fresh=true, same probe instant).
  echo "$pr" | grep -q '"fresh":false' || { log "run=$idx FAIL first read not re-probed"; cleanup; return 1; }
  local pr2; pr2="$(curl -sf "$base/api/providers")"
  echo "$pr2" | grep -q '"fresh":true' || { log "run=$idx FAIL second read not cached"; cleanup; return 1; }
  [ "$(echo "$pr" | python3 -c 'import json,sys; print([x["last_probe_ms"] for x in json.load(sys.stdin)["probes"]])')" = "$(echo "$pr2" | python3 -c 'import json,sys; print([x["last_probe_ms"] for x in json.load(sys.stdin)["probes"]])')" ] || { log "run=$idx FAIL cache instant moved"; cleanup; return 1; }
  log "run=$idx providers: OK (budgets 100 + TTL cache + auth)"

  # --- RUNS: slots + deferred note
  local rn; rn="$(curl -sf "$base/api/runs")"
  echo "$rn" | grep -q '"slug":"run-a"' || { log "run=$idx FAIL run slot"; cleanup; return 1; }
  echo "$rn" | grep -q 'deferred' || { log "run=$idx FAIL deferred note"; cleanup; return 1; }
  log "run=$idx runs: OK (slot + deferred stated)"

  # --- A1 DECIDE: 200 lands in ledger, then 409 / 404 / 400 typed
  local dc; dc="$(curl -sf -X POST -H 'Content-Type: application/json' -d '{"approved":true,"reason":"gate approval"}' "$base/api/ask/apr-p05a/decide")"
  echo "$dc" | grep -q '"status":"approved"' || { log "run=$idx FAIL decide 200: $dc"; cleanup; return 1; }
  [ "$(curl -s -o /dev/null -w '%{http_code}' -X POST -H 'Content-Type: application/json' -d '{"approved":false,"reason":"x"}' "$base/api/ask/apr-p05a/decide")" = "409" ] || { log "run=$idx FAIL decide not 409"; cleanup; return 1; }
  [ "$(curl -s -o /dev/null -w '%{http_code}' -X POST -H 'Content-Type: application/json' -d '{"approved":true,"reason":"x"}' "$base/api/ask/apr-nope/decide")" = "404" ] || { log "run=$idx FAIL decide not 404"; cleanup; return 1; }
  [ "$(curl -s -o /dev/null -w '%{http_code}' -X POST -H 'Content-Type: application/json' -d '{"approved":true,"reason":""}' "$base/api/ask/apr-p05b/decide")" = "400" ] || { log "run=$idx FAIL decide not 400"; cleanup; return 1; }
  local ask2; ask2="$(curl -sf "$base/api/ask")"
  echo "$ask2" | python3 -c 'import json,sys; d=json.load(sys.stdin); r=[i for i in d["items"] if i["id"]=="apr-p05a"][0]; assert r["status"]=="approved" and r["decision_enabled"] is False' || { log "run=$idx FAIL ledger row"; cleanup; return 1; }
  log "run=$idx decide: OK (200 ledger + 409 + 404 + 400)"

  # --- write surface: everything except decide-POST is 405
  [ "$(curl -s -o /dev/null -w '%{http_code}' -X POST "$base/api/war-room")" = "405" ] || { log "run=$idx FAIL war-room POST not 405"; cleanup; return 1; }
  [ "$(curl -s -o /dev/null -w '%{http_code}' -X DELETE "$base/api/providers")" = "405" ] || { log "run=$idx FAIL providers DELETE not 405"; cleanup; return 1; }
  [ "$(curl -s -o /dev/null -w '%{http_code}' -X PUT "$base/api/ask/apr-p05b/decide")" = "405" ] || { log "run=$idx FAIL decide PUT not 405"; cleanup; return 1; }
  log "run=$idx write-surface: OK (decide-POST only)"

  # --- screenshots + DOM: one per view (hash routes), fresh badge each
  shot "$base" "#/war-room" "$EVIDENCE_DIR/war-room.png"
  dom_has "$base" "#/war-room" 'data-view="war-room"' || { log "run=$idx FAIL war-room DOM"; cleanup; return 1; }
  shot "$base" "#/agent-stream" "$EVIDENCE_DIR/agent-stream.png"
  dom_has "$base" "#/agent-stream" 'data-view="agent-stream"' || { log "run=$idx FAIL stream DOM"; cleanup; return 1; }
  shot "$base" "#/audit" "$EVIDENCE_DIR/audit.png"
  dom_has "$base" "#/audit" 'data-view="audit"' || { log "run=$idx FAIL audit DOM"; cleanup; return 1; }
  shot "$base" "#/mcp" "$EVIDENCE_DIR/mcp.png"
  dom_has "$base" "#/mcp" 'data-view="mcp"' || { log "run=$idx FAIL mcp DOM"; cleanup; return 1; }
  shot "$base" "#/providers" "$EVIDENCE_DIR/providers.png"
  dom_has "$base" "#/providers" 'data-view="providers"' || { log "run=$idx FAIL providers DOM"; cleanup; return 1; }
  shot "$base" "#/runs" "$EVIDENCE_DIR/runs.png"
  dom_has "$base" "#/runs" 'data-view="runs"' || { log "run=$idx FAIL runs DOM"; cleanup; return 1; }
  shot "$base" "#/ask" "$EVIDENCE_DIR/ask.png"
  dom_has "$base" "#/ask" 'data-view="ask"' || { log "run=$idx FAIL ask DOM"; cleanup; return 1; }
  dom_has "$base" "#/war-room" 'data-state="Fresh"' || { log "run=$idx FAIL fresh DOM badge"; cleanup; return 1; }
  log "run=$idx screenshots: OK (7 views + DOM + fresh badge)"

  # --- STALE (hub-backed): CDC pause ages the badge; recovery returns fresh
  kill "$server_pid" 2>/dev/null || true
  wait "$server_pid" 2>/dev/null || true
  STUDIO_HUB_RING_CAP=512 STUDIO_HUB_POLL_MS=50 \
    STUDIO_STALE_AFTER_MS=200 STUDIO_RUN_DIR="$tmp/empty-run" \
    "$STUDIO_WEB_BIN" --db "$db" --port "$port" --static-dir "$DIST_DIR" >"$tmp/srv2.log" 2>&1 &
  server_pid=$!
  wait_http "$base/api/health" 15 || { log "run=$idx FAIL restart unhealthy"; cleanup; return 1; }
  sleep 0.4
  chmod 000 "$db"
  sleep 0.7
  local stale_body; stale_body="$(curl -sf "$base/api/status")"
  echo "$stale_body" | grep -q '"state":"Stale"' || { log "run=$idx FAIL stale state: $stale_body"; chmod 644 "$db"; cleanup; return 1; }
  # Direct views fail closed 503 under DB loss (no stale masquerade)...
  [ "$(curl -s -o /dev/null -w '%{http_code}' "$base/api/war-room")" = "503" ] || { log "run=$idx FAIL war-room not 503 under loss"; chmod 644 "$db"; cleanup; return 1; }
  chmod 644 "$db"
  sleep 0.6 # recovery breath before the F1 probe (it starts from healthy)
  # F1: ...but the SPA keeps last-good data with a non-blocking refresh
  # notice + scoped retry (never a frozen Fresh badge with no affordance).
  # A fresh headless load starts with no last-good state, so this needs a
  # PERSISTENT tab: load healthy, observe data, cut the DB, re-read the DOM.
  # The probe body is written once and driven twice (F1 seeded, F1-empty).
  cat > "$tmp/f1probe.py" <<'PYEOF'
import asyncio, json, subprocess, sys, time, urllib.request
port, db, url, MARKER = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4]
CHROME = "google-chrome"
async def call(ws, mid, method, params):
    await ws.send(json.dumps({"id": mid, "method": method, "params": params}))
    while True:
        msg = json.loads(await ws.recv())
        if msg.get("id") == mid:
            if "error" in msg:
                raise RuntimeError(str(msg["error"]))
            return msg.get("result", {})
async def main():
    import websockets
    proc = subprocess.Popen(
        [CHROME, "--headless=new", "--no-sandbox", "--disable-gpu",
         "--no-first-run", f"--remote-debugging-port={port}", "about:blank"],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        tabs = None
        for _ in range(50):
            try:
                tabs = json.load(urllib.request.urlopen(f"http://127.0.0.1:{port}/json/list", timeout=2))
                break
            except Exception:
                time.sleep(0.2)
        assert tabs, "devtools endpoint never came up"
        page = next(t for t in tabs if t["type"] == "page")
        async with websockets.connect(page["webSocketDebuggerUrl"], max_size=8 * 1024 * 1024) as ws:
            mid = 0
            async def ev(expr):
                nonlocal mid
                mid += 1
                r = await call(ws, mid, "Runtime.evaluate", {"expression": expr, "returnByValue": True})
                return (r.get("result") or {}).get("value", "")
            mid += 1
            await call(ws, mid, "Page.navigate", {"url": url})
            for _ in range(60):
                if MARKER in await ev("document.body.innerText.slice(0,6000)"):
                    break
                await asyncio.sleep(0.25)
            else:
                raise RuntimeError(f"no last-good state observed while healthy (marker {MARKER})")
            import os
            os.chmod(db, 0)
            try:
                await asyncio.sleep(2.5)  # >=2 poll cycles at the 1s cadence
                txt = await ev("document.body.innerText.slice(0,6000)")
                assert "showing last good data" in txt, f"missing last-good notice: {txt[:400]}"
                assert "Retry War Room" in txt, f"missing scoped retry: {txt[:400]}"
            finally:
                os.chmod(db, 0o644)
            print("F1PROBE: last-good notice + scoped retry observed")
    finally:
        proc.terminate()
asyncio.run(main())
PYEOF
  F1_PORT="$(free_port)"
  python3 "$tmp/f1probe.py" "$F1_PORT" "$db" "$base/#/war-room" "needs_input" >>"$GATE_LOG" 2>&1
  grep -q "F1PROBE: last-good notice" "$GATE_LOG" || { log "run=$idx FAIL F1 persistent probe"; chmod 644 "$db"; cleanup; return 1; }
  log "run=$idx F1: OK (persistent tab: last-good notice + scoped retry)"
  # F1-empty: same shape on an EMPTY board — fresh init, no seeds: healthy
  # empty state ("No tasks in any bucket"), then chmod, then the notice +
  # scoped retry must appear above the empty copy (own server + empty DB).
  local emptydb port2 base2 empty_pid=""
  emptydb="$tmp/empty.db"
  port2="$(free_port)"
  base2="http://127.0.0.1:$port2"
  "$STUDIO_BIN" init --db "$emptydb" --repo . --worktree-root "$tmp/wt-empty" >/dev/null 2>&1
  STUDIO_MS1_FROZEN_NOW_MS=1800000000000 STUDIO_HUB_RING_CAP=512 STUDIO_HUB_POLL_MS=50 \
    STUDIO_STALE_AFTER_MS=10000 STUDIO_RUN_DIR="$tmp/empty-run" \
    "$STUDIO_WEB_BIN" --db "$emptydb" --port "$port2" --static-dir "$DIST_DIR" >"$tmp/srv-empty.log" 2>&1 &
  empty_pid=$!
  wait_http "$base2/api/health" 15 || { log "run=$idx FAIL empty server unhealthy"; kill "$empty_pid" 2>/dev/null || true; cleanup; return 1; }
  sleep 0.4
  [ "$(curl -sf "$base2/api/war-room" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(sum(len(b["tasks"]) for b in d["buckets"]), len(d["notifications"]))')" = "0 0" ] || { log "run=$idx FAIL empty board not empty"; kill "$empty_pid" 2>/dev/null || true; cleanup; return 1; }
  F1_PORT="$(free_port)"
  : > "$tmp/f1empty.log"
  python3 "$tmp/f1probe.py" "$F1_PORT" "$emptydb" "$base2/#/war-room" "No tasks in any bucket" >>"$tmp/f1empty.log" 2>&1
  cat "$tmp/f1empty.log" >>"$GATE_LOG"
  grep -q "F1PROBE: last-good notice" "$tmp/f1empty.log" || { log "run=$idx FAIL F1-empty probe"; kill "$empty_pid" 2>/dev/null || true; cleanup; return 1; }
  kill "$empty_pid" 2>/dev/null || true
  wait "$empty_pid" 2>/dev/null || true
  log "run=$idx F1-empty: OK (empty board: notice + scoped retry above empty copy)"
  chmod 644 "$db"
  sleep 0.6
  stale_body="$(curl -sf "$base/api/status")"
  echo "$stale_body" | grep -q '"state":"Fresh"' || { log "run=$idx FAIL no recovery"; cleanup; return 1; }
  log "run=$idx stale: OK (pause→stale→recovery + direct 503)"

  # --- LOST (ring overflow): cap-4 ring, cursor 0 predates the floor
  kill "$server_pid" 2>/dev/null || true
  wait "$server_pid" 2>/dev/null || true
  STUDIO_HUB_RING_CAP=4 STUDIO_HUB_POLL_MS=50 \
    STUDIO_STALE_AFTER_MS=10000 STUDIO_RUN_DIR="$tmp/empty-run" \
    "$STUDIO_WEB_BIN" --db "$db" --port "$port" --static-dir "$DIST_DIR" >"$tmp/srv3.log" 2>&1 &
  server_pid=$!
  wait_http "$base/api/health" 15 || { log "run=$idx FAIL restart unhealthy"; cleanup; return 1; }
  sleep 0.4
  local ev; ev="$(curl -sf "$base/api/agent-stream?since=0")"
  echo "$ev" | grep -q '"gap":"Lost"' || { log "run=$idx FAIL stream gap not Lost: $ev"; cleanup; return 1; }
  [ "$(echo "$ev" | python3 -c 'import json,sys; print(len(json.load(sys.stdin)["events"]))')" = "0" ] || { log "run=$idx FAIL Lost must carry no rows"; cleanup; return 1; }
  shot "$base" "#/agent-stream" "$EVIDENCE_DIR/stream-lost.png"
  log "run=$idx lost: OK (gap Lost + no rows + screenshot)"

  log "run=$idx PASS"
  cleanup
  return 0
}

i=1
while [ "$i" -le "$RUNS" ]; do
  one_run "$i" || FLAKES=$((FLAKES + 1))
  i=$((i + 1))
done

log "P05_GATE: runs=$RUNS flakes=$FLAKES (pass bar: 0)"
[ "$FLAKES" -eq 0 ]
