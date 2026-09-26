#!/usr/bin/env python3
"""
H1 — Write-Intent Fabric spike (throwaway).
Question: Can 5 parallel edits against ONE shared tree coordinate via
scope claims (file|fn|symbol) + SQLite claim broker without collisions
on non-overlapping claims, and within 5% task-success of worktree baseline?

Method:
- scope_hash(file|fn|symbol): sha256 of normalized scope key (simulates
  tree-sitter extractor; fallback file-granularity for unknown grammars).
- Claim broker: SQLite (WAL, BEGIN IMMEDIATE) table claims(scope_hash, owner).
  Overlapping claims rejected deterministically; non-overlapping must never collide.
- Work: 5 threads, each owns 2 disjoint functions in shared tree (10 fns total,
  non-overlapping by construction). Each thread: acquire claims -> edit files ->
  release. Overlap detector records any collision on non-overlapping claims.
- Baseline: same 5 edits each in isolated temp copy (worktree analogue) -> merge.
- Kill criterion: any collision on non-overlapping claims OR success drop >5% vs baseline.
"""

import hashlib, os, sqlite3, shutil, tempfile, threading, time

ROOT = tempfile.mkdtemp(prefix="h1-shared-")
DB = os.path.join(ROOT, "claims.db")


def scope_hash(file, fn=None, symbol=None):
    key = file + "|" + (fn or "") + "|" + (symbol or "")
    return hashlib.sha256(key.encode()).hexdigest()[:16]


def init_db():
    con = sqlite3.connect(DB, isolation_level=None, check_same_thread=False)
    con.execute("PRAGMA journal_mode=WAL;")
    con.execute(
        "CREATE TABLE IF NOT EXISTS claims(scope_hash TEXT PRIMARY KEY, owner TEXT)"
    )
    return con


# Build shared tree: 2 files x 5 functions each
FILES = ["src/a.py", "src/b.py"]
for f in FILES:
    fp = os.path.join(ROOT, f)
    os.makedirs(os.path.dirname(fp), exist_ok=True)
    with open(fp, "w") as fh:
        for i in range(5):
            fh.write(f"def fn{i}():\n    return {i}\n\n")

# Assign non-overlapping claims: thread t owns fn[t] in a.py and fn[t] in b.py? That overlaps across threads? No.
# Better: 10 distinct (file,fn) pairs, 2 per thread, disjoint across threads.
TASKS = []
for t in range(5):
    TASKS.append([(FILES[0], f"fn{t}"), (FILES[1], f"fn{(t + 2) % 5}")])
# Verify disjoint
all_claims = [c for task in TASKS for c in task]
assert len(all_claims) == len(set(all_claims)), (
    "tasks must be non-overlapping by construction"
)

con = init_db()
con_lock = threading.Lock()
collisions = []
results = {}


def try_claim(owner, scopes):
    with con_lock:
        con.execute("BEGIN IMMEDIATE;")
        try:
            for s in scopes:
                h = scope_hash(*s)
                cur = con.execute(
                    "SELECT owner FROM claims WHERE scope_hash=?", (h,)
                ).fetchone()
                if cur and cur[0] != owner:
                    con.execute("ROLLBACK;")
                    return False, h
            for s in scopes:
                h = scope_hash(*s)
                con.execute(
                    "INSERT OR REPLACE INTO claims(scope_hash, owner) VALUES(?,?)",
                    (h, owner),
                )
            con.execute("COMMIT;")
            return True, None
        except Exception as e:
            con.execute("ROLLBACK;")
            return False, str(e)


def release(owner):
    with con_lock:
        con.execute("BEGIN IMMEDIATE;")
        con.execute("DELETE FROM claims WHERE owner=?", (owner,))
        con.execute("COMMIT;")


def worker(tid):
    owner = f"agent-{tid}"
    scopes = TASKS[tid]
    ok, conflict = try_claim(owner, scopes)
    if not ok:
        collisions.append((owner, conflict))
        results[owner] = False
        return
    try:
        time.sleep(0.05)  # simulate edit work
        for f, fn in scopes:
            fp = os.path.join(ROOT, f)
            with open(fp, "a") as fh:
                fh.write(f"# edit by {owner} in {fn}\n")
        results[owner] = True
    except Exception:
        results[owner] = False
    finally:
        release(owner)


threads = [threading.Thread(target=worker, args=(i,)) for i in range(5)]
t0 = time.time()
for t in threads:
    t.start()
for t in threads:
    t.join()
shared_time = time.time() - t0
shared_success = sum(1 for v in results.values() if v) / 5.0

# Worktree baseline: 5 isolated copies, same edits, always succeed (no sharing)
t0 = time.time()
baseline_ok = 0
for tid in range(5):
    tmp = tempfile.mkdtemp(prefix=f"h1-wt-{tid}-")
    try:
        for f, fn in TASKS[tid]:
            fp = os.path.join(tmp, f)
            os.makedirs(os.path.dirname(fp), exist_ok=True)
            with open(fp, "w") as fh:
                fh.write(f"# baseline edit {tid} {fn}\n")
        baseline_ok += 1
    finally:
        shutil.rmtree(tmp, ignore_errors=True)
baseline_time = time.time() - t0
baseline_success = baseline_ok / 5.0

drop = baseline_success - shared_success
print(
    f"H1 shared_success={shared_success:.2f} baseline_success={baseline_success:.2f} drop={drop:.3f}"
)
print(f"H1 collisions_on_non_overlapping={len(collisions)} {collisions}")
print(f"H1 times shared={shared_time:.3f}s baseline={baseline_time:.3f}s")
# Overlap stress: two agents claiming SAME scope must collide (deterministic reject)
ok1, _ = try_claim("x", [(FILES[0], "fn0")])
ok2, _ = try_claim("y", [(FILES[0], "fn0")])
print(f"H1 overlap_reject_check first_claim={ok1} second_claim_rejected={not ok2}")
release("x")
release("y")

killed = len(collisions) > 0 or drop > 0.05 or not (not ok2)
print("H1_RESULT:", "KILLED" if killed else "PASS")
shutil.rmtree(ROOT, ignore_errors=True)
exit(0 if not killed else 1)
