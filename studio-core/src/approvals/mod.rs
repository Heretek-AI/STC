//! Approval request/status/outcome protocol + durable approval store.
//!
//! Two harvest mechanisms, both clean-room re-derivations (no source copied):
//!
//! - Approval protocol + in-process service behind citations c001
//!   (`c9d6b38dc6efcf6f72c39ce54fa4347cc9c36041305bcc4ba916cb0e7da2b533`),
//!   c002 (`3c32da5997d4940fc557c2bbe600d6598e9508c1abee6c8ccbeae560f1dc7949`),
//!   c003 (`09a8042c814fe3e899831d26381624d51e3941b05b4713472c64d6d2c839be0a`)
//!   (BloopAI/vibe-kanban, Apache-2.0, verdict vendor+clean-room). The
//!   re-derived rule is small — a request carries task + tool and sits
//!   `pending` until exactly one decision (`approved` / `denied` + reason)
//!   lands; a second decision is a typed refusal, never an overwrite.
//!   Vendoring the upstream file is deferred per the annex guardrail (no
//!   network dependency in the build, zero contamination risk); the protocol
//!   here is original code behind the same status machine.
//! - Durable approval store + scoped capability leases + CAS records behind
//!   citation c008 (`f5bb8dc702d9900cedf75b542c4bb1fdb366796cfda63fdf046f9ba06f415392`)
//!   (nearai/ironclaw, Apache-2.0 dual). Re-derived rules: approvals persist
//!   in `contract_approvals`; a decision is a compare-and-swap on
//!   `status='pending'` (a moved row fails closed, never double-decides);
//!   leases persist in `capability_leases` as single-use, scope-bound,
//!   fingerprint-committed records — only the digest is stored, the secret is
//!   shown once at mint; consume is `UPDATE ... WHERE consumed=0` so two
//!   concurrent consumers cannot both succeed (single-writer
//!   `BEGIN IMMEDIATE`; the loser sees 0 rows and is refused typed).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;

use crate::state::StateStore;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

static APPROVAL_COUNTER: AtomicU64 = AtomicU64::new(0);
static LEASE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Typed approval failures. No `String` errors cross this boundary.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ApprovalError {
    #[error("unknown approval request {id}")]
    UnknownRequest { id: String },
    #[error("approval {id} already decided ({status}): decisions are single-shot")]
    AlreadyDecided { id: String, status: String },
    #[error("unknown capability lease {id}")]
    UnknownLease { id: String },
    #[error("capability lease {id} already consumed (single-use)")]
    LeaseConsumed { id: String },
    #[error("capability lease {id} is scoped to [{scope}], not {wanted}")]
    WrongScope {
        id: String,
        scope: String,
        wanted: String,
    },
    #[error("capability lease {id} secret mismatch (fingerprint commit failed)")]
    FingerprintMismatch { id: String },
    #[error("approval {id} is {status}, not approved: no lease mints")]
    NotApproved { id: String, status: String },
    #[error("state: {0}")]
    State(String),
}

/// Request lifecycle: pending until exactly one decision lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Denied,
}

impl ApprovalStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ApprovalStatus::Pending => "pending",
            ApprovalStatus::Approved => "approved",
            ApprovalStatus::Denied => "denied",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "approved" => ApprovalStatus::Approved,
            "denied" => ApprovalStatus::Denied,
            _ => ApprovalStatus::Pending,
        }
    }
}

/// One approval request: which task wants which tool, and the outcome.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApprovalRequest {
    pub id: String,
    pub task_id: String,
    pub tool: String,
    pub status: ApprovalStatus,
    pub reason: Option<String>,
    pub created_ms: u64,
    pub decided_ms: Option<u64>,
}

/// In-process approval service (vk c001-c003 status machine). The durable
/// projection of these records lives in `contract_approvals` (functions
/// below); the service itself holds no locks and owns no DB rows.
#[derive(Debug, Default)]
pub struct ApprovalService {
    requests: HashMap<String, ApprovalRequest>,
}

impl ApprovalService {
    /// Open a request: task × tool enters `pending`.
    pub fn request(&mut self, task_id: &str, tool: &str) -> String {
        let n = APPROVAL_COUNTER.fetch_add(1, Ordering::Relaxed);
        let id = format!(
            "apr-{task_id}-{n}-{}",
            &sha256_hex(format!("{task_id}:{tool}:{n}:{}", now_ms()).as_bytes())[..8]
        );
        self.requests.insert(
            id.clone(),
            ApprovalRequest {
                id: id.clone(),
                task_id: task_id.into(),
                tool: tool.into(),
                status: ApprovalStatus::Pending,
                reason: None,
                created_ms: now_ms(),
                decided_ms: None,
            },
        );
        id
    }

    /// Decide exactly once. A second decision is a typed refusal.
    pub fn decide(
        &mut self,
        id: &str,
        approved: bool,
        reason: &str,
    ) -> Result<ApprovalStatus, ApprovalError> {
        let req = self
            .requests
            .get_mut(id)
            .ok_or_else(|| ApprovalError::UnknownRequest { id: id.into() })?;
        if req.status != ApprovalStatus::Pending {
            return Err(ApprovalError::AlreadyDecided {
                id: id.into(),
                status: req.status.as_str().into(),
            });
        }
        req.status = if approved {
            ApprovalStatus::Approved
        } else {
            ApprovalStatus::Denied
        };
        req.reason = Some(reason.into());
        req.decided_ms = Some(now_ms());
        Ok(req.status)
    }

    pub fn outcome(&self, id: &str) -> Result<ApprovalStatus, ApprovalError> {
        self.requests
            .get(id)
            .map(|r| r.status)
            .ok_or_else(|| ApprovalError::UnknownRequest { id: id.into() })
    }
}

/// Durable projection: persist a request row (idempotent on id).
pub fn save_request(store: &StateStore, req: &ApprovalRequest) -> Result<(), ApprovalError> {
    store
        .with_write(|conn| {
            conn.execute(
                "INSERT OR IGNORE INTO contract_approvals(id,task_id,tool,status,reason,created_ms,decided_ms) VALUES(?,?,?,?,?,?,?)",
                rusqlite::params![
                    req.id,
                    req.task_id,
                    req.tool,
                    req.status.as_str(),
                    req.reason,
                    req.created_ms as i64,
                    req.decided_ms.map(|m| m as i64),
                ],
            )?;
            Ok(())
        })
        .map_err(|e| ApprovalError::State(e.to_string()))
}

pub fn load_request(
    store: &StateStore,
    id: &str,
) -> Result<Option<ApprovalRequest>, ApprovalError> {
    store
        .with_read(|conn| {
            let mut stmt = conn.prepare(
                "SELECT task_id,tool,status,reason,created_ms,decided_ms FROM contract_approvals WHERE id=?",
            )?;
            let mut rows = stmt.query(rusqlite::params![id])?;
            match rows.next()? {
                None => Ok(None),
                Some(r) => {
                    let status_s: String = r.get(2)?;
                    let created: i64 = r.get(4)?;
                    let decided: Option<i64> = r.get(5)?;
                    Ok(Some(ApprovalRequest {
                        id: id.into(),
                        task_id: r.get(0)?,
                        tool: r.get(1)?,
                        status: ApprovalStatus::parse(&status_s),
                        reason: r.get(3)?,
                        created_ms: created as u64,
                        decided_ms: decided.map(|m| m as u64),
                    }))
                }
            }
        })
        .map_err(|e| ApprovalError::State(e.to_string()))
}

/// Durable decision: CAS on `status='pending'`. Zero rows affected means the
/// row moved under us — re-read for the typed reason (unknown vs decided).
pub fn decide_stored(
    store: &StateStore,
    id: &str,
    approved: bool,
    reason: &str,
) -> Result<ApprovalStatus, ApprovalError> {
    let status = if approved { "approved" } else { "denied" };
    let decided = now_ms() as i64;
    let changed = store
        .with_write(|conn| {
            Ok(conn.execute(
                "UPDATE contract_approvals SET status=?, reason=?, decided_ms=? WHERE id=? AND status='pending'",
                rusqlite::params![status, reason, decided, id],
            )?)
        })
        .map_err(|e| ApprovalError::State(e.to_string()))?;
    if changed == 1 {
        return Ok(ApprovalStatus::parse(status));
    }
    match load_request(store, id)? {
        None => Err(ApprovalError::UnknownRequest { id: id.into() }),
        Some(r) => Err(ApprovalError::AlreadyDecided {
            id: id.into(),
            status: r.status.as_str().into(),
        }),
    }
}

/// A scoped capability lease record (digest only — never the secret).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityLease {
    pub id: String,
    pub approval_id: String,
    pub scope: String,
    pub issued_ms: u64,
}

/// Mint a single-use lease for an APPROVED request. Returns the lease id plus
/// the secret — shown ONCE; only its sha256 is stored. Minting for a
/// pending/denied/unknown request is a typed refusal.
pub fn mint_lease(
    store: &StateStore,
    approval_id: &str,
    scope: &str,
) -> Result<(String, String), ApprovalError> {
    let req = load_request(store, approval_id)?.ok_or_else(|| ApprovalError::UnknownRequest {
        id: approval_id.into(),
    })?;
    if req.status != ApprovalStatus::Approved {
        return Err(ApprovalError::NotApproved {
            id: approval_id.into(),
            status: req.status.as_str().into(),
        });
    }
    let n = LEASE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let secret = format!(
        "cap-{approval_id}-{scope}-{n}-{}",
        &sha256_hex(format!("{approval_id}:{scope}:{n}:{}", now_ms()).as_bytes())[..16]
    );
    let fingerprint = sha256_hex(secret.as_bytes());
    let id = format!("lease-{n}-{fingerprint:.8}");
    let issued = now_ms() as i64;
    store
        .with_write(|conn| {
            conn.execute(
                "INSERT INTO capability_leases(id,approval_id,scope,fingerprint,issued_ms,consumed) VALUES(?,?,?,?,?,0)",
                rusqlite::params![id, approval_id, scope, fingerprint, issued],
            )?;
            Ok(())
        })
        .map_err(|e| ApprovalError::State(e.to_string()))?;
    Ok((id, secret))
}

/// Consume a lease for exactly its scope. One-shot: the guarded
/// `UPDATE ... WHERE consumed=0` decides atomically, so two concurrent
/// consumers cannot both succeed. Order is fail-closed: unknown → scope →
/// fingerprint → consumed-CAS, each typed.
pub fn consume_lease(
    store: &StateStore,
    lease_id: &str,
    secret: &str,
    scope: &str,
) -> Result<(), ApprovalError> {
    let row: Option<(String, String)> = store
        .with_read(|conn| {
            conn.query_row(
                "SELECT scope, fingerprint FROM capability_leases WHERE id=?",
                rusqlite::params![lease_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })
            .map_err(crate::state::StateError::from)
        })
        .map_err(|e| ApprovalError::State(e.to_string()))?;
    let (held_scope, fingerprint) = row.ok_or_else(|| ApprovalError::UnknownLease {
        id: lease_id.into(),
    })?;
    if held_scope != scope {
        return Err(ApprovalError::WrongScope {
            id: lease_id.into(),
            scope: held_scope,
            wanted: scope.into(),
        });
    }
    if sha256_hex(secret.as_bytes()) != fingerprint {
        return Err(ApprovalError::FingerprintMismatch {
            id: lease_id.into(),
        });
    }
    let changed = store
        .with_write(|conn| {
            Ok(conn.execute(
                "UPDATE capability_leases SET consumed=1 WHERE id=? AND consumed=0",
                rusqlite::params![lease_id],
            )?)
        })
        .map_err(|e| ApprovalError::State(e.to_string()))?;
    if changed == 0 {
        return Err(ApprovalError::LeaseConsumed {
            id: lease_id.into(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_decides_once_typed() {
        let mut svc = ApprovalService::default();
        let id = svc.request("t1", "write");
        assert_eq!(svc.outcome(&id).unwrap(), ApprovalStatus::Pending);
        assert_eq!(
            svc.decide(&id, true, "scoped write ok").unwrap(),
            ApprovalStatus::Approved
        );
        assert_eq!(svc.outcome(&id).unwrap(), ApprovalStatus::Approved);
        let err = svc.decide(&id, false, "change of mind").unwrap_err();
        assert!(matches!(err, ApprovalError::AlreadyDecided { .. }));
        assert!(err.to_string().contains("approved"));
        assert!(matches!(
            svc.decide("apr-nope", true, "x").unwrap_err(),
            ApprovalError::UnknownRequest { .. }
        ));
    }

    #[test]
    fn denial_is_terminal() {
        let mut svc = ApprovalService::default();
        let id = svc.request("t1", "runProcess");
        assert_eq!(
            svc.decide(&id, false, "unscoped exec").unwrap(),
            ApprovalStatus::Denied
        );
        assert!(matches!(
            svc.decide(&id, true, "reconsider").unwrap_err(),
            ApprovalError::AlreadyDecided { .. }
        ));
    }

    fn approved_row(store: &StateStore, id: &str) {
        let req = ApprovalRequest {
            id: id.into(),
            task_id: "t1".into(),
            tool: "write".into(),
            status: ApprovalStatus::Pending,
            reason: None,
            created_ms: now_ms(),
            decided_ms: None,
        };
        save_request(store, &req).unwrap();
        decide_stored(store, id, true, "ok").unwrap();
    }

    #[test]
    fn durable_decision_is_cas_single_shot() {
        let store = StateStore::open_in_memory().unwrap();
        let req = ApprovalRequest {
            id: "apr-1".into(),
            task_id: "t1".into(),
            tool: "write".into(),
            status: ApprovalStatus::Pending,
            reason: None,
            created_ms: now_ms(),
            decided_ms: None,
        };
        save_request(&store, &req).unwrap();
        let back = load_request(&store, "apr-1").unwrap().unwrap();
        assert_eq!(back.status, ApprovalStatus::Pending);
        assert!(load_request(&store, "apr-nope").unwrap().is_none());
        assert_eq!(
            decide_stored(&store, "apr-1", true, "ok").unwrap(),
            ApprovalStatus::Approved
        );
        // Second decision: typed AlreadyDecided, row keeps first outcome.
        let err = decide_stored(&store, "apr-1", false, "reconsider").unwrap_err();
        assert!(matches!(err, ApprovalError::AlreadyDecided { .. }));
        assert_eq!(
            load_request(&store, "apr-1").unwrap().unwrap().status,
            ApprovalStatus::Approved
        );
        assert!(matches!(
            decide_stored(&store, "apr-nope", true, "x").unwrap_err(),
            ApprovalError::UnknownRequest { .. }
        ));
    }

    #[test]
    fn lease_mints_once_consumes_once_scoped() {
        let store = StateStore::open_in_memory().unwrap();
        approved_row(&store, "apr-1");
        // Pending request mints nothing.
        let pending = ApprovalRequest {
            id: "apr-pending".into(),
            task_id: "t1".into(),
            tool: "write".into(),
            status: ApprovalStatus::Pending,
            reason: None,
            created_ms: now_ms(),
            decided_ms: None,
        };
        save_request(&store, &pending).unwrap();
        assert!(matches!(
            mint_lease(&store, "apr-pending", "write:a.rs").unwrap_err(),
            ApprovalError::NotApproved { .. }
        ));
        assert!(matches!(
            mint_lease(&store, "apr-nope", "write:a.rs").unwrap_err(),
            ApprovalError::UnknownRequest { .. }
        ));
        let (id, secret) = mint_lease(&store, "apr-1", "write:a.rs").unwrap();
        // Wrong scope before consume: typed, lease survives.
        let err = consume_lease(&store, &id, &secret, "write:b.rs").unwrap_err();
        assert!(matches!(err, ApprovalError::WrongScope { .. }));
        // Wrong secret: typed fingerprint refusal.
        assert!(matches!(
            consume_lease(&store, &id, "bogus", "write:a.rs").unwrap_err(),
            ApprovalError::FingerprintMismatch { .. }
        ));
        // Right scope + secret: consumes exactly once.
        assert!(consume_lease(&store, &id, &secret, "write:a.rs").is_ok());
        assert!(matches!(
            consume_lease(&store, &id, &secret, "write:a.rs").unwrap_err(),
            ApprovalError::LeaseConsumed { .. }
        ));
        assert!(matches!(
            consume_lease(&store, "lease-nope", "x", "write:a.rs").unwrap_err(),
            ApprovalError::UnknownLease { .. }
        ));
    }
}
