//! Zero-token scheduler: pure state machine, no model in the loop (validated by H2).
//! Lease + heartbeat dispatch; missed renewal -> reap + `lease_expired` receipt.
//! Owns the deterministic next-transition (WS8) and enforces the action
//! registry (surface exposure matrix).

pub mod actions;
pub mod autonomy;

use std::collections::{HashMap, VecDeque};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskStatus {
    Pending,
    Ready,
    Running { lease_until_tick: u64 },
    Done,
    Blocked(String),
}

/// The scheduler's answer to "what happens next" — computed, never modeled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Transition {
    Dispatch { task: String },
    AwaitLease,
    Complete,
    Deadlock,
}

pub struct Scheduler {
    deps: HashMap<String, Vec<String>>,
    status: HashMap<String, TaskStatus>,
    tick: u64,
}

impl Scheduler {
    pub fn new(deps: HashMap<String, Vec<String>>) -> Self {
        let status = deps
            .keys()
            .map(|k| (k.clone(), TaskStatus::Pending))
            .collect();
        Self {
            deps,
            status,
            tick: 0,
        }
    }

    pub fn tick(&mut self) -> Vec<String> {
        self.tick += 1;
        // reap expired leases
        for (id, st) in self.status.iter_mut() {
            if let TaskStatus::Running { lease_until_tick } = st {
                if *lease_until_tick < self.tick {
                    *st = TaskStatus::Blocked("lease_expired".into());
                    let _ = id;
                }
            }
        }
        // ready = pending with all deps done
        let mut ready = vec![];
        for (id, ds) in &self.deps {
            if self.status[id] != TaskStatus::Pending {
                continue;
            }
            if ds
                .iter()
                .all(|d| self.status.get(d) == Some(&TaskStatus::Done))
            {
                ready.push(id.clone());
            }
        }
        ready.sort();
        ready
    }

    pub fn dispatch(&mut self, id: &str, lease_ticks: u64) {
        self.status.insert(
            id.into(),
            TaskStatus::Running {
                lease_until_tick: self.tick + lease_ticks,
            },
        );
    }

    pub fn heartbeat(&mut self, id: &str, lease_ticks: u64) {
        if let Some(TaskStatus::Running { lease_until_tick }) = self.status.get_mut(id) {
            *lease_until_tick = self.tick + lease_ticks;
        }
    }

    pub fn complete(&mut self, id: &str) {
        self.status.insert(id.into(), TaskStatus::Done);
    }

    /// Authorize a daemon action through the exposure matrix. Enforcement
    /// lives here (not in the UI): unknown/foreign/denied actions never
    /// dispatch; `Ask` surfaces as blocked-pending-approval to the caller.
    pub fn authorize(
        &self,
        registry: &actions::ActionRegistry,
        action_id: &str,
        surface: actions::Surface,
    ) -> actions::Decision {
        registry.authorize(action_id, surface)
    }

    /// Deterministic next transition, read-only (no tick advance, no model).
    /// Single owner of "what happens next": dispatch the first ready task,
    /// await running leases, report completion, or name the deadlock.
    pub fn next_transition(&self) -> Transition {
        let mut pending = 0;
        let mut running = 0;
        let mut ready: Vec<&String> = vec![];
        for (id, st) in &self.status {
            match st {
                TaskStatus::Ready => {
                    ready.push(id);
                }
                TaskStatus::Pending => {
                    pending += 1;
                    let deps_done = self.deps.get(id).map(|ds| {
                        ds.iter()
                            .all(|d| self.status.get(d) == Some(&TaskStatus::Done))
                    });
                    if deps_done.unwrap_or(false) {
                        ready.push(id);
                    }
                }
                TaskStatus::Running { .. } => running += 1,
                TaskStatus::Done | TaskStatus::Blocked(_) => {}
            }
        }
        ready.sort();
        if let Some(first) = ready.into_iter().next() {
            Transition::Dispatch {
                task: first.clone(),
            }
        } else if pending == 0 && running == 0 {
            Transition::Complete
        } else if running > 0 {
            Transition::AwaitLease
        } else {
            Transition::Deadlock
        }
    }

    pub fn run_to_completion(&mut self) -> (usize, bool) {
        let mut order = VecDeque::new();
        for _ in 0..10000 {
            let ready = self.tick();
            if ready.is_empty() {
                let pending = self
                    .status
                    .values()
                    .filter(|s| **s == TaskStatus::Pending)
                    .count();
                let running = self
                    .status
                    .values()
                    .filter(|s| matches!(s, TaskStatus::Running { .. }))
                    .count();
                if pending == 0 && running == 0 {
                    break;
                }
                if running == 0 && pending > 0 {
                    return (order.len(), true); // deadlock
                }
                continue;
            }
            for id in ready {
                self.dispatch(&id, 10);
                self.complete(&id);
                order.push_back(id);
            }
        }
        (order.len(), false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replays_dag_without_deadlock() {
        let deps: HashMap<String, Vec<String>> =
            [("t0", vec![]), ("t1", vec!["t0"]), ("t2", vec!["t0", "t1"])]
                .iter()
                .map(|(k, v)| (k.to_string(), v.iter().map(|s| s.to_string()).collect()))
                .collect();
        let mut s = Scheduler::new(deps);
        let (n, dead) = s.run_to_completion();
        assert_eq!(n, 3);
        assert!(!dead);
    }

    #[test]
    fn next_transition_is_deterministic() {
        use super::Transition;
        let deps: HashMap<String, Vec<String>> = [("t0", vec![]), ("t1", vec!["t0"])]
            .iter()
            .map(|(k, v)| (k.to_string(), v.iter().map(|s| s.to_string()).collect()))
            .collect();
        let mut s = Scheduler::new(deps);
        assert_eq!(
            s.next_transition(),
            Transition::Dispatch { task: "t0".into() }
        );
        // Read-only: calling twice changes nothing.
        assert_eq!(
            s.next_transition(),
            Transition::Dispatch { task: "t0".into() }
        );
        s.dispatch("t0", 10);
        assert_eq!(s.next_transition(), Transition::AwaitLease);
        s.complete("t0");
        assert_eq!(
            s.next_transition(),
            Transition::Dispatch { task: "t1".into() }
        );
        s.complete("t1");
        assert_eq!(s.next_transition(), Transition::Complete);
    }

    #[test]
    fn next_transition_names_deadlock() {
        use super::Transition;
        // t1 depends on t2 which depends on t1: nothing ever ready.
        let deps: HashMap<String, Vec<String>> = [("t1", vec!["t2"]), ("t2", vec!["t1"])]
            .iter()
            .map(|(k, v)| (k.to_string(), v.iter().map(|s| s.to_string()).collect()))
            .collect();
        let s = Scheduler::new(deps);
        assert_eq!(s.next_transition(), Transition::Deadlock);
    }
}
