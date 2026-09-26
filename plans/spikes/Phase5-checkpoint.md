# Phase 5 Checkpoint (implemented 2026-09-26)

H1 PASS -> write-intent fabric activated (not worktree-only).

## Implementation
- `fabric/scope.rs`: `ScopeClaim{file,func,symbol}`, `scope_hash`,
  `extract_scopes` (Rust/Python/Go/TS heuristics; unknown -> file fallback).
- `fabric/broker.rs`: SQLite `claims` broker, `BEGIN IMMEDIATE`, atomic multi-acquire,
  identical-hash collision => reject; disjoint => allow.
- `fabric/isolation.rs`: `Isolation{Shared,Worktree,Container}` (default Shared),
  router requiring held claims for shared; worktree/container bypass.
  Remote lanes: local-only in v1 (no shared checkout across hosts; no E2E relay —
  out of scope per marketplace/cloud boundary).
- `memory/store.rs`: L1 `memory_docs` + FTS5 `memory_fts`, `put/search_fts`,
  real bounded `reap` (oldest-first to caps).
- `memory/markdown.rs`: L2 project-memory md save/load with path-escape refusal.
- `memory/lifecycle.rs`: L3 rows (veracity/supersession/trust, `is_live`).
- `memory/semantic.rs`: L4 trait with mandatory reranker gate (fails closed).
- `memory/retention.rs`: policy validation (refuses unbounded) + real reap delegation.
- `creative/`: source-of-record enum (SVG/HTML/Remotion/Blender), content hash,
  deterministic validate; merge takes winning hash, never text-merges binaries.
- `roles/`: versioned RolePacks + lockfile hash + catalog parity; per-lane profile
  dirs; one-shot capability-token secret broker.

## Evidence (`cargo test` 46/46 + harnesses)
- FABRIC_OK 5 disjoint fn claims, 0 collisions; OVERLAP_OK identical rejected
- ISOLATION_OK shared default, worktree fallback
- FTS roundtrip, oldest-first reap, L2 roundtrip, L4 reranker refusal,
  RolePack phantom catch, one-shot tokens
- `cargo build` tui ok
