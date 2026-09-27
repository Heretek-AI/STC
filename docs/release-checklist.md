# Release checklist (issue #16)

## Version bump

All manifests carry the release version and must agree
(`scripts/release_check.sh` enforces this):

- `Cargo.toml` (workspace) + `studio-core/Cargo.toml` +
  `adapters/cli/Cargo.toml` + `tui/Cargo.toml` +
  `cockpit/src-tauri/Cargo.toml`
- `cockpit/package.json` + `cockpit/src-tauri/tauri.conf.json`

## Build (unsigned — runnable today)

```bash
cargo test && cargo clippy --all-targets -- -D warnings
npm --prefix cockpit run build
cargo build --release -p studio-cli -p studio-tui
```

## Sign (PENDING OWNER — blocked:owner)

John provides distribution certs. When they land:

1. Wire certs into `cockpit/src-tauri/tauri.conf.json` bundle signing
   config (bundles only). **Never commit private material** — certs arrive
   as files/CI secrets, referenced by path, gitignored.
2. Produce signed `.deb` / `.rpm` / `.AppImage`.
3. Verify signatures on a clean host (fresh container, no repo checkout):
   `dpkg-sig --verify`, `rpm -K`, AppImage digest compare.
4. Record the verification log on the issue before publishing.

## Publish

1. Tag `vX.Y.Z`; CI builds release artifacts.
2. Attach signed bundles + checksums to the GitHub release.
3. Mirror RolePacks: `studio-cli export-packs --dir ../STC-Marketplace/`.
4. Update `docs/profile-catalog.md` if the catalog changed.
