# Studio — 02 MCP Proxy Gateway (Phase 2b)

Goal: agents never see 50+ schemas. Router slices per role at session start; ephemeral per-agent server, shared catalog process.

## 1. Registration (single source)

- Paseo pattern `paseo-tools.ts:559-625`: `createPaseoToolCatalog(deps)` + `registerTool(name,config,handler)` gated by `isPaseoToolEnabled(policy,name)` BEFORE registration. `PaseoToolDefinition{name,title,description,inputSchema,outputSchema,handler(input,ctx{signal,sendUpdate})}`; `parseToolInput` zod `parseAsync`.
- Swarm pattern `tool-metadata.ts:37-1062`: `TOOL_METADATA: Record<ToolName,{description,agents[],prWorkflow?}>`; `agents:[]`=overlay-only. Derived `ToolName/TOOL_NAMES/AGENT_TOOL_MAP` by inversion; `defineHandlers<T extends Record<ToolName,()=>ToolDefinition>>` makes missing/stray compile error; `index.ts` barrel required; `buildPluginToolObject` sole `tool:{}` assembler (`plugin-registration.ts:36-124`, called `index.ts:3752`).
- Agent-swarm `createToolRegistrar(server)(name,{description,inputSchema,outputSchema:swarmToolOutputSchema(dataShape?),handler})` sole `content/structuredContent/isError` builder; tools return `SwarmToolResult{message,details,data,nudge}` never raw `CallToolResult`.
- Enforce CI: `check-tool-registration` / `drift-check --enforce` + `doctor tools` + `bundle-portability` (no top-level `bun:`, Node-fallback loader).

Studio schema: `{id,version,agents[],scopes[],featureFlag?,sandbox,prWorkflow?:observe|validate|null}` + zod `input(strict)/output(looseObject, optional, no uuid/email pins)` + `TOOL_MANIFEST` thunk + barrel export.

## 2. Slicing per role

Base `agents[]` inversion + opt-in overlays merged conditionally (`agents/index.ts:1378-1468`):
- `MEMORY_*` when `memory.enabled`; `SKILL_*` when `skills.enabled` (authoritative strip even if override lists); `COUNCIL/GENERAL_COUNCIL` (adds `web_search/fetch` only there); `TURBO_*` when turbo configured; `EXTERNAL_SKILL_*` when curation enabled; `PR_REVIEW_CHILD_*` only for `dispatch_lanes` children, never ordinary configs.
- Paseo `paseoTools{enabled,disabledTools[]}` per-provider + `speak` always-on + `injectIntoAgents` global override; `tool_filter.overrides[role]` replaces base then overlays union.
- Studio roles: Coder={fs-edit,LSP,exec,worktree,dispatch-lanes-read}; Researcher={search,symbols,doc-fetch,browser,memory_search/read}; Reviewer={diff,syntax_check,sast,placeholder_scan,retrieve_*}; Architect=all + council/skill maps.
- PR fail-closed: omitted `prWorkflow` → `null` → denied in `PR_REVIEW/PR_FEEDBACK`; only `observe|validate`; `retrieve_summary` explicitly allowlisted recovery path.

## 3. Enforcement (least-privilege)

- Single `WRITE_TOOL_NAMES=[write,edit,patch,apply_patch,swarm_apply_patch,create_file,insert,replace,append,prepend,extract_code_blocks]` (shell excluded, separate).
- `resolveWriteTargets`: scalar `path,filePath,file,target` + arrays `files[],paths[],targetFiles[]`; caps `MAX_PATCH_FIELD 1M / AGGREGATE 2M`; CRLF+indent normalize, native vs unified classify, quoted/whitespace-ambiguous → fail-closed, contradictory add/delete → fail-closed, `swarm_apply_patch` requires `files[]` and parsed ⊆ declared.
- `scope-guard` (coder-only, architect bypass): `resolveSessionWorkspaceDirectory(session,fallback)` per-session root → resolve targets → `unverifiable` throw → require exact active v2 scope binding (+ PR-feedback + disk recovery) else `SCOPE_NOT_DECLARED` + architect advisory → `isPathIdentityWithin` else `SCOPE_ROOT_ESCAPE` → `enforceProtectedPathAuthority` → `isFileInScope` else `SCOPE_VIOLATION` (diagnostic bounded agent 128/path 256/scope 3).
- `shell-write-detect`: 8 cats `redirect|here_doc|builtin_write|inplace_edit|interpreter_eval|network_download|archive_extract|git_destructive`; subshell `cd` tracking; `$VAR`/`$(...)` unresolvable → deny; destructive `rm -rf` blocked unless all targets in scope; universal-deny prefix + protected paths.
- Chokepoint orca `runProcess`: `ProcessSpec{program,args,cwd,env,timeoutMs=30s,input,maxOutputBytes=8MiB,signal,detached}`; `runProcess` never rejects on non-zero; `bounded-output-sink` distinguishes short vs clipped; `windowsHide,shell:false`, shim resolve to skip `cmd.exe`; ratchet forbids direct `child_process`.
- oh-my-pi scopes `workspace|sandbox|coordination|device` + approval `allow` + ACP destructive prompt; Paseo `resolveScopedCwd` child inherits `parentCwd` unless `allowCustomCwd && lockedCwd`, `requireCallerHeartbeat` binds ops to `callerAgentId`.

## 4. Anti-flood

- Wire measure: agent-swarm `MCP_RESULT_WIRE_LIMIT 10KB`, prose preview 1200 chars, payload LAST so tail-truncate preserves head; overflow key `v1/tool/hash(sha256)` → 24h `mcp:overflow:<agentId>` KV via `upsertKv+sweepExpired`, same pointer both channels, per-agent auth (`mcpOverflowAuthError`), `kv-get` exempt (retrieval path), script SDK 64MiB throw.
- KV guards: `MAX_KV_BODY 2MiB` reject, `MAX_KV_LIST 1000` (default 100), `DB_QUERY_MCP_MAX_ROWS` + wall-clock budget + `truncated` flag.
- Summarizer (opencode-swarm `tool-summarizer.ts`): `tool.execute.after`, threshold+hysteresis, `MAX_ALLOCATION 8`, store `.swarm/summaries/` + `retrieve_*` floor 8 exempt (`retrieve_summary,retrieve_lane_output,task,read,dispatch_lanes(+async),collect_lane_results,parse_lane_candidates`) — retrieval loop + ref-carrying lane tools self-bounding.
- Context-budget (architect-only `messages.transform`, `MAX_TRACKED 256`): classify+priority, exempt preserved, oversized masked/pruned with `output_ref`; tokens via `estimateTokens ~0.33/char` unless provider usage authoritative; live model limit → static fallback → 128k.
- Paseo `count/ids+JSON` materialization for `structuredContent`-only tools; per-tool `limit` validation; `selectItemsByProjectedLimit`.
- Claude-smart dedup: `search_all(project,query,top_k,session_id)` server remembers per-session seen, skip + backfill next-best; `PreToolUse` matcher narrow (Edit|Write|NotebookEdit|Bash) = least-privilege JIT.
- Central `NUDGES` map, not ad-hoc strings; measure composed wire, arrays truncated in place with `truncation` pointer.

## 5. Sandbox

API owns DB, workers HTTP; `callOrigin mcp|script-sdk|extension` drives bypass (script/extension bypass model ceiling, mcp spills; extension bypasses hooks no recursion). Scripts: `Bun.spawn` + `ulimit -v524288 -t60 -u32 -f65536 -n64`, `bun --no-orphans`, Abort 30s (≤5m), 1MB stdout, config stdin `SwarmConfigPayload` (bearer `Redacted`, never env), `tsc --noEmit` on upsert vs skip inline, FS `none`=tmpdir `workspace-rw`=501 v1. Workspaces `local|worktree`, cwd inheritance locked. `.swarm/` only, no `process.cwd()` in tools/hooks.
