//! Anti-stall supervisors: heartbeat leases, loop breakers, verified counters.
//! Every counter is unit-tested to increment (munder-difflin `HOP_CAP` lesson:
//! a limit checked against a field that never increments is no limit).

use std::collections::{HashMap, VecDeque};

/// 3-class stall taxonomy (generic / no-session / stale-heartbeat).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StallClass {
    Generic,
    NoSession,
    StaleHeartbeat,
}

#[derive(Debug, Default)]
pub struct Counters {
    pub heartbeats: u64,
    pub evictions: u64,
    pub loop_breaks: u64,
    pub budget_exhaustions: u64,
}

pub struct StallSupervisor {
    /// session -> last heartbeat tick
    last_beat: HashMap<String, u64>,
    tick: u64,
    max_missed: u64,
    pub counters: Counters,
}

impl StallSupervisor {
    pub fn new(max_missed: u64) -> Self {
        Self {
            last_beat: HashMap::new(),
            tick: 0,
            max_missed,
            counters: Counters::default(),
        }
    }

    pub fn beat(&mut self, session: &str) {
        self.last_beat.insert(session.into(), self.tick);
        self.counters.heartbeats += 1;
    }

    pub fn advance(&mut self) -> Vec<(String, StallClass)> {
        self.tick += 1;
        let mut evicted = vec![];
        let stale: Vec<String> = self
            .last_beat
            .iter()
            .filter(|(_, &t)| self.tick.saturating_sub(t) > self.max_missed)
            .map(|(s, _)| s.clone())
            .collect();
        for s in stale {
            self.last_beat.remove(&s);
            self.counters.evictions += 1;
            evicted.push((s, StallClass::StaleHeartbeat));
        }
        evicted
    }

    pub fn classify_no_session(&self, has_session: bool) -> Option<StallClass> {
        if has_session {
            None
        } else {
            Some(StallClass::NoSession)
        }
    }
}

/// Doom-loop detector: no file delta / repeated tool signature / burn-with-no-commit.
/// Bounded resume budget; terminal `blocked` receipt on exhaustion.
pub struct DoomLoopDetector {
    /// recent tool signatures (bounded window)
    window: VecDeque<String>,
    window_cap: usize,
    /// commits seen vs tool calls (burn-with-no-commit)
    tool_calls: u64,
    commits: u64,
    /// per-role tool-loop budgets
    budgets: HashMap<String, u64>,
    pub counters: Counters,
}

impl DoomLoopDetector {
    pub fn new(window_cap: usize) -> Self {
        Self {
            window: VecDeque::new(),
            window_cap,
            tool_calls: 0,
            commits: 0,
            budgets: HashMap::new(),
            counters: Counters::default(),
        }
    }

    pub fn set_budget(&mut self, role: &str, budget: u64) {
        self.budgets.insert(role.into(), budget);
    }

    /// Record a tool call. Returns Some(block reason) when loop breaker trips.
    pub fn record_tool(
        &mut self,
        role: &str,
        signature: &str,
        file_delta: bool,
        committed: bool,
    ) -> Option<String> {
        self.tool_calls += 1;
        if committed {
            self.commits += 1;
        }
        self.window.push_back(signature.into());
        while self.window.len() > self.window_cap {
            self.window.pop_front();
        }
        // repeated tool signature: entire window identical
        if self.window.len() == self.window_cap && self.window.iter().all(|s| s == signature) {
            self.counters.loop_breaks += 1;
            return Some(format!(
                "doom-loop: repeated tool signature {signature} x{}",
                self.window_cap
            ));
        }
        // burn-with-no-commit: many calls, zero commits, no delta
        if !file_delta && !committed && self.tool_calls >= 20 && self.commits == 0 {
            self.counters.loop_breaks += 1;
            return Some("doom-loop: burn-with-no-commit (20 calls, 0 commits, no delta)".into());
        }
        // per-role budget
        if let Some(b) = self.budgets.get(role) {
            let used: u64 = self.window.iter().filter(|_| true).count() as u64;
            let _ = used;
            // budget counts total calls per role window lifetime (simplified: global calls vs budget)
            if self.tool_calls > *b {
                self.counters.budget_exhaustions += 1;
                return Some(format!("tool-loop budget exhausted for {role} (>{b})"));
            }
        }
        None
    }

    pub fn record_commit(&mut self) {
        self.commits += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heartbeat_counters_increment() {
        let mut s = StallSupervisor::new(2);
        s.beat("a");
        s.beat("a");
        assert_eq!(s.counters.heartbeats, 2);
        s.advance();
        s.advance();
        s.advance(); // miss window exceeded
        assert_eq!(s.counters.evictions, 1);
    }

    #[test]
    fn repeated_signature_trips_breaker() {
        let mut d = DoomLoopDetector::new(5);
        let mut hit = None;
        for _ in 0..5 {
            hit = d.record_tool("coder", "read(a.rs)", false, false);
        }
        assert!(hit.is_some());
        assert_eq!(d.counters.loop_breaks, 1);
    }

    #[test]
    fn burn_with_no_commit_trips() {
        let mut d = DoomLoopDetector::new(64);
        let mut hit = None;
        for i in 0..20 {
            hit = d.record_tool("coder", &format!("tool-{i}"), false, false);
        }
        assert!(hit.is_some());
    }

    #[test]
    fn budget_exhaustion_counts() {
        let mut d = DoomLoopDetector::new(64);
        d.set_budget("coder", 3);
        let mut hit = None;
        for i in 0..5 {
            hit = d.record_tool("coder", &format!("sig-{i}"), true, false);
        }
        assert!(hit.is_some());
        assert!(d.counters.budget_exhaustions >= 1);
    }
}
