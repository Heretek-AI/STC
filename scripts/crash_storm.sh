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
# TEST-F-01: resolve the *actual* cargo output path. `cargo build` respects
# CARGO_TARGET_DIR, so a hardcoded "$ROOT/target/debug/studio" can silently test
# a stale binary. Ask cargo for the artifact path via --message-format=json.
BUILD_JSON="$(cargo build --message-format=json --manifest-path "$ROOT/Cargo.toml" --bin studio 2>"$BASE/build.err")"
BIN="$(printf '%s\n' "$BUILD_JSON" | jq -r 'select(.reason=="compiler-artifact" and .target.name=="studio") | .executable // empty' 2>/dev/null | tail -n1)"
if [ -z "$BIN" ] || [ ! -x "$BIN" ]; then
    echo "CRASH-STORM: FAIL (could not resolve the studio binary path)"
    tail -n 20 "$BASE/build.err" 2>/dev/null
    exit 1
fi
echo "[crash-storm] studio binary: $BIN"

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
        # gc exits 0 when clean, 1 when it reaped/refused (report not clean) —
        # both converge; only exit >= 2 is a hard failure.
        "$BIN" gc --db "$db" --repo "$repo" --worktree-root "$wt" >/dev/null 2>&1; gcrc=$?
        if [ "$gcrc" -gt 1 ]; then
            echo "  abs round $i: gc FAILED (exit $gcrc)"; FAILED=$((FAILED + 1)); fi
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
        (cd "$repo" && "$BIN" gc) >/dev/null 2>&1; gcrc=$?
        if [ "$gcrc" -gt 1 ]; then
            echo "  defaults round $i: gc FAILED (exit $gcrc)"; FAILED=$((FAILED + 1)); fi
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
    # A refused symlink makes gc's report unclean, so gc must exit non-zero
    # (verify parity) while the victim stays intact and verify stays unclean.
    if [ "$rc" -ne 0 ] && [ "$refused" -ge 1 ] && [ "$intact" -eq 1 ] && [ "$vrc" -eq 1 ]; then
        echo "[crash-storm] symlink self-test: PASS (gc exit $rc refused=$refused victim_intact=1 verify_unclean=1)"
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

# LOCK-B-iii: QA-B's cross-engine scenario. A *second* engine (fresh repo, empty
# ledger, same worktree root) must never reap worktrees that belong to another
# engine and are registered with git — neither a git-`locked` live worktree nor
# one caught mid-create (before `git worktree lock` ran). Direction 2: the second
# engine's own *unlocked* orphan is still reaped, so recovery is not a no-op.
locked_self_test() {
    local work="$BASE/locked"
    local repoA="$work/repoA" repoB="$work/repoB" wt="$work/shared"
    local dbB="$work/dbB"
    rm -rf "$work"; mkdir -p "$repoA" "$repoB" "$wt"
    make_repo "$repoA"; make_repo "$repoB"
    # A's live, git-locked worktree under the shared root.
    local p="$wt/5a9cb6b5/wt-live"
    mkdir -p "$(dirname "$p")"
    git -C "$repoA" worktree add --detach --no-checkout "$p" HEAD >/dev/null 2>&1
    git -C "$repoA" worktree lock --reason daemon "$p"
    echo PRECIOUS >"$p/precious.txt"
    # A registered but still-mid-create worktree (never locked).
    local q="$wt/5a9cb6b5/wt-mid"
    git -C "$repoA" worktree add --detach --no-checkout "$q" HEAD >/dev/null 2>&1
    echo PRECIOUS >"$q/precious.txt"
    "$BIN" init --db "$dbB" >/dev/null 2>&1

    # (1) second engine gc: fresh repo, empty ledger, same root. The refusal
    # makes gc's report unclean, so gc must exit non-zero (verify parity).
    local gjson rc refused swept intact listed
    gjson="$("$BIN" gc --db "$dbB" --repo "$repoB" --worktree-root "$wt" 2>/dev/null)"; rc=$?
    refused="$(printf '%s' "$gjson" | jq '.refused_locked | length' 2>/dev/null || echo 0)"
    swept="$(printf '%s' "$gjson" | jq --arg p "$p" '.swept_dirs | map(select(.==$p)) | length' 2>/dev/null || echo 0)"
    [ -f "$p/precious.txt" ] && [ -f "$q/precious.txt" ] && intact=1 || intact=0
    listed="$(git -C "$repoA" worktree list --porcelain 2>/dev/null | grep -c "$p" || true)"
    if [ "$rc" -eq 0 ] || [ "$refused" -lt 1 ] || [ "$swept" -ne 0 ] || [ "$intact" -ne 1 ] || [ "$listed" -lt 1 ]; then
        echo "[crash-storm] locked-worktree self-test: FAIL gc (rc=$rc refused=$refused swept=$swept intact=$intact still_listed=$listed) — cross-engine destruction"
        return 1
    fi
    # (2) second engine *daemon boot* recovery: same invariant.
    local dbC="$work/dbC" repoC="$work/repoC"
    mkdir -p "$repoC"; make_repo "$repoC"
    "$BIN" init --db "$dbC" >/dev/null 2>&1
    local drc=0
    "$BIN" daemon --db "$dbC" --repo "$repoC" --worktree-root "$wt" --ticks 1 >/dev/null 2>"$work/c.err" || drc=$?
    [ -f "$p/precious.txt" ] && [ -f "$q/precious.txt" ] && intact=1 || intact=0
    if [ "$drc" -ne 0 ] || [ "$intact" -ne 1 ]; then
        echo "[crash-storm] locked-worktree self-test: FAIL daemon-boot (rc=$drc intact=$intact) — cross-engine destruction at boot"
        return 1
    fi
    # (3) direction 2: our own *unlocked* orphan of the second engine's repo is
    # still reaped (LOCK-B-iii did not turn recovery into a no-op).
    local r="$wt/orphanbucket/wt-free"
    mkdir -p "$(dirname "$r")"
    git -C "$repoB" worktree add --detach --no-checkout "$r" HEAD >/dev/null 2>&1
    echo junk >"$r/junk.txt"
    "$BIN" gc --db "$dbB" --repo "$repoB" --worktree-root "$wt" >/dev/null 2>&1
    if [ -e "$r" ]; then
        echo "[crash-storm] locked-worktree self-test: FAIL (unlocked orphan not reaped — recovery is a no-op)"
        return 1
    fi
    [ -f "$p/precious.txt" ] && [ -f "$q/precious.txt" ] && intact=1 || intact=0
    if [ "$intact" -ne 1 ]; then
        echo "[crash-storm] locked-worktree self-test: FAIL (A's worktrees harmed by the second gc)"
        return 1
    fi
    echo "[crash-storm] locked-worktree self-test: PASS (locked + mid-create refused by gc AND daemon boot; own unlocked orphan still reaped)"
    return 0
}

if ! locked_self_test; then
    FAILED=$((FAILED + 1))
fi

# B-EXEMPT-FORGE-01: a second engine forges a `creating` row for the first
# engine's live locked worktree into its OWN db (same repo). gc must refuse
# (exit non-zero, no unlock, no reap, victim + lock intact, forged row kept).
forge_self_test() {
    local work="$BASE/forge"
    local repo="$work/repo" wt="$work/shared" dbB="$work/dbB"
    rm -rf "$work"; mkdir -p "$repo" "$wt"
    make_repo "$repo"
    local p="$wt/aaaaaaa1/victim"
    mkdir -p "$(dirname "$p")"
    git -C "$repo" worktree add --detach --no-checkout "$p" HEAD >/dev/null 2>&1
    git -C "$repo" worktree lock --reason "studio:first-engine-0000000000000001" "$p"
    echo PRECIOUS >"$p/precious.txt"
    "$BIN" init --db "$dbB" >/dev/null 2>&1
    sqlite3 "$dbB" "INSERT INTO worktrees(path,repo,slug,state,created_ms) VALUES('$p','$repo','forged','creating',1);"
    local gjson rc refused reaped intact lockedrows kept
    gjson="$("$BIN" gc --db "$dbB" --repo "$repo" --worktree-root "$wt" 2>/dev/null)"; rc=$?
    refused="$(printf '%s' "$gjson" | jq '.refused_locked | length' 2>/dev/null || echo 0)"
    reaped="$(printf '%s' "$gjson" | jq '(.reaped_creating | length) + (.reaped_git_orphans | length) + (.swept_dirs | length)' 2>/dev/null || echo 99)"
    [ -f "$p/precious.txt" ] && intact=1 || intact=0
    lockedrows="$(git -C "$repo" worktree list --porcelain 2>/dev/null | grep -c "$p" || true)"
    kept="$(sqlite3 "$dbB" "SELECT COUNT(*) FROM worktrees WHERE state='creating';" 2>/dev/null || echo 0)"
    if [ "$rc" -ne 0 ] && [ "$refused" -ge 1 ] && [ "$reaped" -eq 0 ] && [ "$intact" -eq 1 ] && [ "$lockedrows" -ge 1 ] && [ "$kept" -eq 1 ]; then
        echo "[crash-storm] forge self-test: PASS (gc exit $rc refused=$refused reaped=0 victim_intact=1 lock_intact=1 forged_row_kept=1)"
        return 0
    fi
    echo "[crash-storm] forge self-test: FAIL (rc=$rc refused=$refused reaped=$reaped intact=$intact still_listed=$lockedrows forged_kept=$kept) — forged row honoured"
    return 1
}

if ! forge_self_test; then
    FAILED=$((FAILED + 1))
fi

# C-NEWLINE-01: a registered worktree whose path contains a newline. gc must
# refuse the REAL path (dedicated class, exit non-zero), never sweep or
# unlock it, and never report a truncated phantom.
newline_self_test() {
    local work="$BASE/newline"
    local repo="$work/repo" wt="$work/wt" db="$work/db"
    rm -rf "$work"; mkdir -p "$repo" "$wt"
    make_repo "$repo"
    local evil
    evil="$(printf '%s/evil\nlocked' "$wt")"
    git -C "$repo" worktree add --detach "$evil" HEAD >/dev/null 2>&1
    git -C "$repo" worktree lock --reason daemon "$evil"
    echo PRECIOUS >"$evil/precious.txt"
    "$BIN" init --db "$db" >/dev/null 2>&1
    local gjson rc refused_ctrl refused_phantom swept intact
    gjson="$("$BIN" gc --db "$db" --repo "$repo" --worktree-root "$wt" 2>/dev/null)"; rc=$?
    refused_ctrl="$(printf '%s' "$gjson" | jq '.refused_control_paths | length' 2>/dev/null || echo 0)"
    refused_phantom="$(printf '%s' "$gjson" | jq --arg ph "$wt/evil" '.refused_locked | map(select(.==$ph)) | length' 2>/dev/null || echo 99)"
    swept="$(printf '%s' "$gjson" | jq '.swept_dirs | length' 2>/dev/null || echo 99)"
    [ -f "$evil/precious.txt" ] && intact=1 || intact=0
    if [ "$rc" -ne 0 ] && [ "$refused_ctrl" -ge 1 ] && [ "$refused_phantom" -eq 0 ] && [ "$swept" -eq 0 ] && [ "$intact" -eq 1 ]; then
        echo "[crash-storm] newline self-test: PASS (gc exit $rc refused_control=$refused_ctrl phantom=0 swept=0 victim_intact=1)"
        return 0
    fi
    echo "[crash-storm] newline self-test: FAIL (rc=$rc refused_ctrl=$refused_ctrl phantom=$refused_phantom swept=$swept intact=$intact) — newline worktree swept or phantom"
    return 1
}

if ! newline_self_test; then
    FAILED=$((FAILED + 1))
fi

# A-DB-COPY-01: `cp` clones engine_meta but not the inode. The copy must
# diverge to its own engine id on first open, so a forged `creating` row in
# the copy (same repo/root) is refused — never reaps the victim — while the
# original engine stays clean.
dbcopy_self_test() {
    local work="$BASE/dbcopy"
    local repo="$work/repo" wt="$work/wt" dbV="$work/victim.db" dbC="$work/copy.db"
    rm -rf "$work"; mkdir -p "$repo" "$wt"
    make_repo "$repo"
    "$BIN" init --db "$dbV" >/dev/null 2>&1
    "$BIN" daemon --db "$dbV" --repo "$repo" --worktree-root "$wt" --ticks 8 >/dev/null 2>&1
    local victim
    victim="$(sqlite3 "$dbV" "SELECT path FROM worktrees WHERE state='ready' LIMIT 1;" 2>/dev/null)"
    [ -n "$victim" ] || { echo "[crash-storm] dbcopy self-test: FAIL (no victim worktree created)"; return 1; }
    echo PRECIOUS >"$victim/precious.txt"
    "$BIN" gc --db "$dbV" --repo "$repo" --worktree-root "$wt" >/dev/null 2>&1
    cp "$dbV" "$dbC"
    # Attacker surgery on the COPY only: flip the victim row to `creating`.
    sqlite3 "$dbC" "UPDATE worktrees SET state='creating' WHERE path='$victim';"
    local gjson rc refused reaped intact
    gjson="$("$BIN" gc --db "$dbC" --repo "$repo" --worktree-root "$wt" 2>/dev/null)"; rc=$?
    refused="$(printf '%s' "$gjson" | jq '.refused_locked | length' 2>/dev/null || echo 0)"
    reaped="$(printf '%s' "$gjson" | jq '(.reaped_creating | length) + (.reaped_git_orphans | length)' 2>/dev/null || echo 99)"
    [ -f "$victim/precious.txt" ] && intact=1 || intact=0
    # The original engine is unaffected: same id, victim still ready, own gc clean.
    local idV idV2 ownrc
    idV="$(sqlite3 "$dbV" "SELECT value FROM engine_meta WHERE key='engine_id';")"
    "$BIN" gc --db "$dbV" --repo "$repo" --worktree-root "$wt" >/dev/null 2>&1; ownrc=$?
    idV2="$(sqlite3 "$dbV" "SELECT value FROM engine_meta WHERE key='engine_id';")"
    if [ "$rc" -ne 0 ] && [ "$refused" -ge 1 ] && [ "$reaped" -eq 0 ] && [ "$intact" -eq 1 ] && [ "$idV" = "$idV2" ] && [ "$ownrc" -eq 0 ]; then
        echo "[crash-storm] dbcopy self-test: PASS (copy gc exit $rc refused=$refused reaped=0 victim_intact=1 orig_id_stable=1 orig_gc_clean=1)"
        return 0
    fi
    echo "[crash-storm] dbcopy self-test: FAIL (rc=$rc refused=$refused reaped=$reaped intact=$intact orig_stable=$([ "$idV" = "$idV2" ] && echo 1 || echo 0) orig_gc=$ownrc) — copy honoured the forged row"
    return 1
}

if ! dbcopy_self_test; then
    FAILED=$((FAILED + 1))
fi

# C-UNLOCK-TRAP-01: forge → gc refuses → `git worktree unlock` (the old remedy)
# → gc MUST still refuse (unlocking never re-arms auto-reap), victim intact
# across runs; then `studio release` clears the refusal AND the lingering
# `creating` row, and the next gc reaps the now-ordinary orphan (recourse).
unlock_release_self_test() {
    local work="$BASE/unlockrelease"
    local repo="$work/repo" wt="$work/shared" dbB="$work/dbB"
    rm -rf "$work"; mkdir -p "$repo" "$wt"
    make_repo "$repo"
    local p="$wt/aaaaaaa1/victim"
    mkdir -p "$(dirname "$p")"
    git -C "$repo" worktree add --detach --no-checkout "$p" HEAD >/dev/null 2>&1
    git -C "$repo" worktree lock --reason "studio:first-engine-0000000000000001" "$p"
    echo PRECIOUS >"$p/precious.txt"
    "$BIN" init --db "$dbB" >/dev/null 2>&1
    sqlite3 "$dbB" "INSERT INTO worktrees(path,repo,slug,state,created_ms) VALUES('$p','$repo','forged','creating',1);"
    # (1) first gc refuses.
    local g1 rc1 refused1
    g1="$("$BIN" gc --db "$dbB" --repo "$repo" --worktree-root "$wt" 2>/dev/null)"; rc1=$?
    refused1="$(printf '%s' "$g1" | jq '(.refused_locked | length) + (.refused_worktrees | length)' 2>/dev/null || echo 0)"
    if [ "$rc1" -eq 0 ] || [ "$refused1" -lt 1 ]; then
        echo "[crash-storm] unlock-release self-test: FAIL gc1 (rc=$rc1 refused=$refused1) — forged row not refused"
        return 1
    fi
    # (2) unlock, then gc twice: still refused, victim intact, nothing reaped.
    git -C "$repo" worktree unlock "$p" >/dev/null 2>&1
    local i g rc refused reaped intact
    for i in 1 2; do
        g="$("$BIN" gc --db "$dbB" --repo "$repo" --worktree-root "$wt" 2>/dev/null)"; rc=$?
        refused="$(printf '%s' "$g" | jq '(.refused_locked | length) + (.refused_worktrees | length) + ((.refused_persisted // []) | length)' 2>/dev/null || echo 0)"
        reaped="$(printf '%s' "$g" | jq '(.reaped_creating | length) + (.reaped_git_orphans | length) + (.swept_dirs | length)' 2>/dev/null || echo 99)"
        [ -f "$p/precious.txt" ] && intact=1 || intact=0
        if [ "$rc" -eq 0 ] || [ "$refused" -lt 1 ] || [ "$reaped" -ne 0 ] || [ "$intact" -ne 1 ]; then
            echo "[crash-storm] unlock-release self-test: FAIL post-unlock gc$i (rc=$rc refused=$refused reaped=$reaped intact=$intact) — unlock re-armed auto-reap"
            return 1
        fi
    done
    # (3) explicit recourse: release clears refusal + lingering creating row.
    local rel rrc creating_left
    rel="$("$BIN" release --db "$dbB" "$p" 2>/dev/null)"; rrc=$?
    creating_left="$(sqlite3 "$dbB" "SELECT COUNT(*) FROM worktrees WHERE state='creating';" 2>/dev/null || echo 99)"
    if [ "$rrc" -ne 0 ] || [ "$(printf '%s' "$rel" | jq -r '.released' 2>/dev/null)" != "true" ] || [ "$creating_left" -ne 0 ]; then
        echo "[crash-storm] unlock-release self-test: FAIL release (rc=$rrc rel=$rel creating_left=$creating_left) — no recourse"
        return 1
    fi
    # (4) now-ordinary unlocked orphan reaps; tree converges.
    "$BIN" gc --db "$dbB" --repo "$repo" --worktree-root "$wt" >/dev/null 2>&1
    if [ -e "$p" ]; then
        echo "[crash-storm] unlock-release self-test: FAIL (released stale path not reaped)"
        return 1
    fi
    echo "[crash-storm] unlock-release self-test: PASS (refuse → unlock-still-refused x2 → release → reaped)"
    return 0
}

if ! unlock_release_self_test; then
    FAILED=$((FAILED + 1))
fi

# SEC-A-TOCTOU-01 (a): a root bucket is classified as a real directory, then
# swapped for a symlink to a live git repo outside the root *after* that
# classification. A path-based sweep follows the link and destroys the victim
# (exit 0, no refusal); an fd-anchored sweep refuses it. Staggered timings make
# the window independent of readdir order; the invariant is the victim's
# survival and exit 0 on every attempt.
toctou_self_test() {
    local fails=0 refused_seen=0
    local delays="0.03 0.07 0.11 0.15"
    for delay in $delays; do
        local work="$BASE/toctou-$delay"
        local repo="$work/repo" victim="$work/victim" wt="$work/wt" db="$work/studio.db"
        rm -rf "$work"; mkdir -p "$repo" "$victim" "$wt"
        ( cd "$repo" && git init -q && git config user.email s@e.c && git config user.name s \
          && echo seed > README.md && git add -A && git commit -q -m init )
        ( cd "$victim" && git init -q && git config user.email s@e.c && git config user.name s \
          && mkdir -p src && echo precious > precious.txt && echo s > src/s.txt \
          && git add -A && git commit -q -m init )
        mkdir -p "$wt/RACE"; : > "$wt/RACE/placeholder"
        for b in $(seq -w 1 30); do
            mkdir -p "$wt/b$b"
            for f in $(seq 1 800); do : > "$wt/b$b/f$f"; done
        done
        "$BIN" init --db "$db" >/dev/null 2>&1
        ( "$BIN" gc --db "$db" --repo "$repo" --worktree-root "$wt" \
            > "$work/gc.out" 2> "$work/gc.err"; echo $? > "$work/gc.rc" ) &
        local gcpid=$!
        ( sleep "$delay"; rm -rf "$wt/RACE"; ln -s "$victim" "$wt/RACE" )
        wait "$gcpid"
        local rc refused intact gitok
        rc="$(cat "$work/gc.rc")"
        refused="$(jq '.refused_symlinks | length' "$work/gc.out" 2>/dev/null || echo 0)"
        gitok="$(cd "$victim" && git rev-parse --is-inside-work-tree 2>/dev/null || true)"
        intact=0
        [ -f "$victim/precious.txt" ] && [ -f "$victim/src/s.txt" ] && [ "$gitok" = "true" ] && intact=1
        [ "$refused" -ge 1 ] && refused_seen=1
        # gc exits 1 when it refused (unclean report); only >= 2 is a failure.
        # The invariant is the victim's survival on every attempt.
        if [ "$rc" -gt 1 ] || [ "$intact" -ne 1 ]; then
            echo "  toctou[$delay]: rc=$rc refused=$refused victim_intact=$intact"
            fails=$((fails + 1))
        fi
    done
    if [ "$fails" -eq 0 ]; then
        echo "[crash-storm] toctou self-test: PASS (post-classification swap never followed; victim intact; refusals observed=$refused_seen)"
        return 0
    fi
    echo "[crash-storm] toctou self-test: FAIL ($fails/$delays attempts escaped or errored) — symlink escape"
    return 1
}

# SEC-A-TOCTOU-01 (b): entries enumerated inside a bucket are removed from under
# the walk (fast flip). A path-based sweep aborts the whole pass with
# `io: No such file or directory` (exit 2); an fd-anchored sweep skips them.
toctou_fastflip_self_test() {
    local work="$BASE/fastflip"
    local repo="$work/repo" wt="$work/wt" db="$work/studio.db"
    rm -rf "$work"; mkdir -p "$repo" "$wt"
    ( cd "$repo" && git init -q && git config user.email s@e.c && git config user.name s \
      && echo seed > README.md && git add -A && git commit -q -m init )
    # RACE first (oldest); many entries so its walk takes measurable time.
    mkdir -p "$wt/RACE"
    for f in $(seq 1 30000); do : > "$wt/RACE/e$f"; done
    for b in $(seq -w 1 20); do
        mkdir -p "$wt/b$b"
        for f in $(seq 1 300); do : > "$wt/b$b/f$f"; done
    done
    "$BIN" init --db "$db" >/dev/null 2>&1
    ( "$BIN" gc --db "$db" --repo "$repo" --worktree-root "$wt" \
        > "$work/gc.out" 2> "$work/gc.err"; echo $? > "$work/gc.rc" ) &
    local gcpid=$!
    # Wait for the sweep to start, then delete RACE's oldest entries (last in
    # tmpfs readdir order) while gc is walking RACE.
    for _ in $(seq 1 2000); do [ -d "$wt/b20" ] || break; sleep 0.001; done
    ( for f in $(seq 1 30000); do rm -f "$wt/RACE/e$f" 2>/dev/null || true; done ) &
    local delpid=$!
    wait "$gcpid"
    kill "$delpid" 2>/dev/null; wait "$delpid" 2>/dev/null
    local rc; rc="$(cat "$work/gc.rc")"
    # Sweeping the batch makes the report unclean (exit 1); only >= 2 or a
    # fast-flip abort ("No such file") is a failure.
    if { [ "$rc" -eq 0 ] || [ "$rc" -eq 1 ]; } && ! grep -q "No such file" "$work/gc.err"; then
        echo "[crash-storm] toctou fast-flip self-test: PASS (vanished entries skipped; gc exit $rc)"
        return 0
    fi
    echo "[crash-storm] toctou fast-flip self-test: FAIL (gc exit $rc: $(head -c 120 "$work/gc.err"))"
    return 1
}

if ! toctou_self_test; then
    FAILED=$((FAILED + 1))
fi
if ! toctou_fastflip_self_test; then
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
