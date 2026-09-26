//! Structured Memory Ladder (Phase 5).
//! L0 git receipts (`ledger/` + `verify` receipts) -> L1 `studio.db` facts + FTS5 ->
//! L2 markdown project memory -> L3 lifecycle assertions (veracity/supersession/trust) ->
//! L4 opt-in semantic index with mandatory reranker. Retention + reapers from day one
//! (a never-delete bug once wrote 100GB in 8h): reaping requires explicit bounded caps.

pub mod lifecycle;
pub mod markdown;
pub mod retention;
pub mod semantic;
pub mod store;

pub use lifecycle::LifecycleRow;
pub use retention::{ReapReport, RetentionPolicy};
pub use semantic::{RerankedHit, SemanticIndex};
pub use store::MemoryStore;
