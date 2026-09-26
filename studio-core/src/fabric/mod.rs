//! Write-intent fabric moat (Phase 5, gated on H1 PASS).
//! Semantic scope claims (file/fn/symbol) become default isolation; deterministic
//! overlap rules reject/queue colliding intents; worktree is the fallback for
//! scopes declared `isolation: worktree|container`.
//! Extractor: heuristic fn/symbol detection for Rust/Python/Go/TS;
//! unknown grammars fall back to file-granularity (never fail the write).

pub mod broker;
pub mod isolation;
pub mod scope;

pub use broker::{ClaimBroker, ClaimError};
pub use isolation::{Isolation, IsolationRouter};
pub use scope::{extract_scopes, scope_hash, ScopeClaim};
