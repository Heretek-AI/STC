//! Event hub: CDC poll → ring buffer → history cursor + poll-driven live SSE.
//!
//! Schema-first shape (f04-herdr-api clean-room): subscriptions are SSE
//! streams over `/api/events/stream?since=N`; the history cursor is
//! `/api/events?since=N&limit=M` with [`GapKind`] Lost/Unavailable… —
//! precisely: `Lost` when `since` predates the retained floor, `None` when
//! the page covers `(since, head]` contiguously. Overflow is never silent.
//!
//! Retention: bounded ring (`STUDIO_HUB_RING_CAP`, default 512 — the same
//! batch the projection polls at, see `projection::Snapshot`). Eviction moves
//! `floor_seq` forward; any cursor at or below the floor gets `Lost`.
//! A cursor AHEAD of `head_seq` (future cursor, e.g. `since=999999` on a
//! head-3 hub, or an old-epoch cursor `since=3` after a turnover to head-1)
//! is also `Lost` with no rows: the client is ahead of the retained window
//! and must re-sync from `floor_seq` (never retry the stale/future cursor).
//! `since == head` (up-to-date, no new rows) stays `None` empty — the only
//! empty-`None` page.
//!
//! Retry guidance (also emitted as `X-Resync-From` + `Retry-After` headers
//! on Lost over HTTP): on `Lost`, discard the old cursor and re-sync from
//! the returned `floor_seq` (`/api/events?since=<floor>`), then page forward
//! with `limit`. Never retry the same stale cursor — it stays `Lost`.
//!
//! V8 burst waiter guidance (for slow/burst consumers): on `Lost`, discard
//! the old cursor and re-sync from the returned `floor_seq`
//! (`/api/events?since=<floor>`), then page forward with `limit`. Never retry
//! the same stale cursor — it stays `Lost` with no rows by construction.
//! The ms1 gate pins this: burst 10 into cap-4 → `Lost` empty, resync from
//! floor pages exactly `cap` rows contiguous (`None`).
//!
//! V2 epoch fan-out residual (documented, not fenced stateless): the DB-epoch
//! one-shot `Lost` (F3 restore/re-init) is consumed by the FIRST `history()`
//! after the turnover. Concurrent SSE/history consumers fanning out from one
//! hub therefore see `Lost` exactly once *per hub*, not once per consumer —
//! a second consumer arriving after the flag is consumed but before resync
//! observes the new epoch via the floor check only when its cursor predates
//! the new floor. A stateless `since` cursor cannot distinguish old-epoch
//! `since=0` from new-epoch `since=0` (seq namespaces collide on restore),
//! so a full per-consumer fence would need an epoch id in the cursor
//! (`?epoch=` + `HistoryDto.epoch`) — deferred (V14-adjacent contract change,
//! not this hardening phase). The one-shot + floor check covers the
//! single-consumer gate and the rename-restore / same-inode-retreat legs;
//! concurrent-fan-out staleness after an epoch turnover is the documented
//! residual (see `v2_epoch_one_shot_and_fanout_residual`).
//!
//! Polling: 100ms CDC (`STUDIO_HUB_POLL_MS`), batch 512, per the projection
//! contract. A failed poll keeps the last good snapshot (fail closed: the
//! badge ages into `stale` honestly instead of serving an error as data).
//! The DB is opened read-only per poll, so the hub works with no daemon.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use studio_core::projection::Snapshot;
use studio_core::state::StateStore;

use crate::api::{CountsDto, EventDto, GapKind, HistoryDto, StatusDto, TaskDto};
use crate::staleness::{now_ms, staleness_of};

/// Default ring capacity (matches the CDC batch bound).
pub const RING_CAP_DEFAULT: usize = 512;
/// Default CDC poll interval (ms).
pub const POLL_MS_DEFAULT: u64 = 100;

pub fn ring_cap() -> usize {
    std::env::var("STUDIO_HUB_RING_CAP")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|n| *n >= 4)
        .unwrap_or(RING_CAP_DEFAULT)
}

pub fn poll_interval_ms() -> u64 {
    std::env::var("STUDIO_HUB_POLL_MS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|n| *n >= 5)
        .unwrap_or(POLL_MS_DEFAULT)
}

struct HubInner {
    ring: VecDeque<EventDto>,
    cap: usize,
    floor_seq: i64,
    head_seq: i64,
    last_ok_ms: Option<u64>,
    last_snapshot: Option<Snapshot>,
    db_path: String,
    stale_after_ms: u64,
    /// F3 DB-epoch guard: the file identity the ring was built from, plus a
    /// one-shot `Lost` flag consumed by the next history read after an epoch
    /// change (restore/re-init under a live server).
    db_id: Option<DbFileId>,
    epoch_gap: bool,
}

/// Identity of the live database *file object* (F3). Only `(dev, ino)`:
/// mtime/size change on every write and must NOT count as an epoch change.
/// A replaced file (rename-restore, fresh init at a new path) gets a new
/// inode; an in-place overwrite keeps the inode and is caught by the
/// max-seq retreat check in `poll_once` instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DbFileId {
    dev: u64,
    ino: u64,
}

fn file_id(path: &str) -> Option<DbFileId> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(path).ok().map(|m| DbFileId {
            dev: m.dev(),
            ino: m.ino(),
        })
    }
    #[cfg(not(unix))]
    {
        // No stable file identity: epoch changes are retreat-detected only
        // (documented residual; the Linux target has inode identity).
        let _ = path;
        None
    }
}

/// Cloneable handle: pollers, handlers, and SSE tasks share one hub.
#[derive(Clone)]
pub struct Hub {
    inner: Arc<Mutex<HubInner>>,
}

impl Hub {
    pub fn new(db_path: String, stale_after_ms: u64) -> Self {
        Self::new_with_cap(db_path, stale_after_ms, ring_cap().max(4))
    }

    /// Explicit capacity (tests; production reads `STUDIO_HUB_RING_CAP` via
    /// [`new`] so parallel tests never fight over process-global env).
    pub fn new_with_cap(db_path: String, stale_after_ms: u64, cap: usize) -> Self {
        let cap = cap.max(4);
        Self {
            inner: Arc::new(Mutex::new(HubInner {
                ring: VecDeque::with_capacity(cap),
                cap,
                floor_seq: 0,
                head_seq: 0,
                last_ok_ms: None,
                last_snapshot: None,
                db_path,
                stale_after_ms,
                db_id: None,
                epoch_gap: false,
            })),
        }
    }

    /// One CDC poll: read-only snapshot + `changes_since(head, 512)`.
    /// Ok(true) = fresh data; Ok(false) = DB unreadable (kept last good).
    /// Never panics on a missing/locked DB.
    pub fn poll_once(&self) -> bool {
        let (db_path, head) = {
            let guard = self.inner.lock().expect("hub mutex");
            (guard.db_path.clone(), guard.head_seq)
        };
        let store = match StateStore::open_readonly(&db_path) {
            Ok(s) => s,
            Err(_) => return false,
        };
        let snapshot = match store.snapshot() {
            Ok(s) => s,
            Err(_) => return false,
        };
        let changes = match store.changes_since(head, RING_CAP_DEFAULT) {
            Ok(c) => c,
            Err(_) => return false,
        };
        let mut guard = self.inner.lock().expect("hub mutex");
        let now = now_ms();
        // F3 epoch guard: a replaced DB file (new inode) or a retreating
        // max-seq (in-place restore with older content) invalidates the
        // ring — otherwise the hub would serve phantom old-epoch rows with
        // a stuck head and no Lost. Reset + surface Lost once.
        let fid = file_id(&db_path);
        let epoch_changed = match guard.db_id {
            None => false, // first successful poll binds the epoch
            Some(known) => fid.map(|f| f != known).unwrap_or(false),
        };
        let retreated = snapshot.change_seq < guard.head_seq;
        if epoch_changed || retreated {
            guard.ring.clear();
            guard.floor_seq = 0;
            guard.head_seq = snapshot.change_seq;
            guard.db_id = fid;
            guard.epoch_gap = true;
            // Backfill the new epoch's retained window so the post-Lost
            // cursor pages real rows (not an empty ring with a live head).
            if let Ok(backfill) = store.changes_since(0, guard.cap) {
                for c in backfill {
                    let ev = EventDto {
                        seq: c.seq,
                        tbl: c.tbl,
                        row: c.row,
                        op: c.op,
                        old: c.old,
                        new: c.new,
                    };
                    if ev.seq <= snapshot.change_seq {
                        if guard.ring.len() >= guard.cap {
                            guard.ring.pop_front();
                        }
                        guard.ring.push_back(ev);
                    }
                }
                guard.floor_seq = guard
                    .ring
                    .front()
                    .map(|e| e.seq - 1)
                    .unwrap_or(snapshot.change_seq);
            }
            guard.last_ok_ms = Some(now);
            guard.last_snapshot = Some(snapshot);
            return true;
        }
        guard.db_id = fid;
        for c in changes {
            let ev = EventDto {
                seq: c.seq,
                tbl: c.tbl,
                row: c.row,
                op: c.op,
                old: c.old,
                new: c.new,
            };
            if ev.seq <= guard.head_seq {
                continue;
            }
            guard.head_seq = ev.seq;
            if guard.ring.len() >= guard.cap {
                guard.ring.pop_front();
            }
            guard.ring.push_back(ev.clone());
            guard.floor_seq = guard
                .ring
                .front()
                .map(|e| e.seq - 1)
                .unwrap_or(guard.head_seq);
        }
        // Even with no new rows the read succeeded: refresh age + snapshot.
        guard.head_seq = guard.head_seq.max(snapshot.change_seq);
        guard.last_ok_ms = Some(now);
        guard.last_snapshot = Some(snapshot);
        true
    }

    /// Spawn the background CDC poller (100ms default). Returns the handle;
    /// dropping the hub does not stop it — abort the handle on shutdown.
    pub fn spawn_poller(&self) -> tokio::task::JoinHandle<()> {
        let hub = self.clone();
        let interval = poll_interval_ms();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_millis(interval));
            loop {
                tick.tick().await;
                // Poll on a blocking thread: rusqlite is synchronous.
                let h = hub.clone();
                let _ = tokio::task::spawn_blocking(move || h.poll_once()).await;
            }
        })
    }

    /// History cursor. `Lost` when the DB epoch turned over (one-shot, F3)
    /// or when `since` predates the retained floor (ring overflow) or runs
    /// ahead of `head_seq` (future cursor — never silent `None` empty).
    /// F2: negative cursors clamp to 0 — `Lost` requires a real floor>0 or a
    /// real future (`since > head`), never a nonsense cursor.
    /// Resync guidance: on `Lost` re-request from the returned `floor_seq`.
    pub fn history(&self, since: i64, limit: usize) -> HistoryDto {
        let mut guard = self.inner.lock().expect("hub mutex");
        if guard.epoch_gap {
            guard.epoch_gap = false;
            return HistoryDto {
                floor_seq: guard.floor_seq,
                head_seq: guard.head_seq,
                gap: GapKind::Lost,
                events: Vec::new(),
            };
        }
        let since = since.max(0);
        let limit = limit.clamp(1, 1024);
        let gap = if since < guard.floor_seq || since > guard.head_seq {
            GapKind::Lost
        } else {
            GapKind::None
        };
        let events: Vec<EventDto> = if gap == GapKind::Lost {
            Vec::new()
        } else {
            guard
                .ring
                .iter()
                .filter(|e| e.seq > since)
                .take(limit)
                .cloned()
                .collect()
        };
        HistoryDto {
            floor_seq: guard.floor_seq,
            head_seq: guard.head_seq,
            gap,
            events,
        }
    }

    /// Hub retention report for `/api/health`.
    pub fn hub_report(&self) -> crate::api::HubDto {
        let guard = self.inner.lock().expect("hub mutex");
        crate::api::HubDto {
            floor_seq: guard.floor_seq,
            head_seq: guard.head_seq,
            retained: guard.ring.len(),
            cap: guard.cap,
        }
    }

    /// Per-consumer sequence-gap predicate for badge parity (fix 8): true
    /// when `cursor` (clamped like [`history`]) predates the floor OR runs
    /// ahead of the head (future cursor), OR when an unconsumed epoch
    /// turnover is pending and a cursor was supplied (the next history read
    /// would be one-shot `Lost`). `None` (no cursor) is always age-only
    /// `false` by design — views without `?since=` stay honest age badges
    /// (documented, not faked Lost).
    pub fn seq_gap_for(&self, cursor: Option<i64>) -> bool {
        let guard = self.inner.lock().expect("hub mutex");
        let Some(c) = cursor else { return false };
        if guard.epoch_gap {
            return true;
        }
        let c = c.max(0);
        c < guard.floor_seq || c > guard.head_seq
    }

    /// Status payload for `/api/status`. `None` when no poll ever succeeded
    /// (DB missing at boot): the handler turns that into legible 503.
    /// F4: `cursor` threads the caller's event cursor into the taxonomy —
    /// `Some(c)` below the ring floor OR ahead of the head yields `Lost`
    /// (reachable over HTTP via `/api/status?since=`); `None` keeps the
    /// age-only verdict by design (other views document the same rule;
    /// `/api/health` always carries the absolute `db_path` while badges keep
    /// the short basename — see `db_source`).
    pub fn status_view(&self, daemon_reachable: bool, cursor: Option<i64>) -> Option<StatusDto> {
        let guard = self.inner.lock().expect("hub mutex");
        let snap = guard.last_snapshot.clone()?;
        let last_ok = guard.last_ok_ms?;
        let age = now_ms().saturating_sub(last_ok);
        // Future-cursor + epoch-stale cursors are Lost (fixes 1-2): a cursor
        // ahead of the head (future, or old-epoch cursor past the new head)
        // is never silent Fresh/None-empty.
        let seq_gap = match cursor {
            None => false,
            Some(c) => {
                if guard.epoch_gap {
                    true
                } else {
                    let c = c.max(0);
                    c < guard.floor_seq || c > guard.head_seq
                }
            }
        };
        let mut counts = CountsDto {
            building: 0,
            validating: 0,
            in_review: 0,
            ready: 0,
        };
        let tasks: Vec<TaskDto> = snap
            .fleet
            .iter()
            .map(|t| {
                match t.lane.as_str() {
                    "building" => counts.building += 1,
                    "validating" => counts.validating += 1,
                    "in-review" => counts.in_review += 1,
                    _ => counts.ready += 1,
                }
                TaskDto {
                    id: t.id.clone(),
                    kind: t.kind.clone(),
                    status: t.status.clone(),
                    lane: t.lane.clone(),
                }
            })
            .collect();
        Some(StatusDto {
            source: snap.source.clone(),
            db_age_ms: age,
            staleness: staleness_of(
                &snap.source,
                age,
                snap.change_seq,
                guard.stale_after_ms,
                seq_gap,
            ),
            change_seq: snap.change_seq,
            counts,
            tasks,
            daemon_reachable,
        })
    }

    /// Test hook: last successful poll time (None = never).
    #[cfg(test)]
    pub fn last_ok_ms(&self) -> Option<u64> {
        self.inner.lock().expect("hub mutex").last_ok_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use studio_core::state::StateStore;

    fn seeded_db(n_tasks: usize) -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("studio.db");
        let p = path.to_str().unwrap().to_owned();
        let store = StateStore::open(&p).unwrap();
        // NOTE: `change_log` has triggers on `tasks` only — drive task
        // rows (not events) to exercise the CDC cursor.
        for i in 0..n_tasks.max(1) {
            store
                .upsert_task(&format!("t{i}"), "coder", "building")
                .unwrap();
        }
        (dir, p)
    }

    #[test]
    fn poll_serves_snapshot_and_history_cursor() {
        let (_dir, p) = seeded_db(3);
        let hub = Hub::new(p, 2000);
        assert!(hub.poll_once());
        let h = hub.history(0, 512);
        assert_eq!(h.gap, GapKind::None);
        assert_eq!(h.events.len(), 3);
        assert_eq!(h.head_seq, 3);
        let view = hub.status_view(true, None).unwrap();
        assert_eq!(view.tasks.len(), 3);
        assert_eq!(view.counts.building, 3);
        assert_eq!(view.staleness.state, crate::api::StalenessState::Fresh);
    }

    #[test]
    fn db_replace_resets_ring_and_surfaces_lost_once() {
        // F3: restore/re-init under a live server must not serve phantom
        // old-epoch rows with a stuck head and no Lost.
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("studio.db");
        let p = db.to_str().unwrap().to_owned();
        let store = StateStore::open(&p).unwrap();
        for i in 0..6 {
            store
                .upsert_task(&format!("old-{i}"), "coder", "building")
                .unwrap();
        }
        drop(store);
        let hub = Hub::new_with_cap(p.clone(), 2000, 512);
        assert!(hub.poll_once());
        assert_eq!(hub.history(0, 512).events.len(), 6);
        // New epoch, new inode, fewer rows (rename-restore).
        let fresh = dir.path().join("fresh.db");
        let s2 = StateStore::open(fresh.to_str().unwrap()).unwrap();
        s2.upsert_task("new-0", "coder", "done").unwrap();
        drop(s2);
        std::fs::rename(&fresh, &db).unwrap();
        // Sidecars don't survive a restore: drop the old epoch's WAL/SHM so
        // no stale frame can apply to the new main file.
        for suffix in ["-wal", "-shm", "-journal"] {
            let sidecar = format!("{p}{suffix}");
            let _ = std::fs::remove_file(sidecar);
        }
        assert!(hub.poll_once());
        // Lost surfaces exactly once, with no fabricated rows...
        let first = hub.history(0, 512);
        assert_eq!(first.gap, GapKind::Lost);
        assert!(first.events.is_empty());
        let second = hub.history(0, 512);
        assert_eq!(second.gap, GapKind::None);
        // ...then the hub serves the NEW epoch only (no phantoms, no stuck head).
        assert_eq!(second.events.len(), 1);
        let view = hub.status_view(true, None).unwrap();
        assert_eq!(view.tasks.len(), 1);
        assert_eq!(view.tasks[0].id, "new-0");
        assert_eq!(view.change_seq, 1);
    }

    #[test]
    fn db_in_place_overwrite_with_retreating_seq_resets() {
        // F3 second leg: same inode, older content (cp-overwrite restore).
        // The inode guard cannot see it; the max-seq retreat check must.
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("studio.db");
        let p = db.to_str().unwrap().to_owned();
        let store = StateStore::open(&p).unwrap();
        for i in 0..6 {
            store
                .upsert_task(&format!("old-{i}"), "coder", "building")
                .unwrap();
        }
        drop(store);
        let hub = Hub::new_with_cap(p.clone(), 2000, 512);
        assert!(hub.poll_once());
        let fresh = dir.path().join("fresh.db");
        let s2 = StateStore::open(fresh.to_str().unwrap()).unwrap();
        s2.upsert_task("new-0", "coder", "done").unwrap();
        drop(s2);
        std::fs::copy(&fresh, &db).unwrap();
        for suffix in ["-wal", "-shm", "-journal"] {
            let _ = std::fs::remove_file(format!("{p}{suffix}"));
        }
        assert!(hub.poll_once());
        let first = hub.history(0, 512);
        assert_eq!(first.gap, GapKind::Lost);
        let view = hub.status_view(true, None).unwrap();
        assert_eq!(view.tasks.len(), 1);
        assert_eq!(view.tasks[0].id, "new-0");
    }

    #[test]
    fn missing_db_keeps_last_good_and_never_panics() {
        let hub = Hub::new("/nonexistent-dir-xyz/studio.db".into(), 2000);
        assert!(!hub.poll_once());
        assert!(hub.status_view(false, None).is_none());
        let h = hub.history(0, 10);
        assert_eq!(h.events.len(), 0);
        assert_eq!(h.gap, GapKind::None);
    }

    #[test]
    fn ring_overflow_reports_lost_never_silent() {
        let (_dir, p) = seeded_db(10);
        let hub = Hub::new_with_cap(p, 2000, 4);
        assert!(hub.poll_once());
        let rep = hub.hub_report();
        assert!(rep.retained <= 4);
        // A cursor from before the floor is Lost with no fabricated rows.
        let lost = hub.history(0, 512);
        assert_eq!(lost.gap, GapKind::Lost);
        assert!(lost.events.is_empty());
        // A cursor at the floor still pages contiguously.
        let ok = hub.history(lost.floor_seq, 512);
        assert_eq!(ok.gap, GapKind::None);
        assert!(!ok.events.is_empty());
    }

    #[test]
    fn negative_cursor_clamps_to_zero_never_spurious_lost() {
        // F2: since<0 must behave as since=0 (Lost only on a real floor>0).
        let (_dir, p) = seeded_db(10);
        let hub = Hub::new(p.clone(), 2000);
        assert!(hub.poll_once());
        let h = hub.history(-5, 512);
        assert_eq!(h.gap, GapKind::None);
        assert_eq!(h.events.len(), 10);
        // ...while a genuinely overflowed cursor still reports Lost.
        let hub4 = Hub::new_with_cap(p, 2000, 4);
        assert!(hub4.poll_once());
        assert_eq!(hub4.history(0, 512).gap, GapKind::Lost);
        // ...and the clamped negative cursor on the overflowed ring reports
        // Lost CORRECTLY (0 genuinely predates the floor — not spurious).
        assert_eq!(hub4.history(-5, 512).gap, GapKind::Lost);
    }

    #[test]
    fn status_cursor_below_floor_is_lost_over_the_taxonomy() {
        // F4: StalenessState::Lost is reachable via ?since= (per-consumer gap).
        let (_dir, p) = seeded_db(10);
        let hub = Hub::new_with_cap(p, 2000, 4);
        assert!(hub.poll_once());
        let floor = hub.history(0, 512).floor_seq;
        assert!(floor > 0);
        let lost = hub.status_view(true, Some(0)).unwrap();
        assert_eq!(lost.staleness.state, crate::api::StalenessState::Lost);
        assert!(lost.staleness.label.starts_with("lost · "));
        let fresh = hub.status_view(true, None).unwrap();
        assert_eq!(fresh.staleness.state, crate::api::StalenessState::Fresh);
        let at_floor = hub.status_view(true, Some(floor)).unwrap();
        assert_eq!(at_floor.staleness.state, crate::api::StalenessState::Fresh);
        // Negative cursor clamps (F2 consistency): never Lost on floor 0...
        // (here floor>0, so Some(-5)->0 still predates the floor -> Lost is
        // CORRECT: the clamped cursor genuinely lags the ring).
        let neg = hub.status_view(true, Some(-5)).unwrap();
        assert_eq!(neg.staleness.state, crate::api::StalenessState::Lost);
    }

    #[test]
    fn x1_history_gap_oracle_empty_on_lost_contiguous_on_none() {
        // X1 oracle: gap semantics are total — Lost ⟺ empty page (never
        // silent truncation, never fabricated rows); None ⟺ contiguous
        // (since, head] coverage in seq order.
        let (_dir, p) = seeded_db(10);
        let hub = Hub::new_with_cap(p, 2000, 4);
        assert!(hub.poll_once());
        let lost = hub.history(0, 512);
        assert_eq!(lost.gap, GapKind::Lost);
        assert!(lost.events.is_empty(), "Lost must carry no rows");
        let floor = lost.floor_seq;
        let head = lost.head_seq;
        assert!(floor > 0 && head == 10);
        let page = hub.history(floor, 512);
        assert_eq!(page.gap, GapKind::None);
        assert!(!page.events.is_empty());
        // Contiguity: strictly increasing seqs within (since, head].
        let mut prev = floor;
        for e in &page.events {
            assert!(e.seq > prev, "seqs must increase contiguously");
            assert!(e.seq <= head);
            prev = e.seq;
        }
        // Full-ring oracle on a wide ring: no gap, all rows, ordered.
        let (_dir2, p2) = seeded_db(6);
        let hub2 = Hub::new_with_cap(p2, 2000, 512);
        assert!(hub2.poll_once());
        let full = hub2.history(0, 512);
        assert_eq!(full.gap, GapKind::None);
        assert_eq!(full.events.len(), 6);
        let mut prev = 0;
        for e in &full.events {
            assert!(e.seq > prev);
            prev = e.seq;
        }
        assert_eq!(full.head_seq, 6);
    }

    #[test]
    fn v9_limit_clamp_pin_zero_and_huge() {
        // V9 limit clamp pin: history() clamps limit to [1, 1024] —
        // limit=0 behaves as 1 (never an empty-by-clamp page), huge limits
        // cap without panic, limit=1 pages exactly one row.
        let (_dir, p) = seeded_db(10);
        let hub = Hub::new_with_cap(p, 2000, 512);
        assert!(hub.poll_once());
        let zero = hub.history(0, 0);
        assert_eq!(zero.gap, GapKind::None);
        assert_eq!(zero.events.len(), 1, "limit 0 must clamp to 1");
        let one = hub.history(0, 1);
        assert_eq!(one.events.len(), 1);
        assert_eq!(one.events[0].seq, 1);
        let huge = hub.history(0, 999_999);
        assert_eq!(huge.gap, GapKind::None);
        assert_eq!(huge.events.len(), 10, "huge limit caps, serves retained");
        // Clamp holds on an overflowed ring too (Lost still empty).
        let (_dir2, p2) = seeded_db(10);
        let hub4 = Hub::new_with_cap(p2, 2000, 4);
        assert!(hub4.poll_once());
        let lost_zero = hub4.history(0, 0);
        assert_eq!(lost_zero.gap, GapKind::Lost);
        assert!(lost_zero.events.is_empty());
        let floor = hub4.history(0, 512).floor_seq;
        let resync = hub4.history(floor, 999_999);
        assert_eq!(resync.gap, GapKind::None);
        assert_eq!(resync.events.len(), 4);
    }

    #[test]
    fn v8_burst_waiter_resync_guidance() {
        // V8 burst waiter guidance pin: burst 10 into cap-4 → Lost empty;
        // waiter resyncs from floor_seq → exactly cap rows, contiguous,
        // None gap. Retrying the stale cursor stays Lost (never heals).
        let (_dir, p) = seeded_db(10);
        let hub = Hub::new_with_cap(p, 2000, 4);
        assert!(hub.poll_once());
        let rep = hub.hub_report();
        assert_eq!(rep.cap, 4);
        assert!(rep.retained <= 4);
        let lost = hub.history(0, 512);
        assert_eq!(lost.gap, GapKind::Lost);
        assert!(lost.events.is_empty());
        // Retry of the stale cursor: still Lost (waiter must resync).
        let retry = hub.history(0, 512);
        // NOTE: epoch_gap is false here (no DB turnover), so the floor
        // check alone keeps this Lost — the pin is the stable floor.
        assert_eq!(retry.gap, GapKind::Lost);
        assert!(retry.events.is_empty());
        // Guidance: resync from floor pages exactly cap rows.
        let page = hub.history(lost.floor_seq, 512);
        assert_eq!(page.gap, GapKind::None);
        assert_eq!(page.events.len(), 4);
        assert_eq!(page.head_seq, 10);
        // Paging with limit=2 walks the retained window contiguously.
        let p1 = hub.history(lost.floor_seq, 2);
        assert_eq!(p1.events.len(), 2);
        let p2 = hub.history(p1.events[1].seq, 2);
        assert_eq!(p2.events.len(), 2);
        assert_eq!(p2.events[1].seq, 10);
    }

    #[test]
    fn v2_epoch_one_shot_and_fanout_residual() {
        // V2 epoch fan-out: the one-shot Lost fires exactly once per hub
        // after a DB turnover (F3), then the new epoch serves real rows.
        // DOCUMENTED RESIDUAL: a second concurrent consumer arriving after
        // the flag is consumed observes the new epoch WITHOUT its own Lost
        // (stateless since-cursor cannot name the epoch) — full per-consumer
        // fencing needs ?epoch= + HistoryDto.epoch (deferred contract change).
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("studio.db");
        let p = db.to_str().unwrap().to_owned();
        let store = StateStore::open(&p).unwrap();
        for i in 0..4 {
            store
                .upsert_task(&format!("old-{i}"), "coder", "building")
                .unwrap();
        }
        drop(store);
        let hub = Hub::new_with_cap(p.clone(), 2000, 512);
        assert!(hub.poll_once());
        assert_eq!(hub.history(0, 512).events.len(), 4);
        let fresh = dir.path().join("fresh.db");
        let s2 = StateStore::open(fresh.to_str().unwrap()).unwrap();
        s2.upsert_task("new-0", "coder", "done").unwrap();
        drop(s2);
        std::fs::rename(&fresh, &db).unwrap();
        for suffix in ["-wal", "-shm", "-journal"] {
            let _ = std::fs::remove_file(format!("{p}{suffix}"));
        }
        assert!(hub.poll_once());
        // First consumer after turnover: Lost once, no rows.
        let first = hub.history(0, 512);
        assert_eq!(first.gap, GapKind::Lost);
        assert!(first.events.is_empty());
        // Second consumer (fan-out): flag consumed — sees new epoch rows
        // with None (the residual: no per-consumer Lost without an epoch id).
        let second = hub.history(0, 512);
        assert_eq!(second.gap, GapKind::None);
        assert_eq!(second.events.len(), 1);
        assert_eq!(second.events[0].row, "new-0");
    }

    #[test]
    fn future_cursor_is_lost_not_silent_none() {
        // Fix 1: a cursor ahead of the head (future) must be Lost empty,
        // never silent None empty; resync from floor heals. FAILS pre-fix
        // (since=999999 on head-3 returned None empty).
        let (_dir, p) = seeded_db(3);
        let hub = Hub::new_with_cap(p, 2000, 512);
        assert!(hub.poll_once());
        let head = hub.hub_report().head_seq;
        assert_eq!(head, 3);
        let future = hub.history(999_999, 512);
        assert_eq!(future.gap, GapKind::Lost, "future cursor must be Lost");
        assert!(future.events.is_empty(), "Lost carries no rows");
        // Up-to-date cursor (since==head) stays None empty — the only
        // empty-None page.
        let current = hub.history(head, 512);
        assert_eq!(current.gap, GapKind::None);
        assert!(current.events.is_empty());
        // Resync from floor heals to None with rows.
        let healed = hub.history(future.floor_seq, 512);
        assert_eq!(healed.gap, GapKind::None);
        assert_eq!(healed.events.len(), 3);
        // Status mirrors the future as Lost (fixes 1-2).
        let lost = hub.status_view(true, Some(999_999)).unwrap();
        assert_eq!(lost.staleness.state, crate::api::StalenessState::Lost);
        let fresh = hub.status_view(true, Some(head)).unwrap();
        assert_eq!(fresh.staleness.state, crate::api::StalenessState::Fresh);
    }

    #[test]
    fn epoch_stale_cursor_after_turnover_is_lost() {
        // Fix 2: an old-epoch cursor past the new head (e.g. since=3 after a
        // turnover to head-1) must be Lost, never None-empty/Fresh. FAILS
        // pre-fix (3 > 1 returned None empty + Fresh status).
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("studio.db");
        let p = db.to_str().unwrap().to_owned();
        let store = StateStore::open(&p).unwrap();
        for i in 0..4 {
            store
                .upsert_task(&format!("old-{i}"), "coder", "building")
                .unwrap();
        }
        drop(store);
        let hub = Hub::new_with_cap(p.clone(), 2000, 512);
        assert!(hub.poll_once());
        assert_eq!(hub.hub_report().head_seq, 4);
        let fresh = dir.path().join("fresh.db");
        let s2 = StateStore::open(fresh.to_str().unwrap()).unwrap();
        s2.upsert_task("new-0", "coder", "done").unwrap();
        drop(s2);
        std::fs::rename(&fresh, &db).unwrap();
        for suffix in ["-wal", "-shm", "-journal"] {
            let _ = std::fs::remove_file(format!("{p}{suffix}"));
        }
        assert!(hub.poll_once());
        // Consume the one-shot so the floor/future check is the fence.
        let first = hub.history(0, 512);
        assert_eq!(first.gap, GapKind::Lost);
        // Old-head cursors are future relative to the new head → Lost.
        for stale in [3, 4] {
            let h = hub.history(stale, 512);
            assert_eq!(h.gap, GapKind::Lost, "stale since={stale} must be Lost");
            assert!(h.events.is_empty());
            let s = hub.status_view(true, Some(stale)).unwrap();
            assert_eq!(
                s.staleness.state,
                crate::api::StalenessState::Lost,
                "status since={stale} must be Lost"
            );
        }
        // seq_gap_for mirrors the same fence (fix 8 helper).
        assert!(hub.seq_gap_for(Some(3)));
        assert!(!hub.seq_gap_for(Some(0)));
        assert!(!hub.seq_gap_for(None));
    }
}
