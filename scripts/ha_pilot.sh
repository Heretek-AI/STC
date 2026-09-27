#!/usr/bin/env bash
# H-A live-fire harness (issue #1): anchored ≤250-token summaries vs no-memory
# baseline on 20 invented-fact tasks (Frobnicator project — unknowable without
# memory, so the baseline must guess). Retrieval runs the REAL MemoryStore FTS
# path (memory-put / memory-search); answers are live model calls.
# Hypothesis: anchored gives ≥ +5pt task success. Kill: < +2pt → cut rung 3.
# Usage: ha_pilot.sh [--model ID] [--tasks 1..20] [--out DIR]
set -uo pipefail
MODEL="opencode-go/deepseek-v4-flash"
TASKS="1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20"
OUT=""
while [ $# -gt 0 ]; do
  case "$1" in
    --model) MODEL="$2"; shift 2;;
    --tasks) TASKS=$(echo "$2" | tr ',' ' '); shift 2;;
    --out) OUT="$2"; shift 2;;
    *) echo "unknown $1"; exit 1;;
  esac
done
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
cargo build -q -p studio-cli
CLI="$ROOT/target/debug/studio-cli"
D=$(mktemp -d)
DB="$D/ha.db"
LOG="${OUT:-/tmp/opencode/ha-pilot-$(date +%s)}"
mkdir -p "$LOG"
echo "model=$MODEL tasks=$TASKS" | tee "$LOG/run.log"

# Fixture table: id|question|gold entity|fts query|anchored summary (≤250 tok)
read -r -d '' FIXTURES <<'EOF' || true
1|Which Frobnicator module owns retry budgets, and what is the default?|schedulon,7|retry budgets|path: frob/sched/mod.rs | symbol: schedulon | commit: a1b2c3d4 | plan: p01 | Retry budgets are owned by the schedulon module; default is 7 attempts with jitter.
2|What port does the Frobnicator relay listen on?|47107|relay port|path: frob/relay/cfg.rs | symbol: RELAY_PORT | commit: b2c3d4e5 | plan: p02 | The relay listens on port 47107 with TLS required.
3|Who approves Frobnicator production deploys?|quux warden|production deploys|path: frob/ops/policy.md | symbol: quux-warden | commit: c3d4e5f6 | plan: p03 | Production deploys require quux warden approval; no self-approvals.
4|What is the Frobnicator token ledger flush interval?|37 minutes|ledger flushes|path: frob/ledger/store.rs | symbol: FLUSH_SECS | commit: d4e5f607 | plan: p04 | The token ledger flushes every 37 minutes; WAL mode stays on.
5|Which queue backs Frobnicator agent dispatch?|zorp q|agent dispatch|path: frob/dag/queue.rs | symbol: zorp-q | commit: e5f60718 | plan: p05 | Agent dispatch runs on the zorp q queue with at-least-once delivery.
6|What hash does Frobnicator use for content addressing?|quuxhash|content addressing|path: frob/store/hash.rs | symbol: QUUXHASH | commit: f6071829 | plan: p06 | Content addressing uses quuxhash; sha256 only for legacy imports.
7|What is the max lane count per Frobnicator host?|13|lanes host|path: frob/lanes/cfg.rs | symbol: MAX_LANES | commit: 0718293a | plan: p07 | Max 13 lanes per host; the 14th waits for a free slot.
8|Which model slot serves Frobnicator cheap background jobs?|mopslot|background jobs|path: frob/models/tiers.md | symbol: mopslot | commit: 18293a4b | plan: p08 | Cheap background jobs use the mopslot model slot; flagship is reserved.
9|What does Frobnicator burn on merge?|merge chit|merge burns|path: frob/merge/burn.rs | symbol: merge-chit | commit: 293a4b5c | plan: p09 | Every merge burns a merge chit with the candidate hash and ack.
10|How long are Frobnicator capability tokens valid?|11 minutes|capability tokens|path: frob/auth/tokens.rs | symbol: CAP_TTL | commit: 3a4b5c6d | plan: p10 | Capability tokens live 11 minutes; checkout files are 0600.
11|What triggers a Frobnicator lease reap?|5 missed|leases reap|path: frob/sched/lease.rs | symbol: MISS_LIMIT | commit: 4b5c6d7e | plan: p11 | Leases reap after 5 missed heartbeats with a lease_expired receipt.
12|Where do Frobnicator evidence blobs live?|deep vault|evidence blobs|path: frob/store/vault.rs | symbol: deep-vault | commit: 5c6d7e8f | plan: p12 | Evidence blobs live in the deep vault volume, content-addressed.
13|What is Frobnicator's staleness SLA for the war room?|750ms|war room polls|path: frob/ui/poll.rs | symbol: STALE_MS | commit: 6d7e8f90 | plan: p13 | The war room polls every 750ms; badges show age past that.
14|Which Frobnicator role may never write code?|mute poet|role never write|path: frob/roles/lore.md | symbol: mute-poet | commit: 7e8f90a1 | plan: p14 | The mute poet role may never write code; docs and memory only.
15|What signs a Frobnicator release?|sigforge|releases signed|path: frob/ops/release.md | symbol: sigforge | commit: 8f90a1b2 | plan: p15 | Releases are signed with sigforge; checksums ride the release notes.
16|What is the Frobnicator drill cadence for backups?|fortnightly|backup drills|path: frob/ops/backup.md | symbol: DRILL_WEEKLY | commit: 90a1b2c3 | plan: p16 | Backup drills run fortnightly; restores are tested, not assumed.
17|Which lane handles Frobnicator secrets?|hush lane|secrets enter|path: frob/auth/lanes.md | symbol: hush-lane | commit: 0a1b2c3d | plan: p17 | Secrets only ever enter the hush lane; workers get broker tokens.
18|What ends a Frobnicator incident review?|quiet retro|incident reviews|path: frob/ops/incident.md | symbol: quiet-retro | commit: 0a1b2c3e | plan: p18 | Incident reviews end with a quiet retro and dated action items.
19|How many owls form a Frobnicator merge quorum?|3 owls|merge quorum|path: frob/merge/policy.md | symbol: OWL_QUORUM | commit: 2c3d4e5f | plan: p19 | Merges need 3 owls quorum; the parliament checks the first pass.
20|How many passes before a Frobnicator merge?|three|merges pass review|path: frob/merge/policy.md | symbol: three-pass | commit: 3d4e5f60 | plan: p20 | Merges pass review in three pass rounds; the last round is binding.
EOF

ask() { # $1=prompt -> prints reply (stdin closed: must not eat our fixture loop)
  timeout -k 15 240 opencode run --model "$MODEL" "$1" < /dev/null 2>/dev/null | tail -n 30
}

echo "== seeding 20 anchored summaries =="
SEEDED=0
while IFS='|' read -r ID _Q _GOLD _QRY BODY; do
  [ -z "$ID" ] && continue
  TOK=$(( ${#BODY} / 4 ))
  if [ "$TOK" -gt 250 ]; then echo "task$ID summary OVER BUDGET ($TOK)"; exit 1; fi
  DOC="frob/task$ID"
  "$CLI" memory-put --db "$DB" --doc-id "$DOC" --body "$BODY" >/dev/null
  SEEDED=$((SEEDED + 1))
done <<< "$FIXTURES"
echo "seeded=$SEEDED over_budget=0" | tee -a "$LOG/run.log"

BASE_OK=0; ANCH_OK=0; N=0
while IFS='|' read -r ID Q GOLD QRY _BODY; do
  [ -z "$ID" ] && continue
  case " $TASKS " in *" $ID "*) ;; *) continue;; esac
  N=$((N + 1))
  BASE_PROMPT="You are answering factual questions about the Frobnicator project. Answer with exactly the requested entity and nothing else. Question: $Q"
  # Quote each FTS term: bare special chars (-, :, parens) otherwise parse as
  # FTS5 operators (task-20 lesson: `two-pass` read as NOT).
  ANCH_QRY=$(echo "$QRY" | tr ' ' '\n' | sed 's/.*/"&"/' | tr '\n' ' ')
  ANCH_HIT=$("$CLI" memory-search --db "$DB" --query "$ANCH_QRY" --limit 1 | cut -f2-)
  if [ -z "$ANCH_HIT" ]; then echo "task$ID RETRIEVAL MISS" | tee -a "$LOG/run.log"; continue; fi
  ANCH_PROMPT="You are answering factual questions about the Frobnicator project. Use ONLY the memory below. Answer with exactly the requested entity and nothing else. Memory: $ANCH_HIT Question: $Q"
  B=$(ask "$BASE_PROMPT" || echo "CALL-FAILED")
  A=$(ask "$ANCH_PROMPT" || echo "CALL-FAILED")
  echo "$B" > "$LOG/t${ID}_base.txt"; echo "$A" > "$LOG/t${ID}_anch.txt"
  R_BASE=1; R_ANCH=1
  OLDIFS="$IFS"; IFS=','
  norm() { tr 'A-Z' 'a-z' < /dev/stdin | tr '-' ' ' | tr -s ' '; }
  NB=$(echo "$B" | norm); NA=$(echo "$A" | norm)
  # shellcheck disable=SC2162
  for g in $GOLD; do
    g=$(echo "$g" | xargs | norm)
    echo "$NB" | grep -qiF "$g" || R_BASE=0
    echo "$NA" | grep -qiF "$g" || R_ANCH=0
  done
  IFS="$OLDIFS"
  BASE_OK=$((BASE_OK + R_BASE)); ANCH_OK=$((ANCH_OK + R_ANCH))
  echo "task$ID base=$R_BASE anch=$R_ANCH gold='$GOLD'" | tee -a "$LOG/run.log"
done <<< "$FIXTURES"

echo "=== summary (n=$N) ===" | tee -a "$LOG/run.log"
echo "baseline: $BASE_OK/$N | anchored: $ANCH_OK/$N | delta: $((ANCH_OK - BASE_OK))pts" | tee -a "$LOG/run.log"
rm -rf "$D"
echo "H-A LIVE-FIRE DONE log=$LOG"
