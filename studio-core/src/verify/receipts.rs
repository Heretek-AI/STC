//! Versioned execution-envelope / receipt schemas (phase 02).
//!
//! Clean-room port of the yylo mechanism (MIT) behind citations c004
//! (`6ed37d179facd0076445ffd2c6da76175dd7d59e4b442e1fb685e9bf45d6d215`,
//! execution-envelope schema + builder) and c005
//! (`8f05615667955756339af80135345c60358b9965179717a9475056c9f2099d26`,
//! owner authorization -> release receipt). No yylo code is copied: the
//! shapes are re-derived — a versioned envelope built ONLY from provider
//! observations (never assistant prose), and a release receipt that is only
//! producible from a well-formed owner authorization binding this exact
//! candidate + policy identity. Serde-tagged schema versions make old
//! receipts fail closed on parse.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::verify::freeze::sha256_hex;

pub const EXECUTION_ENVELOPE_V1: &str = "stc_execution_envelope.v1";
pub const RELEASE_RECEIPT_V1: &str = "stc_release_gate.v1";
pub const RELEASE_AUTHORIZATION_V1: &str = "stc_release_authorization.v1";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ReceiptError {
    #[error("unknown schema_version {found}, expected {expected}")]
    SchemaMismatch { found: String, expected: String },
    #[error("owner authorization does not bind this local release: {reason}")]
    AuthorizationMismatch { reason: String },
    #[error("envelope inconsistent: {reason}")]
    Inconsistent { reason: String },
}

/// One backend observation (provider/model/session/cost slice). The envelope
/// aggregates these; assistant prose is never an input.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderObservation {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub session_id: Option<String>,
    /// Estimated cost USD, if the provider reported usage.
    pub cost_usd: Option<f64>,
    pub cost_complete: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionStatus {
    Success,
    Failure,
    Timeout,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExecutionEnvelope {
    pub schema_version: String,
    pub status: ExecutionStatus,
    /// Single provider iff ALL observations agree, else None (no guessing).
    pub provider: Option<String>,
    pub model: Option<String>,
    pub session_id: Option<String>,
    pub cost_usd: Option<f64>,
    pub cost_complete: bool,
    pub engine_version: String,
}

/// Build the sole machine execution contract from backend observations.
/// Identity fields collapse to None on any disagreement; cost is complete
/// only if every observation with a cost estimate is complete.
pub fn build_envelope(
    observations: &[ProviderObservation],
    status: ExecutionStatus,
    engine_version: &str,
) -> Result<ExecutionEnvelope, ReceiptError> {
    if observations.is_empty() {
        return Err(ReceiptError::Inconsistent {
            reason: "no provider observations".into(),
        });
    }
    let mut providers: Vec<&str> = vec![];
    let mut models: Vec<&str> = vec![];
    let mut sessions: Vec<&str> = vec![];
    let mut cost_sum = 0.0;
    let mut cost_seen = false;
    let mut cost_complete = true;
    for o in observations {
        if let Some(p) = &o.provider {
            if !providers.contains(&p.as_str()) {
                providers.push(p);
            }
        }
        if let Some(m) = &o.model {
            if !models.contains(&m.as_str()) {
                models.push(m);
            }
        }
        if let Some(s) = &o.session_id {
            if !sessions.contains(&s.as_str()) {
                sessions.push(s);
            }
        }
        if let Some(c) = o.cost_usd {
            cost_seen = true;
            cost_sum += c;
            cost_complete = cost_complete && o.cost_complete;
        }
    }
    Ok(ExecutionEnvelope {
        schema_version: EXECUTION_ENVELOPE_V1.into(),
        status,
        provider: if providers.len() == 1 {
            Some(providers[0].into())
        } else {
            None
        },
        model: if models.len() == 1 {
            Some(models[0].into())
        } else {
            None
        },
        session_id: if sessions.len() == 1 {
            Some(sessions[0].into())
        } else {
            None
        },
        cost_usd: if cost_seen { Some(cost_sum) } else { None },
        cost_complete: cost_complete && cost_seen,
        engine_version: engine_version.into(),
    })
}

/// Owner authorization: the human (or owning policy) binds ONE candidate SHA
/// to ONE policy identity for the `local_release` scope only.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OwnerAuthorization {
    pub schema_version: String,
    pub candidate_sha: String,
    pub policy_identity: String,
    pub owner_id: String,
    pub authorized_scopes: Vec<String>,
}

impl OwnerAuthorization {
    /// Canonical encoding; the digest commits to the exact bytes.
    pub fn canonical(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn digest(&self) -> String {
        sha256_hex(self.canonical().as_bytes())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReleaseReceipt {
    pub schema_version: String,
    pub candidate_sha: String,
    pub policy_identity: String,
    pub authority_id: String,
    pub owner_authorization_sha256: String,
    pub validation: String,
}

/// Produce a release receipt ONLY from an authorization that binds this exact
/// candidate + policy identity, carries the expected schema, names exactly
/// `["local_release"]`, and whose content digest matches. Anything else is a
/// typed refusal — a forged or cross-candidate authorization never mints.
pub fn produce_release_receipt(
    authorization: &OwnerAuthorization,
    authorization_sha256: &str,
    candidate_sha: &str,
    policy_identity: &str,
) -> Result<ReleaseReceipt, ReceiptError> {
    if authorization.digest() != authorization_sha256 {
        return Err(ReceiptError::AuthorizationMismatch {
            reason: "owner authorization digest/content mismatch".into(),
        });
    }
    let bad = |reason: &str| ReceiptError::AuthorizationMismatch {
        reason: reason.into(),
    };
    if authorization.schema_version != RELEASE_AUTHORIZATION_V1 {
        return Err(bad("unexpected authorization schema_version"));
    }
    if authorization.candidate_sha != candidate_sha {
        return Err(bad("authorization binds a different candidate"));
    }
    if authorization.policy_identity != policy_identity {
        return Err(bad("authorization binds a different policy identity"));
    }
    if authorization.owner_id.is_empty() {
        return Err(bad("authorization has no owner_id"));
    }
    if authorization.authorized_scopes != vec!["local_release".to_string()] {
        return Err(bad("authorization scopes are not exactly [local_release]"));
    }
    Ok(ReleaseReceipt {
        schema_version: RELEASE_RECEIPT_V1.into(),
        candidate_sha: candidate_sha.into(),
        policy_identity: policy_identity.into(),
        authority_id: authorization.owner_id.clone(),
        owner_authorization_sha256: authorization_sha256.into(),
        validation: "passed".into(),
    })
}

pub fn check_envelope_schema(value: &serde_json::Value) -> Result<(), ReceiptError> {
    let found = value
        .get("schema_version")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if found != EXECUTION_ENVELOPE_V1 {
        return Err(ReceiptError::SchemaMismatch {
            found,
            expected: EXECUTION_ENVELOPE_V1.into(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(provider: &str, model: &str, session: &str) -> ProviderObservation {
        ProviderObservation {
            provider: Some(provider.into()),
            model: Some(model.into()),
            session_id: Some(session.into()),
            cost_usd: Some(0.5),
            cost_complete: true,
        }
    }

    #[test]
    fn envelope_aggregates_agreeing_observations() {
        let e = build_envelope(
            &[obs("p", "m", "s"), obs("p", "m", "s")],
            ExecutionStatus::Success,
            "v2",
        )
        .unwrap();
        assert_eq!(e.schema_version, EXECUTION_ENVELOPE_V1);
        assert_eq!(e.provider.as_deref(), Some("p"));
        assert_eq!(e.cost_usd, Some(1.0));
        assert!(e.cost_complete);
    }

    #[test]
    fn envelope_collapses_on_disagreement_and_never_reads_prose() {
        let e = build_envelope(
            &[obs("p1", "m", "s"), obs("p2", "m", "s")],
            ExecutionStatus::Failure,
            "v2",
        )
        .unwrap();
        assert_eq!(e.provider, None);
        assert_eq!(e.model.as_deref(), Some("m"));
        assert!(build_envelope(&[], ExecutionStatus::Success, "v2").is_err());
        // Partial cost poisons completeness.
        let mut partial = obs("p", "m", "s");
        partial.cost_complete = false;
        let e = build_envelope(&[partial], ExecutionStatus::Success, "v2").unwrap();
        assert!(!e.cost_complete);
    }

    fn authz() -> OwnerAuthorization {
        OwnerAuthorization {
            schema_version: RELEASE_AUTHORIZATION_V1.into(),
            candidate_sha: "a".repeat(40),
            policy_identity: "b".repeat(64),
            owner_id: "owner-1".into(),
            authorized_scopes: vec!["local_release".into()],
        }
    }

    #[test]
    fn release_receipt_requires_exact_binding() {
        let a = authz();
        let digest = a.digest();
        let r = produce_release_receipt(&a, &digest, &"a".repeat(40), &"b".repeat(64)).unwrap();
        assert_eq!(r.schema_version, RELEASE_RECEIPT_V1);
        assert_eq!(r.validation, "passed");
        // Digest mismatch.
        assert!(
            produce_release_receipt(&a, &"0".repeat(64), &"a".repeat(40), &"b".repeat(64)).is_err()
        );
        // Cross-candidate authorization never mints.
        assert!(matches!(
            produce_release_receipt(&a, &digest, &"c".repeat(40), &"b".repeat(64)).unwrap_err(),
            ReceiptError::AuthorizationMismatch { .. }
        ));
        // Widened scopes never mint.
        let mut wide = a.clone();
        wide.authorized_scopes = vec!["local_release".into(), "remote_release".into()];
        assert!(
            produce_release_receipt(&wide, &wide.digest(), &"a".repeat(40), &"b".repeat(64))
                .is_err()
        );
    }

    #[test]
    fn envelope_schema_gate_rejects_unknown_versions() {
        let v: serde_json::Value =
            serde_json::json!({"schema_version": "stc_execution_envelope.v9"});
        assert!(matches!(
            check_envelope_schema(&v).unwrap_err(),
            ReceiptError::SchemaMismatch { .. }
        ));
        let v: serde_json::Value = serde_json::json!({"schema_version": EXECUTION_ENVELOPE_V1});
        assert!(check_envelope_schema(&v).is_ok());
    }
}
