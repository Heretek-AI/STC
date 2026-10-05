//! MCP process pool: socket-proxy/HTTP lifecycle shape, scope launcher,
//! FD-leak regression accounting.
//!
//! Clean-room re-derivation (asheshgoplani/agent-deck, MIT) behind
//! citations c025
//! (`b037e3a1d7ca6dc1fe23845eff87ca58cc9351e8ad5fd4f0f0cd91204f42bf65`)
//! and c026
//! (`00bd17ff74dee9170bdbf9c28beea40999a28fb883193e4294001843c2b5b315`).
//! No upstream code is copied: the re-derived rules are — workers spawn
//! through a scope launcher (every worker is bound to the scope that
//! requested it, never ambient); lifecycle is start/stop with no orphaned
//! worker surviving pool shutdown; every acquired descriptor is released
//! on drop (FD accounting returns to baseline — the regression test pins
//! this over N launch/stop cycles).
//!
//! In-process by charter (V3: no network, <20s): workers are handles, not
//! subprocesses; the lifecycle + accounting shape is what the pool ports,
//! not socket code. All tool calls through pool workers still cross the
//! single ordered egress seam (`Registry::invoke` with held lens, bound
//! catalog, fresh receipt, and canonical containment) — the pool never
//! executes tools itself.

use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use thiserror::Error;

/// Typed pool failures. No `String` errors cross this boundary.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PoolError {
    #[error("pool is shut down: no new workers launch")]
    ShutDown,
    #[error("unknown worker {id}")]
    UnknownWorker { id: u64 },
    #[error("pool over capacity: {live}/{cap} workers live")]
    OverCapacity { live: usize, cap: usize },
}

pub const MAX_WORKERS: usize = 8;

/// One pooled worker: id + bound scope. The scope binds the worker to the
/// requesting context (P02 spawn non-escalation applies: a worker never
/// serves a scope outside its launch binding — enforced by
/// [`Pool::check_scope`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerHandle {
    pub id: u64,
    pub scope: String,
}

/// FD accounting ledger: acquired vs released descriptor counts. The
/// regression invariant is `acquired == released` after every cycle.
#[derive(Debug, Default)]
pub struct FdLedger {
    pub acquired: u64,
    pub released: u64,
}

impl FdLedger {
    pub fn live(&self) -> u64 {
        self.acquired.saturating_sub(self.released)
    }
}

/// The pool: lifecycle owner + FD ledger. Workers hold an `Arc` clone of
/// the ledger and release exactly one descriptor on drop (RAII — a panic
/// path cannot leak the count).
#[derive(Debug)]
pub struct Pool {
    next_id: u64,
    live: HashMap<u64, WorkerHandle>,
    ledger: Arc<std::sync::Mutex<FdLedger>>,
    shut_down: bool,
    epoch_fds: Vec<u64>,
}

static POOL_SEQ: AtomicU64 = AtomicU64::new(1);

/// RAII worker guard: releasing the FD on drop is what makes the
/// no-leak invariant panic-safe.
#[derive(Debug)]
pub struct PooledWorker {
    pub handle: WorkerHandle,
    ledger: Arc<std::sync::Mutex<FdLedger>>,
}

impl Drop for PooledWorker {
    fn drop(&mut self) {
        self.ledger.lock().unwrap().released += 1;
    }
}

impl Pool {
    pub fn new() -> Self {
        Self {
            next_id: POOL_SEQ.fetch_add(1, Ordering::SeqCst),
            live: HashMap::new(),
            ledger: Arc::new(std::sync::Mutex::new(FdLedger::default())),
            shut_down: false,
            epoch_fds: vec![],
        }
    }

    /// Scope launcher: start one worker bound to `scope`. Shutdown pools
    /// and over-capacity pools refuse typed.
    pub fn launch(&mut self, scope: &str) -> Result<PooledWorker, PoolError> {
        if self.shut_down {
            return Err(PoolError::ShutDown);
        }
        if self.live.len() >= MAX_WORKERS {
            return Err(PoolError::OverCapacity {
                live: self.live.len(),
                cap: MAX_WORKERS,
            });
        }
        let id = self.next_id;
        self.next_id += 1;
        let handle = WorkerHandle {
            id,
            scope: scope.into(),
        };
        self.live.insert(id, handle.clone());
        self.ledger.lock().unwrap().acquired += 1;
        Ok(PooledWorker {
            handle,
            ledger: self.ledger.clone(),
        })
    }

    /// Stop one worker: removes the live record (the FD release happens on
    /// the guard's drop — both must happen for the cycle to close).
    pub fn stop(&mut self, worker: PooledWorker) -> Result<(), PoolError> {
        let id = worker.handle.id;
        if self.live.remove(&id).is_none() {
            return Err(PoolError::UnknownWorker { id });
        }
        drop(worker);
        Ok(())
    }

    /// Scope check: a worker serves only its launch scope.
    pub fn check_scope(&self, id: u64, scope: &str) -> Result<(), PoolError> {
        match self.live.get(&id) {
            None => Err(PoolError::UnknownWorker { id }),
            Some(h) if h.scope == scope => Ok(()),
            Some(_) => Err(PoolError::UnknownWorker { id }),
        }
    }

    /// Snapshot the live-FD count (cycle baseline probe).
    pub fn live_fds(&self) -> u64 {
        self.ledger.lock().unwrap().live()
    }

    pub fn live_workers(&self) -> usize {
        self.live.len()
    }

    /// Shutdown: no new launches; live workers at shutdown time are
    /// recorded (orphan check — the regression test asserts zero).
    pub fn shutdown(&mut self) -> usize {
        self.shut_down = true;
        self.epoch_fds.push(self.live_fds());
        self.live.len()
    }
}

impl Default for Pool {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_binds_scope_and_refuses_after_shutdown() {
        let mut pool = Pool::new();
        let w = pool.launch("coder-web").unwrap();
        assert_eq!(pool.live_workers(), 1);
        assert!(pool.check_scope(w.handle.id, "coder-web").is_ok());
        assert!(pool.check_scope(w.handle.id, "other-scope").is_err());
        pool.stop(w).unwrap();
        assert_eq!(pool.live_workers(), 0);
        assert_eq!(pool.live_fds(), 0);
        assert_eq!(pool.shutdown(), 0);
        assert!(matches!(
            pool.launch("coder-web").unwrap_err(),
            PoolError::ShutDown
        ));
    }

    #[test]
    fn over_capacity_refuses_typed() {
        let mut pool = Pool::new();
        let mut guards = Vec::new();
        for _ in 0..MAX_WORKERS {
            guards.push(pool.launch("s").unwrap());
        }
        assert!(matches!(
            pool.launch("s").unwrap_err(),
            PoolError::OverCapacity { .. }
        ));
        for g in guards {
            pool.stop(g).unwrap();
        }
        assert_eq!(pool.live_fds(), 0);
    }

    #[test]
    fn fd_ledger_returns_to_baseline_over_cycles() {
        // FD-leak regression: 25 launch/stop cycles leave zero live FDs
        // and zero live workers; shutdown orphans zero.
        let mut pool = Pool::new();
        for _ in 0..25 {
            let w = pool.launch("cycle").unwrap();
            pool.stop(w).unwrap();
        }
        assert_eq!(pool.live_fds(), 0);
        assert_eq!(pool.live_workers(), 0);
        assert_eq!(pool.shutdown(), 0);
    }
}
