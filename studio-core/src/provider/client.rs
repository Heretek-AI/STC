//! Minimal internal LLM client for daemon-owned calls only (Manager scope
//! negotiation, reviewer judgment). Harness-internal calls stay harness-native.
//! Dialects: `anthropic-messages` + `openai-completions`. `openai-responses`
//! deferred until a caller needs it. Retry + circuit breaker + cost accrual;
//! every breaker transition is counted (munder-difflin HOP_CAP lesson).

use super::{CostTable, ModelId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Dialect {
    AnthropicMessages,
    OpenAICompletions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
}

/// Billable cost in USD. cache_read excluded per roadmap rule.
pub fn cost_of(table: &CostTable, usage: &Usage) -> f64 {
    (usage.input_tokens as f64 * table.input_per_mtok
        + usage.output_tokens as f64 * table.output_per_mtok
        + usage.cache_write_tokens as f64 * table.cache_write_per_mtok)
        / 1_000_000.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Build the wire body for a dialect. Pure function — no I/O, fully testable.
pub fn build_body(
    dialect: Dialect,
    model: &ModelId,
    messages: &[ChatMessage],
    max_tokens: u32,
) -> serde_json::Value {
    match dialect {
        Dialect::AnthropicMessages => serde_json::json!({
            "model": model.0,
            "max_tokens": max_tokens,
            "messages": messages.iter().map(|m| serde_json::json!({"role": m.role, "content": m.content})).collect::<Vec<_>>(),
        }),
        Dialect::OpenAICompletions => serde_json::json!({
            "model": model.0,
            "max_tokens": max_tokens,
            "messages": messages.iter().map(|m| serde_json::json!({"role": m.role, "content": m.content})).collect::<Vec<_>>(),
        }),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakerState {
    Closed,
    Open,
    HalfOpen,
}

/// Circuit breaker with verified counters. Opens after `threshold` consecutive
/// failures; half-opens after `cooldown_ticks`; closes on first success.
pub struct Breaker {
    pub state: BreakerState,
    threshold: u32,
    cooldown_ticks: u64,
    consecutive_failures: u32,
    opened_at_tick: u64,
    pub transitions: u64,
    pub successes: u64,
    pub failures: u64,
}

impl Breaker {
    pub fn new(threshold: u32, cooldown_ticks: u64) -> Self {
        Self {
            state: BreakerState::Closed,
            threshold,
            cooldown_ticks,
            consecutive_failures: 0,
            opened_at_tick: 0,
            transitions: 0,
            successes: 0,
            failures: 0,
        }
    }

    pub fn allows(&mut self, tick: u64) -> bool {
        if self.state == BreakerState::Open && tick - self.opened_at_tick >= self.cooldown_ticks {
            self.state = BreakerState::HalfOpen;
            self.transitions += 1;
        }
        self.state != BreakerState::Open
    }

    pub fn record_success(&mut self) {
        self.successes += 1;
        self.consecutive_failures = 0;
        if self.state != BreakerState::Closed {
            self.state = BreakerState::Closed;
            self.transitions += 1;
        }
    }

    pub fn record_failure(&mut self, tick: u64) {
        self.failures += 1;
        self.consecutive_failures += 1;
        let tripped = self.state == BreakerState::HalfOpen
            || (self.state == BreakerState::Closed && self.consecutive_failures >= self.threshold);
        if tripped {
            self.state = BreakerState::Open;
            self.opened_at_tick = tick;
            self.transitions += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_read_excluded_from_cost() {
        let t = CostTable {
            input_per_mtok: 3.0,
            output_per_mtok: 15.0,
            cache_read_per_mtok: 0.3,
            cache_write_per_mtok: 3.75,
        };
        let u = Usage {
            input_tokens: 1_000_000,
            output_tokens: 0,
            cache_read_tokens: 10_000_000,
            cache_write_tokens: 0,
        };
        assert!((cost_of(&t, &u) - 3.0).abs() < 1e-9);
    }

    #[test]
    fn bodies_carry_model_and_messages() {
        let m = ModelId("m".into());
        let msgs = vec![ChatMessage {
            role: "user".into(),
            content: "hi".into(),
        }];
        for d in [Dialect::AnthropicMessages, Dialect::OpenAICompletions] {
            let b = build_body(d, &m, &msgs, 100);
            assert_eq!(b["model"], "m");
            assert_eq!(b["messages"][0]["content"], "hi");
        }
    }

    #[test]
    fn breaker_opens_halfopens_closes_with_counts() {
        let mut b = Breaker::new(2, 5);
        b.record_failure(0);
        b.record_failure(1);
        assert_eq!(b.state, BreakerState::Open);
        assert!(!b.allows(2));
        assert!(b.allows(6));
        assert_eq!(b.state, BreakerState::HalfOpen);
        b.record_success();
        assert_eq!(b.state, BreakerState::Closed);
        assert_eq!((b.failures, b.successes), (2, 1));
        assert!(b.transitions >= 3);
    }
}
