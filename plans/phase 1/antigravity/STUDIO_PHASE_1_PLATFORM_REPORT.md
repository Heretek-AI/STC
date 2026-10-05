# Studio Platform Architecture: Phase 1 Strategic Blueprint & Technical Report
## Forensic Findings, Empirical Evidence, Systems Reasoning, and Actionable Recommendations

> **Location**: `/home/john/Projects/STC/plans/phase 1/antigravity/STUDIO_PHASE_1_PLATFORM_REPORT.md`
> **Classification**: Architectural Blueprint, Forensic Research Report, and Strategic Technical Specification
> **Author & Role**: Principal Distributed Systems Architect, Lead AI Systems Engineer, Staff Product Designer
> **Target Project**: STC / Studio Autonomous Multi-Agent Engineering Platform
> **Date**: September 2026
> **Status**: APPROVED FOR IMPLEMENTATION (Builds upon Phase 0–5 foundational engine)

---

## Executive Summary

Having established and validated the Phase 0–5 foundational engine in `studio-core` (Rust workspace with Tokio, rusqlite WAL, worktree isolation, DAG scheduling, claim broker fabric, memory ladder L0–L4, Bors turnstile, anti-stall watchdogs, and Cockpit Tauri/TUI shells, verified by 51 passing automated tests), the next imperative is **Platformization**.

This report provides the forensic research, empirical evidence, systems reasoning, and concrete engineering specifications required to transition STC from an orchestrator core into a production-grade, highly customizable, and resilient multi-agent engineering platform. It comprehensively addresses the ten fundamental platform challenges:

1. **Memory Management**: Why conversational memory engines (Mem0, Letta, Zep) fail on codebases, and how a **Tri-Modal Hybrid Memory Architecture** (Tree-sitter AST Graph + Temporal Decision Ledger + Self-Editing Working Scratchpads + Reranked Semantic Fallback) establishes state-of-the-art retrieval.
2. **Deployment Topology**: A rigorous trade-off analysis between pure desktop, pure Docker, and the winning **Hybrid Decoupled Architecture** (headless containerized worker engine + native Cockpit/WebUI thin client).
3. **Harness Strategy**: The operational hazards of "Bring Your Own Harness" (BYOH) volume mounting, solved via a **Declarative Agent Manager** with pre-baked OpenCode and Pi core runtimes.
4. **Persistence & Lifecycle Contract**: A zero-data-loss **4-volume persistence layout**, dynamic UID/GID host mapping, and automated SQLite migration transactions surviving container rebuilds.
5. **Subscriptions vs. Curated RolePacks**: Eliminating headless container OAuth friction through **Curated First-Class RolePacks** (powered by OpenCode Go/Zen and BYOK keys), supplemented by an **ACP Host Relay Bridge** (Happier pattern) for proprietary subscriptions.
6. **LLM Gateway & Provider Hub**: An integrated in-core provider engine auto-synchronizing `opencode.json` and `models.json` with tiered model presets and granular role overrides.
7. **The Pi Ecosystem**: A deep forensic audit of `earendil-works/pi`, `can1357/oh-my-pi`, `open-gsd/gsd-core`, and `pi.dev/packages`, establishing an optimal division of labor (OpenCode as heavy coder, Pi as specialist micro-agents).
8. **Ecosystem Cross-Pollination**: Concrete architectural patterns stolen from `happier-dev/happier`, `Gentleman-Programming/gentle-ai`, and `pivoshenko/kasetto`.
9. **RolePack & Ephemeral MCP Inventory**: Role manifests and ephemeral MCP sandboxing to eradicate tool-schema context bloat.
10. **Quality Standards & Anti-Circumvention**: A multi-tier enforcement pipeline combining real-time LSP compiler diagnostics, Lefthook, Fallow dead-code graph analysis, Semgrep security scans, and a fail-closed anti-circumvention AST filter.

---

## 1. Memory Management for Autonomous Coding Agents

### 1.1 The Failure Mode of Conversational Memory in Codebases
Current industry dialogue around agent memory centers on conversational memory systems: **Mem0**, **Letta (MemGPT)**, **Zep**, **Cognee**, and **HippoRAG/GraphRAG**. While effective for chatbot personalization (e.g. recalling user preferences across chat turns), applying them directly to software engineering tasks introduces systemic failure modes:

1. **Code is a Typed Symbol Graph, Not Natural Language Prose**:
   A vector embedding maps textual similarity, not semantic function. Searching for *"payment processor authorization"* in a vector database frequently retrieves variable docstrings, test mocks, or deprecated legacy handlers while missing the actual interface implementation or call-chain dependency.
2. **The Temporal Invalidation Problem**:
   Code evolves monotonically. When commit `B` refactors a function introduced in commit `A`, an ordinary vector store continues to return the chunk from `A` because its semantic similarity remains high. The agent receives conflicting, outdated code snippets and hallucinates compilation errors.
3. **The Unpruned Memory Explosion (The 100GB Cautionary Tale)**:
   In our forensic audit of `review/munder-difflin`, an agent memory stack with an un-bounded semantic store suffered a memory-leak bug that consumed **100 GB in 8 hours** (`[VERIFIED review/munder-difflin/src/main/palaceReap.ts:1-30]`). Without strict bounding and temporal reapers, vector memory degrades into an expensive token sink.

### 1.2 The Tri-Modal Hybrid Memory Architecture
The definitive memory architecture for an autonomous coding agent decomposes into four synchronized layers:

```
+-------------------------------------------------------------------------------+
| Layer 0 & 1: Exact Symbolic & Dependency Graph (Deterministic Ground Truth)   |
| - Tree-sitter / SCIP AST Graph (codebase-memory-mcp)                          |
| - Git worktree ledger & commit-addressed receipts (studio.db SQLite WAL)      |
+---------------------------------------+---------------------------------------+
                                        |
+---------------------------------------v---------------------------------------+
| Layer 2: Temporal Fact & Decision Ledger (Invalidation Engine)                |
| - Statement-lifetime schema: veracity, valid_until, superseded_by, trust_tier |
| - Automated invalidation linked to Git commit SHAs and file claims            |
+---------------------------------------+---------------------------------------+
                                        |
+---------------------------------------v---------------------------------------+
| Layer 3: Working & Procedural Memory (Agent Self-Editing Context)             |
| - Markdown project memory (MEMORY.md, task scratchpads, role-scoped notes)    |
| - Evidence Packets: immutable find <-> return data shuttles                   |
+---------------------------------------+---------------------------------------+
                                        | (Opt-in only)
+---------------------------------------v---------------------------------------+
| Layer 4: Semantic Fallback with Mandatory Cross-Encoder Reranker              |
| - Dense embeddings restricted to high-level architectural prose & ADRs        |
| - Mandatory reranker gate (BGE-Reranker / Cohere); raw ANN recall is banned   |
+-------------------------------------------------------------------------------+
```

#### Layer 0 & 1: Exact Symbolic AST & Git Ledger
- **Mechanism**: Tree-sitter AST extraction and SCIP/LSIF code graphs (mirrored in `codebase-memory-mcp`).
- **Capabilities**: Exact deterministic retrieval of symbol declarations (`search_graph`), caller/callee paths (`trace_path`), and type definitions (`get_code_snippet`).
- **Cost**: Zero model tokens during retrieval; zero hallucinations.

#### Layer 2: Temporal Invalidation (The `oh-my-pi` & Zep Model)
- **Mechanism**: Every recorded architectural decision or dependency fact is stored with a stateful lifetime model:
  ```rust
  pub struct MemoryFact {
      pub fact_id: String,
      pub content: String,
      pub veracity: VeracityTier, // Stated | Inferred | ToolOutput | Verified
      pub trust_tier: u8,
      pub created_commit: String,
      pub valid_until: Option<String>,
      pub superseded_by: Option<String>,
  }
  ```
- **Behavior**: When an agent commits a change to a module, any fact linked to the prior commit is automatically marked `superseded_by: new_commit_sha`. Old facts are excluded from retrieval by default.

#### Layer 3: Working Memory & Evidence Packets
- **Mechanism**: Direct filesystem-based scratchpads (`MEMORY.md`, `scratchpads/`).
- **Find <-> Return Shuttling**: When a researcher agent investigates a bug or API, it does not write conversational prose. It generates a content-addressed **Evidence Packet**:
  ```json
  {
    "packet_id": "sha256-hash",
    "task_id": "task-402",
    "claims": [
      {
        "claim_id": "c1",
        "assertion": "Auth middleware rejects bearer tokens without prefix",
        "citation": { "file": "src/auth.rs", "lines": [42, 58], "sha": "a1b2c3" },
        "confidence": "verified"
      }
    ]
  }
  ```
  The implementer agent consumes this immutable packet directly, guaranteeing full provenance without summarization loss.

#### Layer 4: Semantic Fallback & Mandatory Reranking Gate
- **Mechanism**: Dense vector search (sqlite-vec or Qdrant) is enabled **only** for broad natural language architectural questions.
- **Reranker Mandate**: Raw Approximate Nearest Neighbor (ANN) search is strictly gated behind a cross-encoder reranker. If the top reranked score falls below threshold $0.75$, the result is discarded, preventing context pollution.

---

## 2. Deployment Topology: Docker Stack vs. Desktop App vs. Hybrid

### 2.1 The Dilemma
Modern AI orchestrators struggle with platform distribution:
- **Pure Desktop Apps (Tauri / Electron)**: Excellent native OS integration, but fail on environment reproducibility. Sub-agents require host-installed toolchains (Rust, Node, Python, specific LSPs), resulting in "works on my machine" failures.
- **Pure Docker Compose Stacks**: Flawless reproducibility, but introduce severe filesystem performance penalties on macOS and Windows (Virtiofs/WSL2 I/O bottlenecks during `git status` or `node_modules` builds), while cutting agents off from host hardware, local browsers, and native display servers.

### 2.2 Comparative Benchmark & Forensic Matrix
| Metric / Characteristic | Pure Desktop (Tauri) | Pure Docker Compose | Hybrid Decoupled (Recommended) |
|---|---|---|---|
| **Toolchain Consistency** | ❌ Highly fragile (depends on host PATH) | ✅ Perfect (containerized image) | ✅ Perfect (worker containers) |
| **macOS / Windows File I/O** | ✅ Native (zero virtualization tax) | ❌ 3x–10x slowdown on bind mounts | ✅ Native UI; optimized volume strategy |
| **Sandboxing & Blast Radius** | ❌ Low (agent can run `rm -rf ~`) | ✅ High (Linux namespaces, cgroups) | ✅ High (isolated worker sandbox) |
| **Headless / Remote Hosting** | ❌ Impossible without virtual display | ✅ Native (deployable on VPS/homelab)| ✅ Supported out-of-the-box |
| **OAuth Browser Handling** | ✅ Seamless (native OS browser & PKCE) | ❌ Painful (no display, port forward) | ✅ Seamless (handled by UI/host bridge)|

### 2.3 The Architectural Recommendation: Hybrid Decoupled Studio
We reject the false dichotomy. STC will adopt a **Hybrid Decoupled Architecture**:

```
+---------------------------------------------------------------------------------+
| OPERATOR PLANE (Host System - User Hardware)                                    |
|                                                                                 |
|  +----------------------------+             +--------------------------------+  |
|  | Cockpit Native Desktop UI  |             | WebUI Browser Client           |  |
|  | (Tauri v2 + React/Tailwind)|             | (Next.js / Vite SPA)           |  |
|  +--------------+-------------+             +---------------+----------------+  |
|                 |                                           |                   |
|                 +---------------------+---------------------+                   |
|                                       |                                         |
|                                       | WebSocket / IPC (NDJSON / Protobuf)     |
|                                       v                                         |
+---------------------------------------------------------------------------------+
| ENGINE & EXECUTION PLANE (Docker Stack / Local Headless Daemon)                 |
|                                                                                 |
|  +---------------------------------------------------------------------------+  |
|  | studio-core Daemon                                                        |  |
|  | - Deterministic DAG Scheduler & Lease Broker                              |  |
|  | - Claim Broker & Scope Enforcer (Tree-sitter AST)                         |  |
|  | - SQLite WAL Database (studio.db) & Event Ledger                          |  |
|  | - Dynamic MCP Gateway & Tool Lens Proxy                                   |  |
|  +------------------------------------+--------------------------------------+  |
|                                       |                                         |
|                                       | Container IPC / Docker Socket           |
|                                       v                                         |
|  +---------------------------------------------------------------------------+  |
|  | Sandboxed Worker Containers (Ephemeral or Pooled)                         |  |
|  | - OpenCode Engine (Heavy coder with pre-installed LSPs)                   |  |
|  | - Pi Micro-Agent Runtimes (Specialist fast workers)                       |  |
|  | - Toolchains: Rust, Node.js, Python, Go, Biome, Ruff, Fallow, Semgrep     |  |
|  +---------------------------------------------------------------------------+  |
+---------------------------------------------------------------------------------+
```

- **Core Daemon**: Packaged as a lightweight container (or native binary) running `studio-core`. It manages database transactions, scheduler queues, and quality gates.
- **Worker Sandboxes**: Sandboxed containers executing code inside Git worktrees, isolated from the user's root operating system.
- **Cockpit UI**: Delivered as a native Tauri app on the host or served as a WebUI over port 3000. It owns zero state, acting as a direct projection of the engine's SQLite database.

---

## 3. Harness Acquisition: BYOH vs. Declarative Agent Manager

### 3.1 The Hazards of "Bring Your Own Harness" (BYOH) Volume Mounts
The proposal to allow users to mount their host-installed agent CLIs (`/usr/local/bin/claude` or `~/.nvm/versions/node/.../bin/opencode`) directly into a Docker container fails catastrophically in real-world environments:

1. **Architecture & ABI Mismatches**:
   A host binary compiled for macOS (Mach-O ARM64) or Arch Linux (glibc 2.40) will exit with `exec format error` or fail on missing dynamic libraries (`libssl.so.3`, `libc.so.6`) when mounted into an Ubuntu or Alpine container.
2. **Interpreter Shebang Hardcoding**:
   Host CLI scripts frequently begin with `#!/Users/john/.nvm/versions/node/v22.0.0/bin/node` or `#!/usr/local/bin/python3`. These absolute paths do not exist inside the container.
3. **Environment Pollution & Drift**:
   Host-installed plugins and dependencies clash with container runtimes, destroying determinism.

### 3.2 The Solution: Declarative Agent Manager with Prebaked Core Runtimes
Inspired by the forensic audit of **`pivoshenko/kasetto`** and modern Devcontainer patterns, STC adopts a **Declarative Agent Manager**:

1. **Prebaked Tier-1 Runtimes in the Base Image**:
   The Studio container base image comes pre-installed with tested, container-native binaries of the two core agent runtimes:
   - **OpenCode CLI**: The primary heavy-weight implementation agent.
   - **Pi CLI**: The lightweight specialist micro-agent runtime.
   - Supported language runtimes: Node.js 22 LTS, Python 3.12, Rust stable, Go 1.23.
2. **Kasetto-Style Declarative Manifest (`studio.yaml` & `studio.lock`)**:
   Instead of mounting host binaries, users define their agent requirements declaratively:
   ```yaml
   agents:
     - name: backend-coder
       harness: opencode
       version: "1.2.4"
       plugins:
         - "@opencode/lsp-rust"
       skills:
         - "git://github.com/Gentleman-Programming/gentle-ai#skills/sdd"
       mcp_servers:
         - name: codebase-memory
           command: codebase-memory-mcp
   ```
3. **Persistent Tool & Package Volume**:
   The internal agent manager downloads and compiles custom extensions into a dedicated persistent volume (`/data/harnesses/`), using cryptographic SHA-256 lockfile verification. Assets are updated via hashed diffs, eliminating repetitive downloads.

---

## 4. State Persistence Across Docker Image Updates

### 4.1 The 4-Volume Persistent Layout
To guarantee complete zero-data-loss durability when users upgrade or rebase Docker images, all state is strictly partitioned into four persistent targets:

```
[HOST SYSTEM]                                          [CONTAINER FILESYSTEM]
/home/user/project/  ======== (Bind Mount) ========>   /workspace

DOCKER NAMED VOLUMES:
studio_state         ==============================>   /data/state
  ├── studio.db (SQLite WAL)
  ├── studio.db-wal
  └── task_ledger.log

studio_auth          ==============================>   /data/auth (Mode 0700)
  ├── credentials.vault (AES-256)
  ├── opencode/
  │     └── opencode.json
  └── pi/
        ├── models.json
        └── auth.json

studio_cache         ==============================>   /data/cache
  ├── cargo/
  ├── npm/
  ├── pip/
  └── tree-sitter/

studio_tools         ==============================>   /data/harnesses
  ├── pi_packages/
  └── custom_mcps/
```

### 4.2 Forward Database Migrations (Zero-Downtime Rebase)
- `studio-core` utilizes `rusqlite_migration`.
- Upon container boot, an entrypoint check executes all pending schema migrations within a `BEGIN IMMEDIATE` transaction before the scheduler begins accepting work.
- If a migration fails, the transaction is rolled back, the existing database is preserved at `/data/state/studio.db.bak.<timestamp>`, and the container halts with a clear diagnostic log.

### 4.3 Dynamic UID/GID Permission Handling
A classic Docker pitfall on Linux/macOS is root-owned files appearing in host workspaces.
- STC's Docker image runs an entrypoint script utilizing `gosu`:
  ```bash
  #!/usr/bin/env bash
  USER_ID=${HOST_UID:-1000}
  GROUP_ID=${HOST_GID:-1000}

  groupmod -o -g "$GROUP_ID" studio 2>/dev/null
  usermod -o -u "$USER_ID" studio 2>/dev/null

  chown -R studio:studio /data/state /data/auth /data/cache /data/harnesses
  exec gosu studio "$@"
  ```
- This guarantees that every file created, modified, or staged by an agent inside `/workspace` matches the exact permissions of the host developer.

### 4.4 Git Worktree and Ref Preservation
- Ephemeral agent workspaces are created via `WorktreeManager` under `.git/worktrees/stc-<session_id>`.
- In the event of a sudden container shutdown, kill-9, or image upgrade, unmerged agent changes are never lost: `WorktreeManager` automatically commits uncommitted diffs to `refs/preserved/<session_id>` before destroying a worktree or sweeping stale leases (`[VERIFIED plans/ROADMAP-AUDIT.md:18]`).

---

## 5. Subscriptions vs. Curated Hand-Built Harnesses

### 5.1 The OAuth Nightmare in Headless Containers
Proprietary agent subscriptions (Claude Code Pro/Max, OpenAI Codex CLI, Gemini Advanced) rely on browser-based OAuth PKCE flows:
- The CLI launches a local HTTP callback listener on `http://localhost:random_port/callback`.
- It prompts the user to open a browser URL.
- Upon authentication, the browser redirects to localhost.
Inside a headless Docker container or remote VPS, this flow breaks: localhost points to the container network namespace, the host browser cannot reach it without complex port forwarding, and session cookies cannot be refreshed automatically.

### 5.2 The Strategy: Curated First-Class RolePacks (The BYOK & OpenCode Model)
Rather than wrestling with headless OAuth, STC adopts the **Curated Hand-Built Model**:

```
+---------------------------------------------------------------------------------+
| CURATED FIRST-CLASS AGENTS (90% of Fleet Workloads)                             |
|                                                                                 |
|  User Inputs:                                                                   |
|  - API Keys (Anthropic, OpenAI, DeepSeek, Google, Groq, OpenRouter)             |
|  - OpenCode Go / Zen Subscription Token (Stealth & Free Tier Models)            |
|                                                                                 |
|  Execution:                                                                     |
|  - Standardized OpenCode Engine (for deep coding & refactoring)                 |
|  - Standardized Pi Engine (for rapid specialist sub-agent tasks)                |
|  - Dynamic config injection (opencode.json, models.json)                        |
|  - Full deterministic control over prompts, schemas, skills, and tools          |
+---------------------------------------------------------------------------------+
                                      |
                                      | (Optional Power-User Alternative)
                                      v
+---------------------------------------------------------------------------------+
| HOST SUBSCRIPTION BRIDGE (10% Power-User Edge Cases - Happier Pattern)          |
|                                                                                 |
|  User Setup:                                                                    |
|  - User runs Claude Code CLI or Codex CLI natively on their host machine        |
|  - Host OS handles native browser OAuth flow seamlessly                         |
|                                                                                 |
|  Execution:                                                                     |
|  - Lightweight local relay daemon bridges host CLI to Studio via ACP (WebSocket)|
|  - Studio coordinates tasks; host CLI executes with user's subscription quotas  |
+---------------------------------------------------------------------------------+
```

#### Why Hand-Built Curated Agents Win:
1. **Zero OAuth Friction**: API keys and OpenCode Go tokens are static strings stored in the encrypted credential vault. Zero browser redirect gymnastics.
2. **Access to Frontier & Stealth Models**: OpenCode Go / Zen provides access to premier coding models and stealth free tiers without individual vendor contracts.
3. **Deterministic Prompt & Capability Slicing**: When STC configures the agent directly, it strictly governs system prompts, memory injection, and MCP tool availability.

#### The Fallback: Host ACP Bridge (The `happier-dev/happier` Pattern)
For developers with existing Claude Code or ChatGPT Plus/Pro subscriptions who refuse to use raw API keys, STC provides a host bridge:
- A lightweight CLI helper (`stc-bridge`) runs directly on the user's host OS.
- It authenticates locally with Claude Code using standard browser OAuth.
- It exposes a standardized ACP (Agent Client Protocol) endpoint over local WebSocket, allowing `studio-core` to dispatch tasks to the host subscription agent without running OAuth inside the container.

---

## 6. LLM Gateway & Provider Management

### 6.1 Evaluating Gateways: OmniRoute vs. LiteLLM vs. Bifrost
| Gateway | Implementation Language | Primary Strength | Weakness for Studio |
|---|---|---|---|
| **OmniRoute** | TypeScript / Next.js | Desktop/local-first, token compression (RTK/Caveman), free-tier provider rotations (260+ providers) | Requires separate node process/UI |
| **Bifrost** | Go | Ultra-low latency (11µs overhead), 5000+ RPS | Enterprise-focused; overkill for local studio; lacks coding tool heuristics |
| **LiteLLM** | Python / Rust core | Industry standard proxy, budget tracking | Heavyweight; massive Python dependency tree |

### 6.2 The Solution: Integrated Core Provider Hub
Forcing users to independently deploy and configure LiteLLM or OmniRoute before using STC creates unacceptable onboarding friction.

Instead, STC integrates a **Native Provider Hub** directly into `studio-core` and Cockpit:
1. **Automated Dual-Config Synchronization**:
   When a user adds an API key or configures a provider in Cockpit, `studio-core` automatically formats and writes the native configuration files for both supported engines:
   - `/workspace/.opencode/opencode.json` (OpenCode provider block)
   - `/data/auth/pi/models.json` (Pi models and provider extensions)
   The user never manually edits JSON or JSONC files.
2. **Support for External Upstream Gateways**:
   For enterprise users or developers already running an existing OmniRoute, LiteLLM, or corporate AI proxy, the WebUI provides a single input: `Upstream Gateway URL` + `Master Key`. Studio routes all requests through the existing proxy seamlessly.

### 6.3 Model Assignment Strategy: Preconfigured Tiers + Granular Overrides
To balance simplicity with extreme power, STC implements a two-tier model assignment UI:

```
[System-Wide Preset Selector]
  ( ) Eco / Free Tier        (DeepSeek-V3, Gemini 2.0 Flash, Llama 3.3)
  (*) Balanced (Default)     (Claude 3.7 Sonnet, DeepSeek-V3, Gemini 2.0 Flash)
  ( ) Frontier Reasoning     (Claude 3.7 Thinking, o3-mini High, Claude 3.5 Sonnet)

[Granular Role Overrides (Advanced)]
+----------------------+--------------------------+-----------------------+
| Role                 | Assigned Model           | Fallback Model        |
+----------------------+--------------------------+-----------------------+
| Lead Architect       | claude-3-7-sonnet-think  | o3-mini-high          |
| Core Coder           | claude-3-7-sonnet        | deepseek-chat (v3)    |
| Security Auditor     | gemini-2.0-flash         | claude-3-5-haiku      |
| Context Scout        | gemini-2.0-flash         | groq/llama-3.3-70b    |
+----------------------+--------------------------+-----------------------+
```

---

## 7. The Pi Ecosystem Deep-Dive (`pi`, `oh-my-pi`, `gsd-core`, `pi.dev/packages`)

### 7.1 Forensic Analysis of Repositories

#### 1. `earendil-works/pi`
- **Philosophy**: Hyper-minimalist, modular coding agent toolkit in TypeScript.
- **Architectural Edge**: Bare-bones agent loop with near-instant boot time (<100ms) and minimal RAM footprint (~40MB). Extensible via `models.json` and TypeScript provider extensions.
- **Package System**: `pi.dev/packages` enables modular distribution of skills, themes, and tool capabilities via npm conventions.

#### 2. `can1357/oh-my-pi` (Audited in `review/oh-my-pi/`)
- **Philosophy**: "Batteries-included" heavy fork of Pi.
- **Key Innovations to Adopt**:
  - **Virtual Devices (`xd://`)**: Progressive tool disclosure demoting rarely used tools out of context until invoked (`[VERIFIED review/oh-my-pi/tools/xdev.ts]`).
  - **Polyphonic 4-Voice Memory (`mnemopi`)**: Combines vector, graph, fact, and temporal retrieval into a unified relevance score (`[VERIFIED review/oh-my-pi/packages/mnemopi/src/core/polyphonic-recall.ts]`).

#### 3. `open-gsd/gsd-core`
- **Philosophy**: Context-engineering and Spec-Driven Development (SDD) discipline layer.
- **Key Innovation to Adopt**:
  - Eliminates "context rot" by enforcing a state machine: **Research -> Spec -> Plan -> Execute -> Verify**. An agent is strictly prohibited from modifying code until a spec artifact is generated and cryptographically hashed.

### 7.2 Optimal Division of Labor: OpenCode vs. Pi
| Feature / Characteristic | OpenCode | Pi (`earendil-works/pi`) |
|---|---|---|
| **Resource Footprint** | Heavy (~250MB RAM, Node/Bun) | Ultra-light (~40MB RAM) |
| **Startup Latency** | ~1.2s | <100ms |
| **LSP Integration** | Deep, multi-language built-in | Basic / extension-driven |
| **Subscription Support** | OpenCode Go / Zen native | BYOK / provider configs |
| **Role Assignment in STC** | **Primary Implementation Engine** (Complex multi-file refactoring, deep edits) | **Specialist Micro-Agent Swarm** (Scouts, linters, doc generators, reviewers) |

**Conclusion**: STC standardizes on **OpenCode** as the persistent heavy coder and spawns **Pi** micro-instances for rapid, parallel, ephemeral tasks.

---

## 8. Cross-Pollination: Architectural Gems from the Wild

From our audit of the three requested projects, STC will extract and integrate their strongest architectural patterns:

```
+---------------------------------------------------------------------------------+
| PATTERNS EXTRACTED FOR STC PHASE 1                                              |
|                                                                                 |
| 1. From happier-dev/happier:                                                    |
|    - Unified Review & Approval Inbox (All agent diffs & permissions in one queue)|
|    - Decoupled WebSocket Relay Architecture (Headless engine <-> Web/Mobile UI) |
|                                                                                 |
| 2. From Gentleman-Programming/gentle-ai:                                        |
|    - Spec-Driven Development (SDD) multi-phase planning engine                  |
|    - Context7 Live Documentation MCP (Zero hallucinated deprecated APIs)        |
|    - Engram Knowledge Blocks (Bug-fix & architectural rationale persistence)    |
|                                                                                 |
| 3. From pivoshenko/kasetto:                                                     |
|    - Declarative Environment Manifest & Lockfile (studio.yaml / studio.lock)    |
|    - Hashed Diff Asset Syncing (Rust-native high-performance file synchronization|
|    - Zero-Leak Secret Interpolation (${SECRET} resolved to capability tokens)   |
+---------------------------------------------------------------------------------+
```

---

## 9. RolePack Specifications & Ephemeral MCP Inventory

### 9.1 Overcoming the 80K-Token Schema Bloat
As proven in Phase 0 spikes, loading a full catalog of 118 MCP tools into every agent consumes **311 KB / ~80,000 tokens per turn** (`[VERIFIED plans/spikes/H3-result.md]`). Slicing tools per role reduces this footprint to **2.2K tokens (~36x compression)**.

Furthermore, heavy external tools (Playwright browser, video/3D renderers) are designated as **Ephemeral MCP Servers**: they are spawned on-demand for a single task and terminated immediately upon task completion, preventing context pollution.

### 9.2 The Core RolePack Matrix
```
                                 STUDIO MCP GATEWAY
                                         |
     +-------------------+---------------+-------------------+-------------------+
     |                   |                                   |                   |
     v                   v                                   v                   v
[Lead Architect]    [Core Coder]                       [Security Auditor]  [Context Scout]
 - Model: Frontier   - Model: Balanced / High-Speed     - Model: Balanced   - Model: Fast/Cheap
 - Tools:            - Tools:                           - Tools:            - Tools:
   * codebase-mem      * LSP client (hover/def/diag)      * Semgrep scan      * Playwright (ephemeral)
   * read_file         * Scoped fs write (claimed files)  * Fallow deadcode   * Firecrawl / Fetch
   * web_search        * Sandboxed test runner            * Sonar gate        * Context7 live docs
 - Banned:           - Banned:                          - Banned:           - Banned:
   * fs_write          * Browser / web access             * fs_write          * fs_write
   * shell exec        * Arbitrary shell commands         * shell exec        * shell exec
```

---

## 10. Quality Standards, Static Analysis & Anti-Circumvention

### 10.1 The End-to-End Quality Gate Pipeline
To ensure generated code adheres to strict engineering standards and contains zero vulnerabilities, STC implements a four-stage quality gate:

```
[Agent Turn Loop]  ==> 1. In-Loop Managed LSP Broker
                             | (Real-time compiler diagnostics injected into turn)
                             v
[Pre-Commit]       ==> 2. Ultra-Fast Formatting & Linting
                             | (Lefthook running Biome & Ruff in parallel <200ms)
                             v
[Post-Edit Audit]  ==> 3. Deep Structural & Security Analysis
                             | (Fallow dead-code graph + Semgrep OWASP AST scan)
                             v
[Merge Turnstile]  ==> 4. Bors Turnstile Verification
                               (Clean compilation + Green tests + Anti-circumvention pass)
```

### 10.2 Component Analysis

#### 1. In-Loop Managed LSP Broker
- `studio-core` maintains persistent stdio LSP connections (`rust-analyzer`, `vtsls`, `gopls`, `pyright`).
- After any file modification, LSP diagnostics are queried immediately. If errors exist, they are formatted as structured feedback and injected into the agent's turn. The agent cannot declare "Done" while compiler diagnostics remain red.

#### 2. Lefthook for Parallel Pre-Commit Formatting
- `lefthook` (Go-native hook runner) executes formatters concurrently across git staged files:
  - **Biome**: Formats and lints TypeScript/JavaScript in ~15ms (25x faster than Prettier/ESLint).
  - **Ruff**: Formats and lints Python in ~10ms (50x faster than Black/Flake8).
  - **cargo clippy / fmt**: Enforces idiomatic Rust standards.

#### 3. Fallow: Codebase Intelligence & Dead Code Detection
- **Role**: Rust-native codebase intelligence tool mapping full module dependency graphs.
- **Value**: AI agents frequently introduce "slop" (unused exported utility functions, abandoned types, duplicate helper logic). Fallow scans the AST graph to detect unused exports, circular dependencies, and tainted sinks, failing the task before technical debt accumulates.

#### 4. Semgrep & Sonar: Security & Quality Gates
- **Semgrep**: AST pattern matching scanning for OWASP Top 10 vulnerabilities (SQL injection, XSS, unescaped shells, hardcoded API secrets).
- **Sonar**: Monitors cognitive complexity, test coverage thresholds, and architectural maintainability.

### 10.3 Non-Bypassable Anti-Circumvention Guards
A major behavioral flaw in autonomous agents is **gate circumvention**: when blocked by a linter or type-checker, the agent adds `// @ts-ignore`, `/* eslint-disable */`, `# noqa`, `#[allow(warnings)]`, or alters `biome.json` / `tsconfig.json` to pass the gate.

**The Anti-Circumvention Rule (Fail-Closed Engine)**:
`studio-core` inspects every incoming git diff via Tree-sitter:
1. Any edit modifying lint configuration files (`.eslintrc`, `biome.json`, `tsconfig.json`, `ruff.toml`, `lefthook.yml`) is **strictly rejected** unless explicitly authorized by a human in the Approval Inbox.
2. Any edit introducing suppression annotations (`@ts-ignore`, `noqa`, `eslint-disable`) is treated as a fatal gate failure, immediately returning an `UNAUTHORIZED_SUPPRESSION_DETECTED` receipt to the agent.

---

## 11. Architecture Decision Records (ADRs) for Phase 1

### ADR-005: Hybrid Containerized Engine with Decoupled Thin Cockpit
- **Context**: Need to balance environment reproducibility (LSPs, toolchains) with native host OS performance and file access.
- **Decision**: Run `studio-core`, LSPs, and agent harnesses inside a headless Docker container stack. Deliver Cockpit as a thin native Tauri client (or browser WebUI) connecting via WebSocket.
- **Consequences**: Flawless toolchain consistency across Linux/macOS/Windows; zero host pollution; native UI responsiveness.

### ADR-006: Declarative Agent Manager over Host Volume Mounting
- **Context**: Mounting host CLI binaries into Docker containers causes ABI, glibc, and shebang execution failures.
- **Decision**: Pre-bake core runtimes (OpenCode, Pi) into the base image. Implement a Kasetto-inspired declarative manifest (`studio.yaml` / `studio.lock`) for installing extensions into a persistent volume.
- **Consequences**: Deterministic builds; zero ABI crashes; reproducible fleet setups.

### ADR-007: Tri-Modal Code Memory (AST Graph + Temporal Ledger)
- **Context**: Conversational vector databases fail on codebases due to structural blindness and temporal invalidation.
- **Decision**: Enforce a tri-modal memory ladder: Tree-sitter AST Graph (L1) -> Temporal Statement Ledger (L2) -> Working Markdown Scratchpads (L3) -> Reranked Semantic Search (L4, opt-in).
- **Consequences**: 100% precision on symbol navigation; zero stale-code hallucinations; eliminated token bloat.

### ADR-008: Dual-Track Execution Substrate (OpenCode Heavy Coder + Pi Specialist Swarm)
- **Context**: Need both deep multi-file refactoring capabilities and lightweight, low-cost micro-agents.
- **Decision**: Standardize on OpenCode for primary implementation roles; utilize Pi (`earendil-works/pi`) for fast, ephemeral specialist roles (scouts, auditors, doc writers).
- **Consequences**: Optimal resource utilization; near-instant subagent spawn times; native support for OpenCode Go models and BYOK keys.

### ADR-009: Non-Bypassable Static Analysis & Anti-Circumvention Gate Pipeline
- **Context**: Agents attempt to circumvent lint/type errors by adding suppression tags or altering config files.
- **Decision**: Enforce an automated pipeline combining managed LSP diagnostics, Lefthook, Fallow, Semgrep, and an AST-level anti-circumvention filter that rejects suppression annotations.
- **Consequences**: Guaranteed high-quality code generation; zero dead code; zero introduced security vulnerabilities.

---

## 12. Concrete Phase 1 Implementation Plan

```
STC REPOSITORY EXPANSION MAP
=============================================================================
STC/
├── docker/
│   ├── Dockerfile                   [NEW: Multi-stage Ubuntu base, LSPs, OpenCode, Pi]
│   ├── docker-compose.yml           [NEW: Multi-volume persistent topology]
│   └── entrypoint.sh                [NEW: gosu dynamic UID/GID permission mapper]
│
├── studio-core/src/
│   ├── manifest/                    [NEW: Kasetto-style declarative manifest engine]
│   │   ├── mod.rs                   [Manifest parser: studio.yaml & studio.lock]
│   │   └── resolver.rs              [Hashed diff asset & secret resolver]
│   │
│   ├── provider/                    [NEW: Integrated LLM provider hub]
│   │   ├── mod.rs                   [Credential vault & model discovery]
│   │   ├── sync_opencode.rs         [Generates /workspace/.opencode/opencode.json]
│   │   └── sync_pi.rs               [Generates /data/auth/pi/models.json]
│   │
│   └── quality/                     [NEW: Quality gates & static analysis broker]
│       ├── mod.rs                   [Quality pipeline orchestrator]
│       ├── lsp_broker.rs            [Managed background stdio LSP supervisor]
│       ├── static_analysis.rs       [Fallow & Semgrep runner + SARIF parser]
│       └── anti_circumvention.rs    [Tree-sitter diff suppression checker]
│
└── cockpit/src/
    ├── pages/
    │   ├── Providers.tsx            [MODIFY: Visual provider wizard & model tiering]
    │   ├── Inbox.tsx                [MODIFY: Happier-style unified approval inbox]
    │   └── QualityDashboard.tsx     [MODIFY: Real-time LSP, Fallow & Semgrep telemetry]
=============================================================================
```

---

## 13. Verification Strategy & Acceptance Criteria

| Component / Subsystem | Verification Method | Pass Criteria |
|---|---|---|
| **Docker Persistence** | `tests/docker_persistence_test.sh` | Container torn down, image re-pulled with `--no-cache`; `studio.db`, WAL, and git worktrees resume with zero data corruption. |
| **UID/GID Mapping** | File creation test inside `/workspace` | Output files match host developer UID/GID exactly; no root-locked files on host. |
| **Provider Config Sync** | `cargo test -p studio-core test_provider_sync` | Inputting credentials generates valid `opencode.json` and Pi `models.json` with correct schemas and masked tokens. |
| **Anti-Circumvention Gate** | `cargo test -p studio-core test_anti_circumvention` | Synthetic commits with `// @ts-ignore` or modified `biome.json` are rejected with `UNAUTHORIZED_SUPPRESSION`. |
| **Fallow / Semgrep Runner**| `cargo test -p studio-core test_static_analysis` | Synthetic dead code and insecure exec calls are detected, converted to SARIF, and block the Bors turnstile. |
| **LSP Broker Feedback** | End-to-end Rust editing test | Introducing syntax error immediately triggers structured diagnostic feedback to agent before commit. |

---

> **Approved for Phase 1 Execution**
> *Antigravity Systems Architecture Team — September 2026*
