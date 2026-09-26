//! Verification gate: deterministic DoD receipts, anti-tap-out, anti-stall.
//! Anchors (by symbol):
//! - opencode-swarm `security/write-authority.ts` (`WriteApprovalFactV1`, one-shot approval,
//!   `computeWriteApprovalHash`, spec-hash staleness), `plan/ledger.ts` gate-evidence,
//!   munder-difflin `Stop` hook `{"decision":"block"}`, Orkas loop guards / progress governor
//! - agent-orchestrator `review.go` head-SHA-fenced feedback (see `merge/`)
//! - agent-of-empires `process/` supervision (lease/epoch; see `scheduler/`)

pub mod anti_stall;
pub mod dod;

pub use anti_stall::{DoomLoopDetector, StallSupervisor};
pub use dod::{DodCheck, DodReceipt, TapOut, evaluate_done};
