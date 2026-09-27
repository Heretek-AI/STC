# AGENTS.md — Working Contract for This Repo

> Generated from code sources — do not hand-edit derived sections.
> Sources: `studio-core/src/mcp/registry.rs` (tool catalog), `studio-core/src/roles/`
> (RolePack contract), `studio-core/src/verify/` (gates), `.pre-commit-config.yaml`
> (hooks), `studio-core/src/scheduler/` (dispatch). Regenerate (not rewrite) on drift.

## Build, test, lint

```bash
cargo test                          # 105 tests, must stay green + new tests per change
cargo clippy --all-targets          # 0 warnings (deny-by-default posture)
cargo fmt --check                   # rustfmt clean (rustfmt.toml + .editorconfig)
prek run --all-files                # pre-commit gates, budget <15s
npm run build                       # cockpit frontend (cockpit/)
```

## Conventions

- **Anchors by symbol, never line numbers.** Code moves; symbols don't.
- **Fail closed.** Unknown tools, missing manifests, unresolvable paths, spent tokens → deny with typed reason. Never downgrade silently.
- **Append-only budgets.** Token ledger is append-only; cache hits tracked separately, never reset.
- **UI never owns state.** Every view is a read projection of `studio.db` with staleness badges.
- **LSP is navigational.** Gates run CLI diagnostics (clippy, tree-sitter, fallow audit, Semgrep); never gate on LSP output.
- **Suppressions live in SARIF objects** with justification + CODEOWNERS ownership. No `#[allow]`/`eslint-disable`/`# type: ignore` in source; CI runs with ignores disabled.
- **Secrets are placeholders** (`{{STUDIO_SECRET:label}}`), resolved at lane bind time. Raw `sk-`/`AKIA`/`xox`/`ghp_` material anywhere fails validation.
- **Workers never see raw credentials.** Capability tokens only: one-shot, short-TTL, 0600 files.
- **Never destructive on parallel-coder work.** No `git reset --hard` on shared trees; conflicts retain worktrees + route diagnostics (`CONFLICT_RETAINED`).
- **Never prebake harness CLIs into images.** Versioned `~/.studio/harness/bin/<name>/<version>/` dirs only.
- **Kill-gated spikes.** Every experiment ships pass/fail criteria; misses cut scope, never slip the spine.
- **Stats hygiene.** Re-fetch ecosystem counts at cite time; never copy numbers forward.

## Gate order (DoD)

format → fallow audit (TS/JS, new-findings-only) → Semgrep Guardian → tests + tree-sitter → RDD freeze → one bounded correction → burned ack → Bors merge. Tap-outs ("done", no evidence) get `blocked` receipts; repeat offenders escalate to the manager.

## Live-fire testing (real credentials available)

- `.env` (gitignored, never commit) holds `OPENCODE_API_KEY` — a real key. This unblocks live-model measurements (H-A/H-B/H-C full variants), Providers live-ping proof, and lane smoke tests against real models.
- Rules: never print or log key material (redact in all output); prefer the cheapest adequate model for smoke tests; record spend per run; `--locked`/deterministic gates still apply to live-fire results.
- Kill-gated live measurements that were `blocked:credentials` are now runnable — see issues #1, #2, #3, #4.

## Derived: tool catalog (15 tools)

`read, write, edit, patch, runProcess, code_search, web_search, tool_open, kv_get, syntax_check, sast, sbom, plan_open, dag_commit, retrieve_docs` — role manifests slice per role (`Allow/OnDemand/Deny`); `tool_open` loads full schemas on demand (~20 tok/tool index).

## Derived: role contract

`RolePack{name, version, tools, skill_lockfile_hash, system_prompt, mcp_manifest, model_slot, harness_profile, spawns, output_schema}` — parity + manifest + secret checks enforced; `rolepack.yaml/.lock` pinned, `--locked` verified; spawns never escalate (typed rejection).
