#!/usr/bin/env bash
# Crash storm for STC v2 phase 01.
#
# Repeatedly `kill -9` the single-writer daemon while it is writing rows and
# creating worktrees, with a concurrent read-only `studio status` loop
# overlapping the writer. After every kill a boot-time GC pass runs, followed by
# a read-only integrity projection. The storm passes only if, across all rounds,
# there are zero corrupt rows and zero orphaned worktrees.
#
# Two configurations are run so the metric actually covers the real CLI paths:
#   1. `abs`      — explicit absolute --db/--repo/--worktree-root paths.
#   2. `defaults` — the CLI's OWN DEFAULTS with relative paths (cwd == repo,
#                   `--db studio.db`, default repo `.`, default worktree root
#                   `.studio/worktrees`). This is what exposes path-
#                   canonicalization / one-pass-convergence regressions.
#
# Corruption metric includes invalid worktree states (`state NOT IN
# ('creating','ready')`) in addition to integrity + creating/missing rows.
# A metric self-test proves the predicate CAN fail: it plants a bogus state and
# requires `studio verify` to exit 1; if not, the storm fails.
#
# Usage: scripts/crash_storm.sh [ROUNDS_PER_CONFIG]   (default 50)
# Exit:  0 = PASS, 1 = FAIL. Prints a final machine-checkable summary line.
set -uo pipefail

ROUNDS="${1:-${ROUNDS:-50}}"
ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
BASE="$(mktemp -d /tmp/opencode/stc-crash.XXXXXX)"
trap 'rm -rf "$BASE" 2>/dev/null || true' EXIT

echo "[crash-storm] building studio..."
if ! cargo build --quiet --manifest-path "$ROOT/Cargo.toml" --bin studio; then
    echo "CRASH-STORM: FAIL (build)"
    exit 1
fi
BIN="$ROOT/target/debug/studio"

CORRUPT_TOTAL=0
ORPHAN_TOTAL=0
FAILED=0

make_repo() {
    local repo="$1"
    mkdir -p "$repo"
    git -C "$repo" init -q
    git -C "$repo" config user.email storm@example.com
    git -C "$repo" config user.name storm
    echo "seed" >"$repo/README.md"
    git -C "$repo" add -A
    git -C "$repo" commit -q -m init
}

# Run one round in `abs` mode (explicit absolute flags, cwd = $ROOT).
round_abs() {
    local i="$1" db="$2" repo="$3" wt="$4"
    ("$BIN" status --db "$db" >/dev/null 2>&1 || true) &
    "$BIN" daemon --db "$db" --repo "$repo" --worktree-root "$wt" >/dev/null 2>&1 &
    local pid=$!
    sleep "0.$(printf '%02d' $((RANDOM % 20 + 5)))"
    kill -9 "$pid" 2>/dev/null
    wait "$pid" 2>/dev/null
    wait 2>/dev/null
}

# Run one round in `defaults` mode (cwd = repo, relative db, no repo/root flags).
round_defaults() {
    local i="$1" repo="$2"
    (cd "$repo" && for _ in $(seq 1 10); do "$BIN" status >/dev/null 2>&1 || true; done) &
    # `exec` so $! is the daemon itself, not a subshell (otherwise kill -9
    # would orphan the daemon and it would keep holding the engine lock).
    (cd "$repo" && exec "$BIN" daemon) >/dev/null 2>&1 &
    local pid=$!
    sleep "0.$(printf '%02d' $((RANDOM % 20 + 5)))"
    kill -9 "$pid" 2>/dev/null
    wait "$pid" 2>/dev/null
    wait 2>/dev/null
}

# Count corruption and orphans from one `studio verify` run; accumulate.
account_round() {
    local label="$1" vjson="$2" rc="$3"
    if [ "$rc" -ne 0 ]; then
        echo "  $label: verify UNCLEAN -> $vjson"
        FAILED=$((FAILED + 1))
    fi
    local orphans corrupt
    orphans="$(printf '%s' "$vjson" | jq '((.git_orphans | length) + (.leftover_dirs | length))' 2>/dev/null || echo 0)"
    corrupt="$(printf '%s' "$vjson" | jq 'if .integrity_ok then (.creating_rows + .missing_rows + .invalid_state_rows) else 999 end' 2>/dev/null || echo 999)"
    ORPHAN_TOTAL=$((ORPHAN_TOTAL + orphans))
    CORRUPT_TOTAL=$((CORRUPT_TOTAL + corrupt))
}

storm_abs() {
    local work="$1" rounds="$2"
    local db="$work/studio.db" repo="$work/repo" wt="$work/worktrees"
    make_repo "$repo"
    "$BIN" init --db "$db" >/dev/null || { echo "CRASH-STORM: FAIL (init abs)"; exit 1; }
    for i in $(seq 1 "$rounds"); do
        round_abs "$i" "$db" "$repo" "$wt"
        sleep 0.15
        "$BIN" gc --db "$db" --repo "$repo" --worktree-root "$wt" >/dev/null 2>&1 || {
            echo "  abs round $i: gc FAILED"; FAILED=$((FAILED + 1)); }
        local v rc
        v="$("$BIN" verify --db "$db" --repo "$repo" --worktree-root "$wt" 2>/dev/null)"; rc=$?
        account_round "abs round $i" "$v" "$rc"
    done
    # Final DB-level corruption predicate (belt and braces).
    local integ bad
    integ="$(sqlite3 "$db" 'PRAGMA integrity_check;' 2>/dev/null || echo unreadable)"
    bad="$(sqlite3 "$db" "SELECT COUNT(*) FROM worktrees WHERE state NOT IN ('creating','ready');" 2>/dev/null || echo 999)"
    [ "$integ" = "ok" ] || CORRUPT_TOTAL=$((CORRUPT_TOTAL + 1))
    CORRUPT_TOTAL=$((CORRUPT_TOTAL + bad))
    "$BIN" status --db "$db" >/dev/null 2>&1 || { echo "  final status FAILED"; FAILED=$((FAILED + 1)); }
}

storm_defaults() {
    local work="$1" rounds="$2"
    local repo="$work/repo"
    make_repo "$repo"
    (cd "$repo" && "$BIN" init) >/dev/null || { echo "CRASH-STORM: FAIL (init defaults)"; exit 1; }
    for i in $(seq 1 "$rounds"); do
        round_defaults "$i" "$repo"
        sleep 0.15
        (cd "$repo" && "$BIN" gc) >/dev/null 2>&1 || {
            echo "  defaults round $i: gc FAILED"; FAILED=$((FAILED + 1)); }
        local v rc
        v="$(cd "$repo" && "$BIN" verify 2>/dev/null)"; rc=$?
        account_round "defaults round $i" "$v" "$rc"
    done
    (cd "$repo" && "$BIN" gc) >/dev/null 2>&1
    local v rc
    v="$(cd "$repo" && "$BIN" verify 2>/dev/null)"; rc=$?
    account_round "defaults final" "$v" "$rc"
    (cd "$repo" && "$BIN" status) >/dev/null 2>&1 || { echo "  defaults final status FAILED"; FAILED=$((FAILED + 1)); }
}

# Prove the corruption metric can fail: plant an invalid state and require verify
# to report it unclean (exit 1, invalid_state_rows >= 1).
metric_self_test() {
    local work="$BASE/metric"
    local db="$work/studio.db" repo="$work/repo" wt="$work/wt"
    make_repo "$repo"
    "$BIN" init --db "$db" >/dev/null 2>&1
    "$BIN" daemon --db "$db" --repo "$repo" --worktree-root "$wt" --ticks 1 >/dev/null 2>&1
    sqlite3 "$db" "UPDATE worktrees SET state='bogus_state';" 2>/dev/null
    local v rc inv
    v="$("$BIN" verify --db "$db" --repo "$repo" --worktree-root "$wt" 2>/dev/null)"; rc=$?
    inv="$(printf '%s' "$v" | jq '.invalid_state_rows' 2>/dev/null || echo 0)"
    if [ "$rc" -eq 1 ] && [ "$inv" -ge 1 ]; then
        echo "[crash-storm] metric self-test: PASS (invalid state -> verify exit 1, invalid_state_rows=$inv)"
        return 0
    fi
    echo "[crash-storm] metric self-test: FAIL (verify rc=$rc invalid_state_rows=$inv) — metric cannot fail"
    return 1
}

# Prove recovery refuses symlinks (A3/A4): a symlink bucket under the worktree
# root must never be traversed or deleted through, and `verify` must report it.
symlink_self_test() {
    local work="$BASE/symlink"
    local db="$work/studio.db" repo="$work/repo" wt="$work/wt" victim="$work/victim"
    make_repo "$repo"
    mkdir -p "$wt" "$victim/keep"
    echo VICTIM >"$victim/marker.txt"
    echo KEEP >"$victim/keep/k.txt"
    ln -s "$victim" "$wt/evilbucket"
    "$BIN" init --db "$db" >/dev/null 2>&1
    local gjson rc refused intact vrc
    gjson="$("$BIN" gc --db "$db" --repo "$repo" --worktree-root "$wt" 2>/dev/null)"; rc=$?
    refused="$(printf '%s' "$gjson" | jq '.refused_symlinks | length' 2>/dev/null || echo 0)"
    [ -f "$victim/marker.txt" ] && [ -f "$victim/keep/k.txt" ] && intact=1 || intact=0
    "$BIN" verify --db "$db" --repo "$repo" --worktree-root "$wt" >/dev/null 2>&1; vrc=$?
    if [ "$rc" -eq 0 ] && [ "$refused" -ge 1 ] && [ "$intact" -eq 1 ] && [ "$vrc" -eq 1 ]; then
        echo "[crash-storm] symlink self-test: PASS (refused=$refused victim_intact=1 verify_unclean=1)"
        return 0
    fi
    echo "[crash-storm] symlink self-test: FAIL (gc rc=$rc refused=$refused victim_intact=$intact verify_rc=$vrc) — symlink escape"
    return 1
}

if ! metric_self_test; then
    FAILED=$((FAILED + 1))
fi
if ! symlink_self_test; then
    FAILED=$((FAILED + 1))
fi

# Prove the engine lock is keyed to the shared repo/worktree root (B): a second
# engine on a DIFFERENT db but the SAME repo/root must be refused.
lock_self_test() {
    local work="$BASE/lock"
    local dbA="$work/dbA" dbB="$work/dbB" repo="$work/repo" wt="$work/wt"
    make_repo "$repo"
    "$BIN" init --db "$dbA" >/dev/null 2>&1
    "$BIN" init --db "$dbB" >/dev/null 2>&1
    "$BIN" daemon --db "$dbA" --repo "$repo" --worktree-root "$wt" --ticks 100000 \
        >/dev/null 2>"$work/a.err" &
    local pid=$!
    for _ in $(seq 1 200); do
        grep -q "wal soft bound" "$work/a.err" 2>/dev/null && break
        sleep 0.05
    done
    local gcrc=0
    "$BIN" gc --db "$dbB" --repo "$repo" --worktree-root "$wt" >/dev/null 2>&1 || gcrc=$?
    kill -9 "$pid" 2>/dev/null
    wait "$pid" 2>/dev/null
    if [ "$gcrc" -ne 0 ]; then
        echo "[crash-storm] lock self-test: PASS (cross-db gc on shared repo/root refused, exit $gcrc)"
        return 0
    fi
    echo "[crash-storm] lock self-test: FAIL (cross-db gc acquired the shared repo/root)"
    return 1
}

if ! lock_self_test; then
    FAILED=$((FAILED + 1))
fi

echo "[crash-storm] config=abs rounds=$ROUNDS"
storm_abs "$BASE/abs" "$ROUNDS"
echo "[crash-storm] config=defaults rounds=$ROUNDS (CLI defaults, relative paths)"
storm_defaults "$BASE/defaults" "$ROUNDS"

echo "[crash-storm] configs=abs,defaults rounds_per_config=$ROUNDS corrupt_rows=$CORRUPT_TOTAL orphaned_worktrees=$ORPHAN_TOTAL failures=$FAILED"
if [ "$FAILED" -eq 0 ] && [ "$CORRUPT_TOTAL" -eq 0 ] && [ "$ORPHAN_TOTAL" -eq 0 ]; then
    echo "CRASH-STORM: PASS configs=abs,defaults rounds_per_config=$ROUNDS corrupt_rows=0 orphaned_worktrees=0"
    exit 0
fi
echo "CRASH-STORM: FAIL configs=abs,defaults rounds_per_config=$ROUNDS corrupt_rows=$CORRUPT_TOTAL orphaned_worktrees=$ORPHAN_TOTAL failures=$FAILED"
exit 1
