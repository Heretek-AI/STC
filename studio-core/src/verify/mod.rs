//! Verification gate: deterministic DoD receipts, anti-tap-out, anti-stall.
//! Anchors (by symbol):
//! - opencode-swarm `security/write-authority.ts` (`WriteApprovalFactV1`, one-shot approval,
//!   `computeWriteApprovalHash`, spec-hash staleness), `plan/ledger.ts` gate-evidence,
//!   munder-difflin `Stop` hook `{"decision":"block"}`, Orkas loop guards / progress governor
//! - agent-orchestrator `review.go` head-SHA-fenced feedback (see `merge/`)
//! - agent-of-empires `process/` supervision (lease/epoch; see `scheduler/`)

pub mod anti_stall;
pub mod dod;
pub mod fallow;
pub mod freeze;

pub use anti_stall::{DoomLoopDetector, StallSupervisor};
pub use dod::{evaluate_done, DodCheck, DodReceipt, TapOut};
pub use fallow::{fallow_audit, fallow_audit_with, FallowEvidence};
pub use freeze::{freeze_candidate, AckLedger, CorrectionBudget, FrozenCandidate, RiskTier};
