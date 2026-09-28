# 01-engine-skeleton — Engine skeleton: daemon + SQLite v2 state + CLI projection read

**Program:** STC v2 greenfield rebuild · run `stc-greenfield` · branch `v2-greenfield`
**Frontier:** `file://.research/frontier.json` sha256 `86b6c81b505caccdffed71d0ca5139047cf4bd693e838791ab306025a52a9af7`
**DoD:** command-first; browser verification only at MS-1 (phase 04) and MS-2 (phase 06).

## Goal
Stand up the minimal durable core of STC v2: a Tokio daemon whose only source of truth is a greenfield SQLite v2 database (WAL, single-writer discipline), plus studio-cli read verbs that are pure projections of that DB. Crash-anytime-safe: SIGKILL mid-write leaves the DB consistent and recovery idempotent. No UI.

## Acceptance criteria
- [ ] v2-greenfield branch + v1 archived to .archive/v1
- [ ] cargo build/test exit 0
- [ ] studio status exits 0 reading real DB (no mock)
- [ ] 50x kill -9 leaves 0 corrupt rows + 0 orphaned worktrees
- [ ] ported modules cite v1 sha256 + pass tests/clippy in v2
- [ ] gate order runs clean

## Brief (to programmer)
Create branch v2-greenfield off main; archive v1 (.factory/.roadmap/.research/.fallow) to .archive/v1 then clear. Scaffold workspace studio-core (state, ledger, projection) + studio-cli. Greenfield v2 schema, no v1 migration. Port {state,ledger,projection,worktree} ONLY if tests pass + clippy clean in the v2 tree; else rewrite. Enforce single-writer discipline (VERIFIED: one write txn at a time) and CHECKPOINT the WAL to avoid unbounded growth (VERIFIED: WAL grows without bound under continuous readers). Be careful with POSIX advisory locks across fd close (VERIFIED). studio status must work WITHOUT a live daemon.

## Evidence discipline
Every claim in `dossier.json` carries a hash. VERIFIED = quote re-checked in the content-addressed cache.
FILE_HASH_ONLY = real file + real sha256; claim about the file not independently verified.
UNVERIFIED_HYPOTHESIS = direction only; not settled design.

## Swarm status (honest)
Gate cycle 1/5. Both swarms FAILED (`alpha_dossier.json` missing); beta produced one P1 dossier; alpha+audit never ran.
Manager archived the 7 beta-cited sources and re-verified 6 quotes via `iumbtems_verify_quote` (0.98).
