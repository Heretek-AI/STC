# PHASE-RECEIPT — 05-cockpit-views (operator views + A1 decide wiring)

**Program:** STC v2 greenfield rebuild · run `harvest-review-01` · `main` working tree (predecessors 02 signed off `2ff256b`, 03 signed off `6b2cb8d`, 04 signed off + MS-1 passed `83ce455`; `v2-greenfield` merge is the user's)
**Phase brief:** `.roadmap/05-cockpit-views/GOAL.md` · parent dossier `.roadmap/05-cockpit-views/dossier.json` (verdict: pending — NOT modified, see guard note) · annex `.roadmap/05-cockpit-views/ANNEX-harvest-review-01.md` (sha256 `c47733731904c008f5953984d3a8e9f0e720e44fd93f5c6fc12794f8c236a1d9`) · gate dossier `.roadmap/harvest-review-01/dossier.json` (`7cac8a00e6cdb0dba39e230e3f47d1bb91558eebc835f5d4cbe3f0ba4d100e5e`)
**Goal:** five operator views (War Room, Agent Stream, Audit+Gatekeeper, MCP Registry, Providers) + Runs compare slots over `/api`, all read projections with staleness badges, plus the A1 one-tap decide wiring through the P02 approval service.

> Every claim below carries the command run and its exit code. Open questions and deferrals are under **Residual limitations**, not overclaimed. No swarms or other programmers invoked. No `iumbtems_verify_quote` call was needed (no new web-source claims; all evidence is hash-cited in-tree material).

## Phase evidence hashes cited (from parent dossier.json — §-corrections apply)

| Claim | sha256 | Pointer | Check this run |
|---|---|---|---|
| frontier (settled decisions) | `86b6c81b505caccdffed71d0ca5139047cf4bd693e838791ab306025a52a9af7` | `file://.research/frontier.json` | see §E5.2 correction note (file absent in-tree; live frontier `.factory/frontier.json`) |
| v1 projection (read candidate) | `3600ef7d48ac6c1c325b49d3f976c1995b4fa75b62fdcf48aef5465c7c633c4f` | `file://studio-core/src/projection/mod.rs` | see §E5.3 revision (reproducible revision form, no caret syntax) |
| beta dossier (alpha_completed=false → HYPOTHESIS) | `082c5c6e1bc543996d2372206e7ca8123ba02e2f60e5108f53e01c19832bb768` | `file://.research/scratchpads/scope_01_engine_state_isolation/beta_dossier.json` | cited as HYPOTHESIS only; no design settled from it. NOTE (QA-R1): the spawn brief carries a 63-hex malformed variant ending `…f53fbbf6fa206dc1`; the parent `dossier.json` `hashes.beta_dossier` 64-hex above is authoritative and cited |
| gate dossier | `7cac8a00e6cdb0dba39e230e3f47d1bb91558eebc835f5d4cbe3f0ba4d100e5e` | `file://.roadmap/harvest-review-01/dossier.json` | read-only; 8 P05 adoption rows + A1 + V7 + V8 extracted |
| phase annex | `c47733731904c008f5953984d3a8e9f0e720e44fd93f5c6fc12794f8c236a1d9` | `file://.roadmap/05-cockpit-views/ANNEX-harvest-review-01.md` | `sha256sum` verified this run; implemented exactly |

### E5.2 — Frontier correction (P02–P04 lessons applied)

The parent dossier asserts `86b6c81b…` @ `file://.research/frontier.json` (VERIFIED_HASH — the dossier author's claim, guarded, NOT altered). That file is ABSENT in this tree (`ls .research/frontier.json` → No such file); the live frontier in this tree is `.factory/frontier.json`. I cite the dossier claim + this footnote and do not assert a `sha256sum` match against a file I cannot see.

### E5.3 — v1 projection citation revision (reproducible form, no caret syntax)

The v1 `studio-core/src/projection/mod.rs` (`3600ef7d…`, FILE_HASH_ONLY) lives in pre-greenfield history, not in the v2 tree. Reproducible command without caret syntax: `git log --all --format='%H' -- studio-core/src/projection/mod.rs` lists the containing revision(s); `git show <full-hash>:studio-core/src/projection/mod.rs | sha256sum` reproduces `3600ef7d48ac6c1c325b49d3f976c1995b4fa75b62fdcf48aef5465c7c633c4f`. The v2 projection consumed by `/api` is the greenfield `studio-core/src/projection/mod.rs` + `state::snapshot` already in-tree; this phase adds read-paths over it (two additive readers + one additive CDC trigger, no v1 code copied, `schema_version` stays 2).

## Starting state (repro)

`git log --oneline -1` → `83ce455 phase(04-cockpit-shell-ms1): close cockpit shell + MS-1 — dual QA pass (R2)`. Live foundation: studio-web SSE + fresh/stale/lost taxonomy + `ms1_gate.sh`; StatusView + read-only AskView; StalenessBadge; ts-rs contract export. Pre-existing tree dirt (NOT mine, NOT touched): `M plans/phase 0/antigravity/*` (3 files), `M scripts/stc_supervisor.py`, untracked `--help`, `cand.py`, `.roadmap/h1-05-views/`, `.roadmap/phase-01-cockpit-beta/`, `skills/`. The 1548-line `cockpit/src/pages/Providers.tsx` (Phase-6 localStorage wizard, dead code — imported nowhere) was left untouched as out-of-scope; see Residuals.

## What was built

**Backend (`studio-web`, `studio-core` additive only):**
- 7 new endpoints, all read projections with staleness badges: `GET /api/war-room` (attention-first buckets needs_input>failed>running>attention>done + permission notifications, f05-paseo-buckets c036–c038), `GET /api/agent-stream` (normalized AgentEvent union + HookProvider protocolVersion v1, unknown versions refused-never-dropped, f05-px-events c035), `GET /api/audit` (kanban + receipt evidence + head-SHA-fenced feedback, f05-vk-kanban-diff c043–c044), `GET /api/mcp` (bundled PATH-probe manifests + override file + listed-never-fetched remote + working/blocked/idle flags, f05-herdr-detect c039–c042, ghostty-vt NOT a dependency), `GET /api/providers` (30s-TTL probe cache + token_ledger budgets + credential-presence ACP auth probes with unknown fallback, f05-openfang-providers c045 + f05-oh-acp-authprobe c046), `GET /api/runs` (≤5 worktree run slots + receipt linkage + stated deferral, f05-openchamber-multirun c049).
- A1 decide: `POST /api/ask/:id/decide` (P02 `decide_stored` CAS; 200/400/404/409/503 typed; every decision lands in `contract_approvals`). Write surface is decide-POST-only; all other routes stay GET-only 405 (contract test renamed `browser_is_read_only` → `write_surface_is_decide_only`).
- `studio-core`: two additive readers (`frozen_candidate_rows`, `token_ledger_by_session`, +2 unit tests) and one additive CDC trigger (`trg_events_log` fans `events` inserts into `change_log` so agent-origin rows are stream citizens). No schema version change.
- Catalog grows 1 → 7 entries; 18 new fixtures (empty/loading/error per view); `EXPECTED_*_KEYS` red-on-stale consts per view; `cockpit/src/api-types.ts` regenerated (24 ts-rs DTOs).
- `scripts/ms1_gate.sh`: the ask assertion legitimately flips with A1 (`decision_enabled:true` for the pending fixture + log line); nothing else touched.

**Frontend (`cockpit`):**
- `ViewShell` envelope (badge + designed loading/error+scoped-retry/empty/ready) shared by all views; `useProjection` split into `useCorePoll` + `useViewPoll` (per-view error slots, `retryView` scoped refetch); hash-routed tabs (`#/war-room` …) so gates screenshot each view; AskView one-tap Approve/Deny enabled only on live paths (`decision_enabled && pending`), decided rows visibly disabled with reasons; War Room desktop-notification opt-in (attention payload pattern only).
- Tokens + tabular-nums: no new colors; inputs use the existing token set (`.field` class). Keyboard: 100% native buttons/inputs, zero `tabIndex` (DOM-order flow), `focus-visible` ring retained, aria labels/titles on every control row.

**Fixes found by my own gates (repro/fix/proof):**
1. P05 gate run 1 FAILED bucket order: two pending approvals put both tasks in `needs_input`, dropping the empty `running` bucket. Fix: all five buckets always emitted (empty renders "bucket clear"). Proof: gate green since.
2. Fallow audit failed on 14 new findings in my TS (11 complexity, 1 dup, 1 dead-code). Fix: extracted single-branch subcomponents + `throwForError` helper + `DecideBody` reuse; 0 error findings after. (The complexity split grew view LOC, e.g. Agent Stream 109 → 159 lines — recorded honestly in §V7.)
3. prek fixer-vs-generator conflict on `api-types.ts` (end-of-file strips the generator's trailing blank line → contract test red). Fix: regenerated byte-exact; contract test is king. See guard note.
4. QA-R1 F1: ViewShell ready-state dropped the per-view error slot (last-good data rendered with a frozen Fresh badge, no retry). Fix: `ReadyNotice` — non-blocking error + scoped retry line above last-good children (`ViewShell.tsx`); proven by a persistent-tab CDP probe in `p05_gate.sh` (fresh headless loads cannot carry last-good state, so the gate drives a remote-debugging tab: load healthy → observe `needs_input` → `chmod 000` → assert `showing last good data` + `Retry War Room` → restore). Total-down stays the App Connecting banner; Status/Ask keep P04 behavior (no per-view error slot there — noted, not changed).
5. QA-R1 F2/F3: `ProbeDto.fresh` was vacuous (always true; `re-probed` branch dead; badge hardcoded fresh). Fix: `fresh:true` ONLY on TTL cache hits; just-executed probes report `fresh:false` with the new `last_probe_ms`. `ProbeRow` badge now varies (`probeBadge`) with titles. Proven by contract test (first read `false`, immediate second `true` + same instant) and gate (double-read + instant equality).
6. QA-R2 F1-empty: all 6 views hardcoded `error={null}` in their empty branches, so the shell could never show a notice there (live CDP repro: empty board + cut DB = frozen FRESH badge + empty copy, no affordance). Fix (same shape as F1-ready, centrally in `ShellBody` + one-word pass-through in each of the 6 views): the empty branch renders `ReadyNotice` above `EmptyBody` when a live error exists. Proven by a second persistent-tab probe in `p05_gate.sh` against a fresh-init empty DB on its own server (marker `No tasks in any bucket` → cut → notice + `Retry War Room`).

## Gate / test summary (commands actually run, final tree)

| Gate | Command | Exit | Result |
|---|---|---|---|
| tests (workspace) | `cargo test --workspace` | 0 | **235 passed, 0 failed** (15 suites: core lib 162 incl. 2 new readers + trigger coverage via contract tests · web lib 14 · web_contract 18 · all P01–P04 suites intact) |
| clippy | `cargo clippy --all-targets --workspace -- -D warnings` | 0 | 0 warnings, no `#[allow]` in source (type alias + doc-blank-line fixes) |
| format | `cargo fmt --check` | 0 | clean |
| frontend build | `(cd cockpit && npm run build)` (`tsc && vite build`) | 0 | clean |
| prek | `prek run --all-files` | 0 overall (2 hooks fail, see guard note) | rustfmt/clippy/gitleaks/fallow/CODEOWNERS Passed; **fallow 0 error findings** after refactor; `trailing-whitespace` + `end-of-file-fixer` fail ONLY on pre-existing files + guarded dossiers (all reverted, see guard note) |
| V8 gate (evidence) | `./scripts/p05_gate.sh` (RUNS=1, `EVIDENCE_DIR=.roadmap/05-cockpit-views/evidence`) | 0 | PASS: 5 buckets ordered + 2 notifications · 1 refused rogue v99 · fence old/current + evidence link · probes/budgets/auth + F2 double-read (re-probe→cached, instant equality) · run slot + deferred · decide 200/409/404/400 + ledger · decide-POST-only 405s · 7 view DOMs + fresh badge · F1 persistent-tab probe (last-good notice + scoped retry) · F1-empty probe (empty board notice + retry, own server) · stale→recovery + direct-503 · Lost + no rows |
| V8 gate (flakes) | `RUNS=3 EVIDENCE_DIR=/tmp/opencode/p05-3runs ./scripts/p05_gate.sh` | 0 | **3 runs, 0 flakes** (bar: 0); log `/tmp/opencode/p05-3runs/gate.log` |
| MS-1 regression | `./scripts/ms1_gate.sh` | 0 | PASS (fresh/stale/lost + updated ask-live assertion) |
| chrome screenshots | headless `google-chrome` 154.0.8037.57 via gate | 0 | 8 PNGs: war-room, agent-stream, audit, mcp, providers, runs, ask, stream-lost (+ gate.log) |
| a11y contrast | `python3 /tmp/opencode/a11y_contrast.py` (oklch→linear-sRGB WCAG math) | 0 | **13/13 token pairs ≥ 4.5:1 AA** (text-dim/surface-0 5.17:1 confirms the token comment) |
| a11y keyboard | grep audit (native controls, no tabIndex, focus-visible, aria) | 0 | all clicks on native `<button>`; inputs labelled; DOM-order tab flow |

File hashes (via `sha256sum`, this run, final tree):

| Artifact | sha256 | Provenance |
|---|---|---|
| `studio-web/src/api.rs` (24 ts-rs DTOs + `export_ts`) | `256b1be4f7b8f9ee168807e8382f53cd6f8f5e314eb8e065a06a8b3f7097a0d6` | extended this phase |
| `studio-web/src/server.rs` (7 views + decide-POST-only router + honest probe TTL) | `89c54557a752c3ea14d4dad4061d9d5aa537bc946190e9bb0fda6305af513df4` | extended this phase |
| `studio-web/src/catalog.rs` (7 entries + V7 revisit) | `3c1008a8b3bb9db53bd6761d05838c3af6d990606e330c6f388a276a8eb82c47` | extended this phase |
| `studio-web/src/main.rs` (+ ProbeCache wiring) | `9724694c04f7a66591e8b6502a22ea0e5a435639b1eece9b08a5e1bffa8edc06` | extended this phase |
| `studio-core/src/state/mod.rs` (+2 readers +1 trigger +2 tests) | `6428fabc3f97768b147aac5c3eedf25d449113363443407d7108adee7360c20d` | extended this phase |
| `cockpit/src/api-types.ts` (regenerated, byte-exact) | `9e4e233f438fdb6df2d64bd7c40b733ca21a919344d1f3c22f5e1ae14cc60820` | regenerated this phase |
| `cockpit/src/api.ts` (+6 fetchers + decide) | `5460f2e3b673c4ccc775553fde1f576dd2a4f8558fd14dd6d8a3d7f032a49e4c` | extended this phase |
| `cockpit/src/App.tsx` (8 hash-routed tabs) | `874049c101ab413eb0e62e8651a808ace1b86feda307ab90763fd6b0627f5d7e` | extended this phase |
| `cockpit/src/hooks/useProjection.ts` (core+view polls) | `59ccf263ff4e8ad00c962209df27d6ab27e1af244f29b0efb2180824ee83b1b4` | rewritten this phase |
| `cockpit/src/components/ViewShell.tsx` (shared envelope + F1/F1-empty ReadyNotice) | `9b6a88c78ab59ca9fe472a6bd3938be642542de191e78d0a9d2254da2473683f` | written this phase |
| `cockpit/src/views/WarRoomView.tsx` | `a151dd67a8efa5044a8da1651a3a2c2bdd93adfa9d0531cf2acc33a37525ecb0` | written this phase |
| `cockpit/src/views/AgentStreamView.tsx` | `cc778df8b0804e9ed660087cbcd5a0267093c9c93ccb3538a90e16114bfa4c77` | written this phase |
| `cockpit/src/views/AuditView.tsx` | `38aadbc35f073d9d3d6f98a6af0ed256bf2ced8ab5efbd7fb557471bc8aadd4d` | written this phase |
| `cockpit/src/views/McpView.tsx` | `5b512c14ba4bdc9c8c2ae98796eab205bba731c9b4d926a6fcad40ea38321beb` | written this phase |
| `cockpit/src/views/ProvidersView.tsx` | `559cc0797c60979d2b9caf708bca9e2f6e289839ee0ed9318d77f2c17544ae47` | written this phase |
| `cockpit/src/views/RunsView.tsx` | `40d65b580212322655389c2df22e010423cec784c736108b8600f1a23b912cd5` | written this phase |
| `cockpit/src/views/AskView.tsx` (A1 decide wiring) | `74a96ea499db0b5541e1b357ed14ad3cf6653be87f0ef095fa0d324cfb58cd8a` | rewritten this phase |
| `scripts/p05_gate.sh` (V8 per-view gate + F1 CDP probe + F2 double-read) | `b2c1e930a7e5dcd3d51f9afcc44377990e18e2dcb27993b56f85c5a5edbb75cb` | written this phase |

Counts (exact): 7 new `.tsx` files (6 views + ViewShell; AskView/App/api/useProjection/tokens modified), **18 new fixture JSONs**, **1 new gate script**, **15 backend/frontend files modified** (Cargo.lock, App, api-types, api, useProjection, tokens, AskView, ms1_gate, state, Cargo.toml, api, catalog, main, server, web_contract), **18 contract tests total = 11 at base + 7 new fns** (`decide_round_trip`, `war_room`, `agent_stream`, `audit`, `mcp`, `providers`, `runs`) **+ 2 rewritten** (`browser_is_read_only`→`write_surface_is_decide_only`, `ask_queue_is_read_only_and_disabled`→`ask_queue_decide_is_live_for_pending`; fixtures test generalized, catalog unit updated), **2 new core unit tests**.

### §V7 — codegen revisit (measured on view #2, Agent Stream)

Hand-written `AgentStreamView.tsx` = **159 lines** (after the complexity-gate split: provider pills + refused banner + gap notice + event rows as single-branch pieces). Shared envelope factored once: ViewShell 89 + StalenessBadge 29 = 118 lines over 8 views ≈ 15/view; catalog entry ≈ 8 lines/view. A generator for view #2's body would need a template expressing refused-styling + provider pills + gap latch (≈60-line partial, measured from the actual extracted pieces) + ~20-line schema + envelope reuse ≈ **~170 lines to generate the first 159-line view** — kill HOLDS on the first-view clause (narrowly; estimate built from measured parts, stated as estimate). The envelope factorization (ViewShell + CATALOG) already captures the amortization a generator would offer; generating view bodies is correctly declined.

### §V8 — per-view gate reuse

`scripts/p05_gate.sh` extends the `ms1_gate.sh` pattern per view: seeded fixture ledger (`tasks=5 change_seq=7 approvals=2` — the 7 proves the P05 `trg_events_log` fan-out), frozen clock (`STUDIO_MS1_FROZEN_NOW_MS`), fresh/stale/lost assertions on the streamed views, fail-closed 503 + recovery on direct views, typed decide round-trip, 7 hash-route screenshots + DOM badge checks, zero-mock grep. 3/3 runs, 0 flakes.

### a11y baseline (measured, not adjectives)

Contrast (oklch→linear-sRGB, WCAG ratio): text-0/surface-0 15.30 · text-1/surface-0 8.45 · **text-dim/surface-0 5.17** · text-0/surface-1 14.09 · text-dim/surface-1 4.76 · status-building 7.07 · validating 10.03 · review 7.04 · ready 8.99 · diff-added 8.99 · modified 10.03 · deleted 6.01 · renamed 7.22 — **13/13 ≥ 4.5:1 AA**. Keyboard: every interactive element a native `<button>`/`<input>`; `tabIndex` count 0 (DOM-order flow); `:focus-visible` 2px outline; per-row `title`/`aria-label` on all 8 views (grep counts in §gate table row). Script: `/tmp/opencode/a11y_contrast.py`.

## Acceptance checklist (evidence paths)

- [x] all five views verified live with real data + screenshot each — `.roadmap/05-cockpit-views/evidence/{war-room,agent-stream,audit,mcp,providers}.png` + `runs.png`, `ask.png` + DOM `data-view` asserts in `gate.log`
- [x] empty/loading/stale/error states designed with per-view scoped retry — `ViewShell` + `retryView(name)`; error path exercised (direct-503), empty via fixtures, stale/lost via gate
- [x] zero mock data — gate greps every body for `mock` (fail on hit); fixture ledger + `change_seq=7` proof
- [x] tokens + tabular-nums consistency — no new colors; `.field` reuses tokens; `tabular-nums` on body + nums
- [x] keyboard nav + AA contrast baseline measured — §a11y (13/13 AA, native controls, DOM-order)
- [x] builds green — workspace 235/0, clippy 0, fmt clean, vite clean, fallow 0 errors

## Guard note (P02–P04 lessons applied)

`prek run --all-files` fixer hooks modify guarded files in place: `end-of-file-fixer` retouches 12 `.roadmap` dossiers/phase files (incl. the phase `dossier.json` I must not modify) and disagrees with the ts-rs generator on `api-types.ts`' trailing blank line (contract test `ts_export_matches_committed` is king — regenerated byte-exact); `trailing-whitespace` fails on 4 pre-existing files (`plans/phase 0/antigravity/*` ×3 + `scripts/stc_supervisor.py`, whitespace-only vs HEAD, pre-session dirt). ALL fixer damage reverted (`git diff --name-only | grep '^\.roadmap/' | xargs git checkout --`, api-types regenerated); verified pre-existing by stashing my work and re-running prek (same 2 hooks fail on the clean tree). The 4 pre-existing files were left byte-identical to session start. No `#[allow]`/`eslint-disable` added anywhere.

## Residual limitations (not overclaimed)

1. **Embedded terminal (f05-agentdeck-terminal c047–c048): PATTERN ONLY, deferred.** No PTY code written: terminal visibility lands only if MS-2 needs a raw PTY view. Nothing in P05 requires it; the five views + runs are fully served without a terminal surface.
2. **Multi-run compare scoped:** run slots (≤5) + receipt evidence counts are live; per-model columns + guided changes walkthrough are DEFERRED and stated in `RunsDto.deferred` — schema v2 tasks carry no run/model linkage, so a matrix would be mock data.
3. **MCP remote manifests** are listed-never-fetched (no silent network); override requires `STUDIO_MCP_MANIFEST`. Auth `loggedIn-json`/`stderr-text` mechanisms report `unknown` (presence checks are the live path).
4. **`cockpit/src/pages/Providers.tsx`** (1548-line pre-existing localStorage wizard, imported nowhere) left untouched — out-of-scope dead code; the P05 Providers view is `views/ProvidersView.tsx` (server-backed). Suggest removal in a later phase, not this one.
5. **Desktop notification** is an opt-in attention pattern (`Notification` API); no mobile/push code exists anywhere (scope firewall holds: no marketplace/cloud/mobile/voice, no harness OAuth reimpl).
6. **Screenshot review** is file evidence (8 PNGs + DOM asserts); no desktop browser was connected to this session for live preview.
7. **A1 ledger note:** "every decision lands in the ledger" = the durable `contract_approvals` row (CAS outcome + `decided_ms`); no separate decision-journal table was created (the approvals table IS the ledger per P02).
