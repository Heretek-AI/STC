# Studio — 05 OSINT & Open Questions (Phase 5)

Folded from live OSINT (Sep 2026) + local audit. Read-only research, no code.

## 1. Other orchestrators + community likes / missing

Taxonomy (`awesome-agent-orchestrators/README.md:30-167` + web Sep 2026):

- **Parallel TUI/CLI:** claude-squad, agent-manager, cmux, dmux, amux, swarm-code (RLM `thread()/merge_threads()`, auto-route, budget cap), claude-swarm (tmux send-keys + `/tmp/claude-shared/status|tasks|results`), openclaw-agent-swarm (`active-tasks.json`, stuck >60min flag, max 3 respawns).
- **Desktop/Web:** Superset, Vibe Kanban / easy-vibe-kanban, Cline Kanban, Kagan, agent-board, Orca. Kanban `Backlog→In Progress→Review→Done`, per-card worktree+terminal, inline comments → agent, `Commit/PR` from card.
- **Infra/primitives:** Concord MCP (claim work, collision detect, handoff evidence), foremerge (intent/scope declare before write + verification gate, SQLite), guild (Go SQLite hybrid search), AX (Google Sep 2026, K8s-style), LangGraph / MS Agent Framework.

Community consensus (Builder.io "AI Agent Orchestration is Broken", HN local merge queue, r/LLMDevs worktrees, DEV "It Got Messy Fast"):

Likes: worktree isolation, persistent tmux, diff + Apply/Keep/Drop, port plan (A 3000 / B 3001 + symlink `.env`), `claude --worktree <name>` native since v2.1.49.
Missing: worktrees solve isolation, not coordination — who works on what, overlap early, stale work (main moves mid-run), landing order, retry, notification deep-link to needing agent. 90 pushes/day = CI bill + 4 builds melt 8GB Air → local merge queue (turnstile, full tests, no PRs). MCP = N server processes RAM.

Studio take: ship per-worktree `CLAUDE.md` declaring owns/off-limits, file-claim before edit, overlap detector, stale rebroadcast, Bors-style queue — already specced in 01/P2.

## 2. Complex tasks: 3D / image-video / desktop-web

Keep out of coder. Specialist workers with narrow MCP:

- **3D (Blender):** `blender-orchestrator` pattern: `create_material/generate_texture/generate_mesh_from_text|image/import_mesh + list_objects/get_scene_summary + spatial toolkit`. Research (LL3M arXiv:2508.08228, Planner-Actor-Critic 2601.05016, EZBlender WACV26): planner → retrieval (docs/PolyHaven) → coding (Blender Python) → critic (VLM screenshot) → verification. Preview-before-commit: chat → preview image iterate → commit mesh. Caveat: dense tris, retopo needed for deformables. Studio: `Asset3D` worker in disposable worktree/blender-headless, returns `preview.png` for critic, then `GLB/FBX + .blend`.
- **Image/video/marketing:** Orkas pattern `ImageStudio (HTML/CSS/SVG first, model when needed) + VideoStudio + PptMaker`; EvoLink `Blender render + Midjourney first-frame → Seedance reference-video → Topaz 4K`, one key, MCP per stage. Studio: deterministic HTML-first draft, generative fill only for hero, outputs stay editable (iPolloWork rule).
- **Desktop/web:** Orca `orca computer list-windows/get-app-state/click/set-value/type-text/scroll/drag --json` via a11y + screenshots, secrets via stdin; MS Copilot CUA same (vision+reason, adapts to shift). Orca Design Mode: click element → HTML/CSS/screenshot → prompt. Rule: prefer API/MCP when exists; computer-use only for no-API flows, sandboxed VM, allowlisted apps, human gate.

## 3. Stack distribution

- Daemon (Go, loopback only) owns worktrees/PTY/SQLite/CDC; adapters leaves (01 ADR-1/2).
- Tool plane: shared catalog process + ephemeral per-agent MCP server (Paseo `mcp-server.ts` thin proxy). Heavy MCP (browser, blender, video) as sidecars with port stride `PORT=base+i*stride`, `cache_redirects→lane-<i>`.
- Compute: local lanes default; SSH worktrees for beefy box (Orca auto-reconnect+forward); cloud sandboxes (E2B/Daytona/Modal) for untrusted renders. Never share one checkout across hosts — `repoId::path` + host-qualified `wt2:host:inst`.
- Model plane: `modelTier smol/regular/smart/ultra` intent + concrete override; explorer→flash/lightning-free, coder→strong code, reviewer→different family. Zen free (Sep 2026): `mimo-v2.5-free, ling-3.0-flash-fin-free, nemotron-3-ultra-free, nemotron-3.5-lightning-free, big-pickle, muse-spark-1.3-contributor-free` via `opencode/<id>`; `pi-opencode-zen/pi-opencode-free` shims send `x-opencode-client` headers for keyless free.

## 4. Long-term data: files vs vectors

Converged answer: start files+SQLite, add vectors lazily.

- Files win (200+ sessions practice): deterministic, editable, zero-ops, token-efficient. `MEMORY.md + memory/<role>.md (≤80 lines pruned) + .swarm/context.md + plan-ledger.jsonl`.
- Vectors help when: 1000+ file unfamiliar codebase, multi-agent different views, "last time X failed because Y" across histories. Pure vector n.s., pure BM25 degrades; hybrid wins (MRR 0.759, 11x compression). Prod: 49k chunks/15.8k files/83MB/8MB model, drift cosine 0.30 100% precision. OpenClaw RAG-lite: `files(mtime/hash) + chunks(text+embedding) + chunks_vec(sqlite-vec) + FTS5`, RRF 0.7/0.3, 512/64 overlap, on-device `bge-small` 95MB or `nomic-embed`, fallback to FTS if ext missing.
- Breakpoint: not size but sync logic accumulating (file sync + second vector store + metadata tables + local API). Then move to service; keep single-file `.db` until then.

Find↔return agents: `explorer` (read-only scan) → `curator_init/phase` (phase digest, auto-retire skills >30% violation) → `sme` (cached guidance) → `memory_search/read`. Propagation logs `skill-usage.jsonl`, relevance ≥0.5, max 5/delegation. Markdown source of truth, embeddings cache + content-hash skip.

## 5. Human feedback, assign/review, merges, anti-stall, finish

- Feedback UI: card `In Review` → diff → `+` line comment → `Send` batches → back to `In Progress` (Vibe); approval cards `Accept/Reject/Approve all`; Orca annotate+batch; agent-board SSE chat + plan-approval gate. Studio Audit view = this + evidence checklist (review/tests/sast/secrets/drift).
- Assign: Leader (Ed25519 JWT) creates/assigns, Worker claims, Daemon spawns/closes; or orchestrator splits, server moves handoff (`STATUS.md handoff_ready:true + state:done/review` → streamer → test/diff_review → Done → git_pr). Kagan gate: read-only intake confirms assumptions → sandboxed worktree (no push, merge only via dialog) → reviewer classifies `misalignment/bug/uncertainty` by confidence → human triages.
- Merges: local turnstile — one at a time, full tests, `merge --abort` on `diff --diff-filter=U + ls-files -u`, typed `{conflictFiles}`; `prune` before `branch -D`; file-disjoint fast path else serial.
- Anti-stall: heartbeat must be activity-gated (OpenClaw 2500 credits/day idle bug). Shapes: Repeater (same args Nx) / Wanderer (activity, no goal progress) / A-B-A-B. Ladder: nudge (block exit 2 "edited X 3x, try Y") → replan → escalate model/effort → context reset from progress file/git green → human handoff → abort logged. Progress metric must monotonically rise (pass rate, unique sources), flat N beats = stuck. Tombstone context so restart doesn't repeat.
- Finish guarantee: never accept prose "done". Require `phase_complete` + evidence (reviewer APPROVED + tests + sast + spec-hash drift match) + `is-task-settled` + WAL `COMMITTED`. Full-Auto routes ambiguous via read-only `critic_oversight`; repeated denials pause; completion needs APPROVED record. Early tap-out triggers PRM `stuck-on-test/expansion-drift`; settlement ownership blocks new dispatch until settled or operator `--force`.

## 6. Templates, OAuth, profiles

- **Pi templates:** yes. Pi is KDL-driven + `toolNames/restrictToolNames` + scopes `workspace|sandbox|coordination|device`. Ship `templates/skills/<name>/{config.json,content.md,files/}` seeded + `skill-routing.yaml` per role + `SKILLS:` field (enforce blocks missing). Prebuilds: `coder-blender, researcher-docs, reviewer-diff, video-pipeline, computer-use`.
- **OAuth:** Claude Code MCP OAuth: 401 → browser flow, token in OS keychain keyed server+hash, auto-refresh, step-up on 403 `insufficient_scope`, `claude mcp login/logout <name>`, `--callback-port`, lazy-auth (public anon, 401 only on protected). Headless SDK: complete flow in app, pass `Authorization: Bearer`. Studio: secret broker write-only, redacted `[env:,headers:]` in UI, `allowSecretEgress` per tool, `scrubSecrets` at egress, never log raw creds.
- **Multi-profile:** Claude natively weak — `ANTHROPIC_PROFILE` + `~/.claude/.credentials.json`, one active; community `cloak` (`create/switch work|home`, `claude -a work`) preserves sessions/tokens/MCP, `aistat` adds Codex + JSON; issue #27359 = native profiles landing. Codex app-server: `account/login/start {apiKey|chatgpt|chatgptDeviceCode|amazonBedrock}` + `account/updated{authMode,planType}` + Bedrock discover (metadata only). OpenClaw workaround: declare `auth.profiles {shared-1,shared-2,private-1}`, login lands in `default`, snapshot → remap → `auth order set`, replicate file. Studio: `cloak`-style profile dir + `CODEX_HOME`/`CLAUDE_CONFIG_DIR` per lane + quota rotation + usage UI (5h/7d resets).
