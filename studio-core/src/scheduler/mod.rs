//! Zero-token scheduler: pure state machine, no model in the loop (validated by H2).
//! Lease + heartbeat dispatch; missed renewal -> reap + `lease_expired` receipt.

use std::collections::{HashMap, VecDeque};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskStatus {
    Pending,
    Ready,
    Running { lease_until_tick: u64 },
    Done,
    Blocked(String),
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
}
