//! Deterministic monotonic runtime-policy resolver (phase 02).
//!
//! Clean-room port of the ironclaw mechanism (Apache-2.0 dual) behind citation
//! c009 (`4cc1a18df5f7724ddedaaf0c5c9e73508ae9c05ab39e78e70085fa4a171e1416`).
//! No ironclaw code is copied: the re-derived rules are — privilege resolves
//! monotonically DOWN only (a request above the current grant is a typed
//! refusal, never an upgrade); the least-privilege `Locked` level is the
//! fail-closed default (unknown level names parse to an error, never to a
//! grant); entering the permissive `Yolo` level requires an explicit,
//! recorded acknowledgement string (empty/missing ack fails closed); and every
//! decision round-trips through JSON losslessly (audit round-trip: what was
//! decided is what re-parses).

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Privilege levels, ordered least → most. Resolution only ever moves DOWN
/// this order (reduce-only); anything else is a typed refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum PolicyLevel {
    Locked,
    Standard,
    Permissive,
    /// Explicit human-acknowledged bypass. Never resolved implicitly: it must
    /// be entered through [`acknowledge_yolo`] with a recorded ack string.
    Yolo,
}

impl PolicyLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            PolicyLevel::Locked => "locked",
            PolicyLevel::Standard => "standard",
            PolicyLevel::Permissive => "permissive",
            PolicyLevel::Yolo => "yolo",
        }
    }

    /// Fail-closed parse: unknown names are errors, never grants.
    pub fn parse(s: &str) -> Result<Self, PolicyError> {
        match s {
            "locked" => Ok(PolicyLevel::Locked),
            "standard" => Ok(PolicyLevel::Standard),
            "permissive" => Ok(PolicyLevel::Permissive),
            "yolo" => Ok(PolicyLevel::Yolo),
            other => Err(PolicyError::UnknownLevel {
                found: other.into(),
            }),
        }
    }
}

/// Typed policy failures. No `String` errors cross this boundary.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PolicyError {
    #[error("policy escalation refused: requested {requested} above grant {grant} (reduce-only)")]
    Escalation { requested: String, grant: String },
    #[error("unknown policy level {found}: fail-closed to locked")]
    UnknownLevel { found: String },
    #[error("yolo entry refused: explicit non-empty acknowledgement required")]
    MissingYoloAck,
    #[error("audit round-trip mismatch: {detail}")]
    AuditMismatch { detail: String },
}

/// The active runtime policy: current grant + optional recorded yolo ack.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimePolicy {
    pub level: PolicyLevel,
    pub yolo_ack: Option<String>,
}

impl RuntimePolicy {
    /// Fail-closed default: least privilege, no ack.
    pub fn locked() -> Self {
        Self {
            level: PolicyLevel::Locked,
            yolo_ack: None,
        }
    }

    /// Resolve a request against the current grant. Reduce-only: the resolved
    /// level is at most the current grant; any request above it is a typed
    /// refusal. `Yolo` can never be reached through resolution — only through
    /// [`acknowledge_yolo`].
    pub fn resolve(&self, requested: PolicyLevel) -> Result<PolicyLevel, PolicyError> {
        if requested == PolicyLevel::Yolo {
            return Err(PolicyError::MissingYoloAck);
        }
        if requested > self.level {
            return Err(PolicyError::Escalation {
                requested: requested.as_str().into(),
                grant: self.level.as_str().into(),
            });
        }
        Ok(requested)
    }

    /// Enter `Yolo` with an explicit, recorded acknowledgement. The ack string
    /// must be non-empty (the human names what they accept); it is stored on
    /// the policy so the audit trail shows WHO acknowledged WHAT.
    pub fn acknowledge_yolo(&mut self, ack: &str) -> Result<(), PolicyError> {
        if ack.trim().is_empty() {
            return Err(PolicyError::MissingYoloAck);
        }
        self.level = PolicyLevel::Yolo;
        self.yolo_ack = Some(ack.into());
        Ok(())
    }

    /// Step down to a less-privileged level (always allowed; monotonic).
    /// Stepping UP goes through [`RuntimePolicy::resolve`] and fails closed.
    pub fn reduce(&mut self, level: PolicyLevel) {
        if level < self.level {
            self.level = level;
            if level != PolicyLevel::Yolo {
                self.yolo_ack = None;
            }
        }
    }

    /// Audit round-trip: serialize and re-parse; the decision must survive
    /// losslessly or the audit record is corrupt (typed, never assumed).
    pub fn audit_round_trip(&self) -> Result<RuntimePolicy, PolicyError> {
        let body = serde_json::to_string(self).map_err(|e| PolicyError::AuditMismatch {
            detail: e.to_string(),
        })?;
        let back: RuntimePolicy =
            serde_json::from_str(&body).map_err(|e| PolicyError::AuditMismatch {
                detail: e.to_string(),
            })?;
        if back != *self {
            return Err(PolicyError::AuditMismatch {
                detail: "re-parsed policy differs".into(),
            });
        }
        Ok(back)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolution_is_reduce_only_typed() {
        let pol = RuntimePolicy {
            level: PolicyLevel::Standard,
            yolo_ack: None,
        };
        // Same or lower resolves.
        assert_eq!(
            pol.resolve(PolicyLevel::Standard).unwrap(),
            PolicyLevel::Standard
        );
        assert_eq!(
            pol.resolve(PolicyLevel::Locked).unwrap(),
            PolicyLevel::Locked
        );
        // Higher is a typed escalation refusal, not an upgrade.
        let err = pol.resolve(PolicyLevel::Permissive).unwrap_err();
        assert!(matches!(err, PolicyError::Escalation { .. }));
        assert!(err.to_string().contains("permissive"));
        // Yolo never resolves implicitly, even from Yolo.
        assert!(matches!(
            pol.resolve(PolicyLevel::Yolo).unwrap_err(),
            PolicyError::MissingYoloAck
        ));
    }

    #[test]
    fn yolo_requires_explicit_ack() {
        let mut pol = RuntimePolicy::locked();
        assert_eq!(
            pol.acknowledge_yolo("").unwrap_err(),
            PolicyError::MissingYoloAck
        );
        assert_eq!(
            pol.acknowledge_yolo("   ").unwrap_err(),
            PolicyError::MissingYoloAck
        );
        pol.acknowledge_yolo("owner accepts unscoped local release")
            .unwrap();
        assert_eq!(pol.level, PolicyLevel::Yolo);
        assert!(pol.yolo_ack.is_some());
    }

    #[test]
    fn unknown_levels_fail_closed() {
        assert_eq!(
            PolicyLevel::parse("turbo").unwrap_err(),
            PolicyError::UnknownLevel {
                found: "turbo".into()
            }
        );
        // Case is significant: no guessing.
        assert!(PolicyLevel::parse("Locked").is_err());
        assert_eq!(PolicyLevel::parse("locked").unwrap(), PolicyLevel::Locked);
    }

    #[test]
    fn reduce_is_monotonic_and_clears_ack() {
        let mut pol = RuntimePolicy::locked();
        pol.acknowledge_yolo("ack").unwrap();
        pol.reduce(PolicyLevel::Standard);
        assert_eq!(pol.level, PolicyLevel::Standard);
        assert_eq!(pol.yolo_ack, None);
        // Reducing UP is a no-op (use resolve for upgrades: denied).
        pol.reduce(PolicyLevel::Permissive);
        assert_eq!(pol.level, PolicyLevel::Standard);
    }

    #[test]
    fn audit_round_trip_is_lossless() {
        let mut pol = RuntimePolicy::locked();
        pol.acknowledge_yolo("owner-1:local_release").unwrap();
        let back = pol.audit_round_trip().unwrap();
        assert_eq!(back, pol);
        assert_eq!(back.yolo_ack.as_deref(), Some("owner-1:local_release"));
        assert!(RuntimePolicy::locked().audit_round_trip().is_ok());
    }
}
