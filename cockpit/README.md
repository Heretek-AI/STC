# Cockpit (Phase 4)

- `tui/` (workspace root): Ratatui terminal shell — `cargo build` passes.
  Four views, 100ms CDC poll, staleness badge. Run: `studio-tui ./studio.db`.
- `cockpit/src/`: React/Tailwind frontend sharing the oklch token set (`tokens.css`).
  `App.tsx` polls the `snapshot` projection every 100ms across the 4 views.
  `npm install` + `npm run build` succeed (`dist/`).
- `cockpit/src-tauri/`: Tauri backend exposing `snapshot(db_path)` from
  `studio-core::projection`. `cargo check` and `cargo build` (desktop binary link)
  pass. Installer bundle (`tauri build`) + signing still pending.
- Headless proof: `cockpit_e2e` example drives scope→DAG→dispatch→review→merge
  with all four views reflecting `studio.db`.
