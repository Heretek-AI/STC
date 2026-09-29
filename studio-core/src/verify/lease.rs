//! Fencing lease tokens gating mutating task operations (phase 02).
//!
//! Clean-room port of the yylo mechanism (MIT) behind citation c007
//! (`fbb900ad6ad6e39a63dbeeffb114f27fc4c2f5c41c0db7d21bdcdc9055d4a1d8`,
//! lease-fence helpers in `task_workspace.py`). No yylo code is copied: the
//! re-derived rule is small — every mutating op (`start`, `sync`,
//! `checkpoint`, `evidence-run`, `child-checkpoint`, `hydrate`) must present
//! the task's CURRENT fencing token; only the token digest is stored, the
//! token itself is shown once at issue; a stale/superseded token is a typed
//! refusal with a stable code, never a silent downgrade. Rotation is
//! monotonic (generation counter only increases).

use std::collections::HashMap;
use thiserror::Error;

use crate::verify::freeze::{now_ms, sha256_hex};

/// Mutating operations that require fence authority. Read-only ops
/// (`status`, `doctor`, `lease-status`) never call the fence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MutatingOp {
    Start,
    Sync,
    Checkpoint,
    EvidenceRun,
    ChildCheckpoint,
    Hydrate,
}

impl MutatingOp {
    pub fn code(&self) -> &'static str {
        match self {
            MutatingOp::Start => "start",
            MutatingOp::Sync => "sync",
            MutatingOp::Checkpoint => "checkpoint",
            MutatingOp::EvidenceRun => "evidence-run",
            MutatingOp::ChildCheckpoint => "child-checkpoint",
            MutatingOp::Hydrate => "hydrate",
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FenceError {
    #[error("task {task} {op} refused ({code}): {message}")]
    Refused {
        task: String,
        op: String,
        code: String,
        message: String,
    },
}

impl FenceError {
    fn refused(task: &str, op: MutatingOp, code: &str, message: String) -> Self {
        Self::Refused {
            task: task.into(),
            op: op.code().into(),
            code: code.into(),
            message,
        }
    }

    pub fn code(&self) -> &str {
        match self {
            Self::Refused { code, .. } => code,
        }
    }
}

#[derive(Debug, Clone)]
struct LeaseRecord {
    generation: u64,
    token_digest: String,
    issued_ms: u64,
}

/// In-process fencing authority. (The durable projection of these records is
/// a phase-04 concern; the tokens themselves are never persisted — only
/// digests — so a DB copy carries no usable authority.)
#[derive(Debug, Default)]
pub struct LeaseFence {
    leases: HashMap<String, LeaseRecord>,
}

impl LeaseFence {
    /// Issue the initial fencing token for a task (fresh admission).
    /// The token is returned ONCE; only its digest is retained.
    pub fn issue(&mut self, task: &str) -> String {
        self.rotate(task)
    }

    /// Rotate after handoff/successor: the old token dies immediately.
    pub fn rotate(&mut self, task: &str) -> String {
        let next_gen = self.leases.get(task).map(|r| r.generation + 1).unwrap_or(0);
        let token = format!(
            "fence-{task}-{next_gen}-{}",
            &sha256_hex(format!("{task}:{next_gen}:{}", now_ms()).as_bytes())[..16]
        );
        self.leases.insert(
            task.into(),
            LeaseRecord {
                generation: next_gen,
                token_digest: sha256_hex(token.as_bytes()),
                issued_ms: now_ms(),
            },
        );
        token
    }

    /// Fail closed unless `token` is the task's CURRENT fencing token.
    pub fn require(
        &self,
        op: MutatingOp,
        task: &str,
        token: Option<&str>,
    ) -> Result<u64, FenceError> {
        let record = self.leases.get(task).ok_or_else(|| {
            FenceError::refused(task, op, "no-lease", "task holds no fencing lease".into())
        })?;
        let presented = token.ok_or_else(|| {
            FenceError::refused(
                task,
                op,
                "missing-token",
                "mutating op requires the fencing token".into(),
            )
        })?;
        if sha256_hex(presented.as_bytes()) != record.token_digest {
            return Err(FenceError::refused(
                task,
                op,
                "stale-token",
                format!("token is not the generation-{} holder", record.generation),
            ));
        }
        Ok(record.generation)
    }

    pub fn generation(&self, task: &str) -> Option<u64> {
        self.leases.get(task).map(|r| r.generation)
    }

    pub fn issued_ms(&self, task: &str) -> Option<u64> {
        self.leases.get(task).map(|r| r.issued_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fence_admits_current_token_and_refuses_everything_else_typed() {
        let mut fence = LeaseFence::default();
        // No lease yet: typed no-lease.
        let err = fence.require(MutatingOp::Start, "t1", None).unwrap_err();
        assert_eq!(err.code(), "no-lease");
        let tok = fence.issue("t1");
        // Missing token: typed missing-token.
        let err = fence.require(MutatingOp::Sync, "t1", None).unwrap_err();
        assert_eq!(err.code(), "missing-token");
        // Wrong token: typed stale-token.
        let err = fence
            .require(MutatingOp::Sync, "t1", Some("bogus"))
            .unwrap_err();
        assert_eq!(err.code(), "stale-token");
        // Current token: admitted, generation visible.
        assert_eq!(
            fence.require(MutatingOp::Checkpoint, "t1", Some(&tok)),
            Ok(0)
        );
        assert_eq!(
            fence.require(MutatingOp::EvidenceRun, "t1", Some(&tok)),
            Ok(0)
        );
    }

    #[test]
    fn rotation_kills_the_old_token_monotonically() {
        let mut fence = LeaseFence::default();
        let t0 = fence.issue("t1");
        let t1 = fence.rotate("t1");
        assert_ne!(t0, t1);
        assert_eq!(fence.generation("t1"), Some(1));
        assert!(fence.require(MutatingOp::Sync, "t1", Some(&t1)).is_ok());
        let err = fence
            .require(MutatingOp::Sync, "t1", Some(&t0))
            .unwrap_err();
        assert_eq!(err.code(), "stale-token");
        assert!(err.to_string().contains("generation-1"));
    }

    #[test]
    fn all_mutating_ops_are_gated() {
        let mut fence = LeaseFence::default();
        let tok = fence.issue("t1");
        for op in [
            MutatingOp::Start,
            MutatingOp::Sync,
            MutatingOp::Checkpoint,
            MutatingOp::EvidenceRun,
            MutatingOp::ChildCheckpoint,
            MutatingOp::Hydrate,
        ] {
            assert!(fence.require(op, "t1", Some(&tok)).is_ok(), "{op:?}");
            assert!(fence.require(op, "t1", Some("stale")).is_err(), "{op:?}");
        }
    }
}
