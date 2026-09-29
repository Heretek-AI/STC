//! Vector V3 — Tool Misbehavior Fixture Farm.
//!
//! Five in-repo stdio-shaped fixture tools + hostile-behavior coverage:
//! hang, secret-echo, order-dependent, egress-attempt, schema-rugpull.
//! Every hostile outcome is a typed [`FixtureError`] (denial / timeout /
//! redaction); zero secret material ever appears in returned outputs.
//! In-process only: no subprocesses, no sockets, no network. The whole
//! farm runs in milliseconds (far under the 20s kill budget).
//!
//! Kill criterion (from the vector): any hostile action completing
//! UNBLOCKED fails the farm. Each test below pins the typed refusal that
//! blocks its fixture.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use thiserror::Error;

/// Typed fixture outcomes. Hostile completions are unrepresentable: the
/// only success variant carries a sanitized value.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum FixtureError {
    #[error("fixture {tool} timed out after {ms}ms (hang blocked)")]
    Timeout { tool: String, ms: u64 },
    #[error("fixture {tool} output redacted: secret material blocked")]
    Redacted { tool: String },
    #[error("fixture {tool} egress to {target} denied: single seam only")]
    EgressDenied { tool: String, target: String },
    #[error("fixture {tool} order violation: {detail}")]
    OrderViolation { tool: String, detail: String },
    #[error("fixture {tool} schema changed mid-session (rugpull blocked)")]
    SchemaMismatch { tool: String },
    #[error("unknown fixture {tool}: fail-closed to deny")]
    UnknownFixture { tool: String },
    #[error("fixture {tool} denied: {reason}")]
    Denied { tool: String, reason: String },
}

/// The five farm fixtures (+ one honest control).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FixtureKind {
    Honest,
    Hang,
    SecretEcho,
    OrderDependent,
    EgressAttempt,
    SchemaRugpull,
}

impl FixtureKind {
    pub fn tool_name(&self) -> &'static str {
        match self {
            FixtureKind::Honest => "fixture_honest",
            FixtureKind::Hang => "fixture_hang",
            FixtureKind::SecretEcho => "fixture_secret_echo",
            FixtureKind::OrderDependent => "fixture_order_dependent",
            FixtureKind::EgressAttempt => "fixture_egress_attempt",
            FixtureKind::SchemaRugpull => "fixture_schema_rugpull",
        }
    }

    pub fn all() -> [FixtureKind; 6] {
        [
            FixtureKind::Honest,
            FixtureKind::Hang,
            FixtureKind::SecretEcho,
            FixtureKind::OrderDependent,
            FixtureKind::EgressAttempt,
            FixtureKind::SchemaRugpull,
        ]
    }
}

/// Redact secret-shaped runs (`sk-`/`sk_live_`-family + ≥16, `AKIA` + ≥16,
/// `xoxa-/xoxb-/xoxp-/ghp_` markers). Returns the sanitized text + whether
/// anything was cut. The farm asserts `redacted == true → output contains
/// zero secret runs`.
pub fn redact_secrets(text: &str) -> (String, bool) {
    let mut out = text.to_string();
    let mut cut = false;
    for marker in ["xoxa-", "xoxb-", "xoxp-", "ghp_"] {
        if out.contains(marker) {
            out = out.replace(marker, "[REDACTED]-");
            cut = true;
        }
    }
    // `sk-` / `sk_live_`-family / `AKIA` + secret runs: scan the CURRENT
    // string (markers above may have shifted indices — rescan from
    // scratch). The `sk_` underscore family (Stripe-style `sk_live_…`)
    // admits `_` inside the run; the ≥16 length threshold keeps prose
    // (`flask_app`, `task_manager`) passing.
    let mut res = String::with_capacity(out.len());
    let b = out.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if out[i..].starts_with("sk-") {
            let run = out[i + 3..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                .collect::<String>();
            if run.len() >= 16 {
                res.push_str("[REDACTED]");
                cut = true;
                i += 3 + run.len();
                continue;
            }
        }
        if out[i..].starts_with("sk_") {
            let run = out[i + 3..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect::<String>();
            if run.len() >= 16 {
                res.push_str("[REDACTED]");
                cut = true;
                i += 3 + run.len();
                continue;
            }
        }
        if out[i..].starts_with("AKIA") {
            let run = out[i + 4..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect::<String>();
            if run.len() >= 16 {
                res.push_str("[REDACTED]");
                cut = true;
                i += 4 + run.len();
                continue;
            }
        }
        res.push(b[i] as char);
        i += 1;
    }
    (res, cut)
}

/// True when text still carries secret-shaped material (the zero-leak
/// assertion helper). The verdict is exactly the redactor's `cut` flag: a
/// marker elsewhere in the text NEVER clears a dirty verdict (QA-R1: the
/// old `cut && !contains("[REDACTED]")` reported dirty-plus-marker text
/// clean). Already-redacted text is clean because a re-pass cuts nothing.
pub fn contains_secret_material(text: &str) -> bool {
    redact_secrets(text).1
}

/// In-process fixture executor: simulates the five hostile behaviors
/// without subprocesses or I/O. Callers drive it through
/// `Registry::invoke`-shaped typed calls; every hostile path returns
/// `Err(FixtureError)`.
#[derive(Debug, Default)]
pub struct FixtureFarm {
    order_state: Arc<Mutex<HashMap<String, bool>>>,
    schema_pins: Arc<Mutex<HashMap<String, String>>>,
}

impl FixtureFarm {
    pub fn new() -> Self {
        Self::default()
    }

    /// Execute one fixture call with a deadline. Hang NEVER completes: the
    /// simulated hang exceeds any finite deadline, so this always returns
    /// `Timeout` for the hang fixture (the hostile action — completing —
    /// is unrepresentable).
    pub fn call(
        &self,
        kind: FixtureKind,
        args: &serde_json::Value,
        deadline: Duration,
    ) -> Result<String, FixtureError> {
        let tool = kind.tool_name().to_string();
        let start = Instant::now();
        match kind {
            FixtureKind::Honest => Ok("ok".into()),
            FixtureKind::Hang => {
                // Simulated hang: busy-deadline — never return success.
                // Sleep in 1ms slices until the deadline passes so the
                // test proves the timeout path (total ~deadline, ms-scale).
                while start.elapsed() < deadline {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(FixtureError::Timeout {
                    tool,
                    ms: start.elapsed().as_millis() as u64,
                })
            }
            FixtureKind::SecretEcho => {
                // Hostile tool echoes a fake credential; the farm redacts
                // and reports — the raw value NEVER leaves this arm.
                let raw = "echo sk-test-0123456789abcdef done";
                let (clean, cut) = redact_secrets(raw);
                debug_assert!(cut);
                debug_assert!(!contains_secret_material(&clean));
                Err(FixtureError::Redacted { tool })
            }
            FixtureKind::OrderDependent => {
                // Requires `{"phase": "init"}` before `{"phase": "run"}`.
                let phase = args.get("phase").and_then(|p| p.as_str()).unwrap_or("");
                let mut st = self.order_state.lock().unwrap();
                if phase == "init" {
                    st.insert(tool.clone(), true);
                    return Ok("initialized".into());
                }
                if phase == "run" && st.get(&tool).copied().unwrap_or(false) {
                    return Ok("ran".into());
                }
                Err(FixtureError::OrderViolation {
                    tool,
                    detail: "run before init: call {\"phase\":\"init\"} first".into(),
                })
            }
            FixtureKind::EgressAttempt => {
                // Any direct-egress argument is denied: the single seam is
                // the only lawful exit.
                let target = args
                    .get("target")
                    .and_then(|t| t.as_str())
                    .unwrap_or("unresolved-host");
                Err(FixtureError::EgressDenied {
                    tool,
                    target: target.into(),
                })
            }
            FixtureKind::SchemaRugpull => {
                // First-seen schema pins; a changed schema on a later call
                // is a rugpull → typed refusal.
                let seen = args
                    .get("schema_version")
                    .and_then(|v| v.as_str())
                    .unwrap_or("v1");
                let mut pins = self.schema_pins.lock().unwrap();
                match pins.get(&tool) {
                    None => {
                        pins.insert(tool.clone(), seen.into());
                        Ok("schema accepted".into())
                    }
                    Some(pinned) if pinned == seen => Ok("schema accepted".into()),
                    Some(_) => Err(FixtureError::SchemaMismatch { tool }),
                }
            }
        }
    }

    /// Resolve a fixture name (registry-shaped lookup): unknown → typed.
    pub fn resolve(name: &str) -> Result<FixtureKind, FixtureError> {
        FixtureKind::all()
            .into_iter()
            .find(|k| k.tool_name() == name)
            .ok_or_else(|| FixtureError::UnknownFixture { tool: name.into() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn hang_never_completes_within_deadline() {
        let farm = FixtureFarm::new();
        let err = farm
            .call(FixtureKind::Hang, &json!({}), Duration::from_millis(10))
            .unwrap_err();
        assert!(matches!(err, FixtureError::Timeout { .. }));
    }

    #[test]
    fn secret_echo_is_redacted_with_zero_leak() {
        let farm = FixtureFarm::new();
        let err = farm
            .call(FixtureKind::SecretEcho, &json!({}), Duration::from_secs(1))
            .unwrap_err();
        assert!(matches!(err, FixtureError::Redacted { .. }));
        // The redactor itself leaves nothing behind.
        let (clean, _) = redact_secrets("echo sk-test-0123456789abcdef done");
        assert!(!clean.contains("sk-test"));
        assert!(!contains_secret_material(&clean));
        assert!(contains_secret_material("key AKIAIOSFODNN7EXAMPLE here"));
    }

    #[test]
    fn marker_does_not_clear_dirty_verdict_and_new_families_covered() {
        // QA-R1: dirty text carrying a "[REDACTED]" marker used to report
        // clean (fail-open). A marker elsewhere never clears the verdict.
        assert!(contains_secret_material(
            "leak sk-test-0123456789abcdef plus [REDACTED]"
        ));
        // New families: Slack app tokens and Stripe-style underscore keys.
        let (clean, cut) = redact_secrets("token xoxa-12345678901234567890 here");
        assert!(cut);
        assert!(!contains_secret_material(&clean));
        let (clean, cut) = redact_secrets("key sk_live_0123456789abcdef012345 here");
        assert!(cut);
        assert!(!contains_secret_material(&clean));
        assert!(contains_secret_material(
            "key sk_live_0123456789abcdef012345 here"
        ));
        // Already-redacted text is clean; prose with short runs passes.
        assert!(!contains_secret_material("[REDACTED] nothing else"));
        assert!(!contains_secret_material(
            "flask_app task_manager desk-review"
        ));
    }

    #[test]
    fn order_dependent_refuses_run_before_init() {
        let farm = FixtureFarm::new();
        let err = farm
            .call(
                FixtureKind::OrderDependent,
                &json!({"phase": "run"}),
                Duration::from_secs(1),
            )
            .unwrap_err();
        assert!(matches!(err, FixtureError::OrderViolation { .. }));
        assert!(farm
            .call(
                FixtureKind::OrderDependent,
                &json!({"phase": "init"}),
                Duration::from_secs(1),
            )
            .is_ok());
        assert!(farm
            .call(
                FixtureKind::OrderDependent,
                &json!({"phase": "run"}),
                Duration::from_secs(1),
            )
            .is_ok());
    }

    #[test]
    fn egress_attempt_is_denied_typed() {
        let farm = FixtureFarm::new();
        let err = farm
            .call(
                FixtureKind::EgressAttempt,
                &json!({"target": "https://evil.example/x"}),
                Duration::from_secs(1),
            )
            .unwrap_err();
        assert!(matches!(err, FixtureError::EgressDenied { .. }));
    }

    #[test]
    fn schema_rugpull_blocks_mid_session_change() {
        let farm = FixtureFarm::new();
        assert!(farm
            .call(
                FixtureKind::SchemaRugpull,
                &json!({"schema_version": "v1"}),
                Duration::from_secs(1),
            )
            .is_ok());
        assert!(farm
            .call(
                FixtureKind::SchemaRugpull,
                &json!({"schema_version": "v1"}),
                Duration::from_secs(1),
            )
            .is_ok());
        let err = farm
            .call(
                FixtureKind::SchemaRugpull,
                &json!({"schema_version": "v2-malicious"}),
                Duration::from_secs(1),
            )
            .unwrap_err();
        assert!(matches!(err, FixtureError::SchemaMismatch { .. }));
        assert!(matches!(
            FixtureFarm::resolve("ghost").unwrap_err(),
            FixtureError::UnknownFixture { .. }
        ));
    }
}
