//! Staleness taxonomy (V8): fresh / stale / lost.
//!
//! Pure functions of `(now_ms, last_ok_ms, seq_gap)` — the frozen clock
//! (`STUDIO_MS1_FROZEN_NOW_MS`) makes them deterministic under
//! `scripts/ms1_gate.sh`. No I/O, no wall clock inside the classifier.

use crate::api::{StalenessDto, StalenessState};

/// Default time-stale budget (ms). Overridable with `STUDIO_STALE_AFTER_MS`.
pub const STALE_AFTER_MS_DEFAULT: u64 = 2000;

/// Classify freshness. `seq_gap` is true when the consumer's cursor predates
/// the retained floor (sequence-lost) — it outranks age: a lost consumer is
/// lost no matter how fresh the last read was.
pub fn classify(db_age_ms: u64, stale_after_ms: u64, seq_gap: bool) -> StalenessState {
    if seq_gap {
        StalenessState::Lost
    } else if db_age_ms > stale_after_ms {
        StalenessState::Stale
    } else {
        StalenessState::Fresh
    }
}

/// Exact badge text the UI renders and `ms1_gate.sh` asserts on.
/// Format: `<state> · source: <source> · updated <age>ms ago · seq <seq>`.
pub fn badge_label(state: StalenessState, source: &str, db_age_ms: u64, change_seq: i64) -> String {
    let word = match state {
        StalenessState::Fresh => "fresh",
        StalenessState::Stale => "stale",
        StalenessState::Lost => "lost",
    };
    format!("{word} · source: {source} · updated {db_age_ms}ms ago · seq {change_seq}")
}

pub fn staleness_of(
    source: &str,
    db_age_ms: u64,
    change_seq: i64,
    stale_after_ms: u64,
    seq_gap: bool,
) -> StalenessDto {
    let state = classify(db_age_ms, stale_after_ms, seq_gap);
    StalenessDto {
        label: badge_label(state, source, db_age_ms, change_seq),
        state,
        db_age_ms,
        change_seq,
    }
}

/// Effective "now" in epoch ms. Frozen under `STUDIO_MS1_FROZEN_NOW_MS` for
/// deterministic gate runs; wall clock otherwise.
pub fn now_ms() -> u64 {
    if let Ok(v) = std::env::var("STUDIO_MS1_FROZEN_NOW_MS") {
        if let Ok(n) = v.trim().parse::<u64>() {
            return n;
        }
    }
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn stale_after_ms() -> u64 {
    std::env::var("STUDIO_STALE_AFTER_MS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(STALE_AFTER_MS_DEFAULT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taxonomy_splits_time_stale_from_sequence_lost() {
        assert_eq!(classify(10, 2000, false), StalenessState::Fresh);
        assert_eq!(classify(2000, 2000, false), StalenessState::Fresh);
        assert_eq!(classify(2001, 2000, false), StalenessState::Stale);
        // Lost outranks age: a gap is lost even with a fresh read.
        assert_eq!(classify(0, 2000, true), StalenessState::Lost);
        assert_eq!(classify(999_999, 2000, true), StalenessState::Lost);
    }

    #[test]
    fn badge_text_is_stable_and_assertable() {
        let s = staleness_of("studio.db", 12, 42, 2000, false);
        assert_eq!(
            s.label,
            "fresh · source: studio.db · updated 12ms ago · seq 42"
        );
        let s = staleness_of("studio.db", 5000, 42, 2000, false);
        assert_eq!(
            s.label,
            "stale · source: studio.db · updated 5000ms ago · seq 42"
        );
        let s = staleness_of("studio.db", 5, 42, 2000, true);
        assert_eq!(
            s.label,
            "lost · source: studio.db · updated 5ms ago · seq 42"
        );
    }
}
