# Phase 4 Checkpoint (data path proven 2026-09-26; desktop packaging pending)

## Implementation
- `studio-core/src/projection/`: read-only `Snapshot{fleet, stream, receipts, burn,
  change_seq, db_age_ms}` + `lane_for` (building/validating/in-review/ready) +
  `stale_badge` ("source: studio.db · updated Nms ago"). `StateStore` gained
  `upsert_task/append_event/append_receipt/snapshot` + UPDATE-status CDC trigger.
- `tui/`: `studio-tui` Ratatui binary — header (4 views + badge), War Room fleet
  lanes w/ color, Agent Stream spine, Audit evidence-first receipts, MCP Registry
  matrix + burn; tab/1-4/q keys; 100ms poll. Builds clean.
- `cockpit/`: React/Tailwind stub (`tokens.css` oklch dark set + diff tokens +
  tabular-nums; `App.tsx` 4 sections polling `/snapshot` every 100ms) +
  `src-tauri/main.rs` exposing `snapshot(db_path)` via studio-core.
  Full Tauri desktop packaging (webkit system deps, npm build) pending.

## Exit-gate evidence (headless cockpit drive, throwaway)
- E2E_OK scope->DAG->dispatch->review->merge; EPIC fleet=3 stream=2 receipts=1 burn=1
- War Room shows t0 ready; Stream shows dispatch; Audit shows delta evidence;
  Registry shows coder-1 burn; badge names studio.db
- `cargo test` studio-core 31/31; `cargo build` tui ok

## Update (2026-09-26, later turn)
- `cockpit/src-tauri`: proper binary layout + `tauri.conf.json` + `build.rs` +
  generated icons; `cargo check` AND `cargo build` (full desktop binary link) pass.
- `cockpit/`: `tsconfig.json` + `vite.config.ts` + `index.html` + `main.tsx`;
  `npm install` + `npm run build` succeed (`dist/` with JS/CSS/HTML).
- `adapters/acp`: NDJSON stdio bridge verified live (initialize/new/prompt result
  frames; permission deny with SCOPE_VIOLATION; unknown method -32601).
- Remaining: `tauri build` installer bundle + app signing (packaging, not function).

## Update: installer bundles built
- `tauri build` exit 0: `Studio_0.1.0_amd64.deb` (4.2MB), `.rpm` (4.2MB),
  `.AppImage` (112MB ELF) under `src-tauri/target/release/bundle/`.
- Frontend data path fixed: `App.tsx` invokes Tauri `snapshot` command with
  `dbPath` (canonical); `/snapshot` HTTP kept as browser-dev fallback.
  Rebuilt `dist/`; second `tauri build` rebundles with fixed UI.
- Signing: none (unsigned local-first artifacts; no cert configured).
