//! STC v2 core: durable engine skeleton.
//!
//! The only source of truth is the greenfield SQLite v2 database (WAL,
//! single-writer discipline). `projection` is a pure read projection of that
//! DB; nothing here owns canonical state in memory.
//!
//! Ports (see `PHASE-RECEIPT.md` in phase 01): `projection` and `worktree` are
//! carried from v1 under the port rule (tests + clippy clean in the v2 tree).
//! `state` is a greenfield v2 rewrite — v1's schema migration stack is
//! deliberately not carried (greenfield v2 schema, no v1 migration).
//!
//! The v1 `ledger` module (a filesystem `.swarm/plan-ledger.jsonl` hash chain)
//! is **not** carried: it was dead code and its file-based ledger contradicts
//! "SQLite is the only source of truth". It returns in phase 02 (contract
//! runtime) if a plan ledger is needed, backed by the DB.

pub mod acp;
pub mod approvals;
pub mod lock;
pub mod mcp;
pub mod policy;
pub mod projection;
pub mod recovery;
pub mod roles;
pub mod state;
pub mod verify;
pub mod worktree;
