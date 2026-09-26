# Phase 2 Checkpoint (exit gate satisfied 2026-09-26)

## Implementation (`studio-core/src/mcp/`)
- `registry.rs`: `ToolDef` catalog (15 tools), `Access {Allow,OnDemand,Deny}`,
  `RoleManifest::check` (fail-closed SCOPE_VIOLATION), `default_manifest` per role,
  `Gateway::index_for` (compact ~20 tok/tool), `tool_open` (resolution-time manifest check),
  `derive_tool_map` by inversion, `catalog()`
- `write_detect.rs`: 8 `WriteCategory` (redirect|here_doc|builtin_write|inplace_edit|
  interpreter_eval|network_download|archive_extract|git_destructive),
  `detect_shell_writes` (pure scan, fail-closed on $VAR/$(...)/backticks),
  `WRITE_TOOL_NAMES` (11 tools), `resolve_write_targets` (count/length caps,
  SCOPE_NOT_DECLARED / SCOPE_ROOT_ESCAPE / SCOPE_VIOLATION)
- `process.rs`: `ProcessSpec{program,args,cwd,timeout=30s,max=8MiB}`,
  array-spawn only, shell deny, stdin null, bounded sink, kill_on_drop (finally),
  Timeout/Overflow/Spawn/Denied errors
- `overflow.rs`: `WIRE_LIMIT 10KB`, `mcp:overflow:<agentId>`, 24h TTL, `collapse`/`fetch`
  (kv_get exempt)

## Exit-gate evidence (`cargo test` 19/19 + scoreboard harness, throwaway)
- ROLE manager tools=8 cut=22.9x; researcher 7/25.2x; coder 8/27.0x; reviewer 7/23.8x
- BENCH n=50 11.17x; n=100 16.81x; n=200 22.47x (toward ~35x; scripts-only 36.4x per H3)
- PARITY_OK 15 catalog tools, all registered (no phantom tools)
- SCOPE_OK: coder `write` ok, `sbom` denied, unknown denied
- OVERFLOW_OK: 1MB -> `mcp:overflow:coder-1` pointer, not flood
- Write: 8/8 categories detected; $VAR fail-closed; `../etc/passwd` root-escape denied
- runProcess: `bash -c` denied (shell:false); `true` ok
