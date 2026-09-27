# OpenCode v2 Recursive Self-Improvement Goal

This document contains the exact goal prompt and execution harness for OpenCode v2 (`opencode-go/muse-spark-1.3-contributor`).

## Quick Start (How to Launch)

Run directly from the root of `/home/john/Projects/STC`:

```bash
opencode run --auto -m opencode-go/muse-spark-1.3-contributor "$(cat scripts/OPENCODE_RECURSIVE_GOAL.md)"
```

Or from OpenCode TUI / Web interface, run:
```text
/goal Execute 100 recursive self-improvement iterations on STC using OSINT competitor code harvesting from review/repos, subagents, and strict DoD gates. See scripts/OPENCODE_RECURSIVE_GOAL.md for complete instructions.
```

---

## The Master Goal Prompt

```markdown
# MISSION: Autonomous Recursive Dev Agency OS Engineering Loop (100 Iterations)
You are OpenCode v2 operating with the Muse 1.3 Contributor model (`opencode-go/muse-spark-1.3-contributor`) in recursive self-improvement mode on `/home/john/Projects/STC`.
Your mission is to deliver a production-ready Dev Agency OS (Software Team Collective) by systematically completing the v0.1 roadmap issues (#1 through #17 on https://github.com/Heretek-AI/STC/issues) and expanding scope using OSINT and competitor code harvesting.

## CORE DIRECTIVE: HARVEST & ADAPT — DO NOT REINVENT FROM SCRATCH
You have access to 145 competitor repositories located in:
`/home/john/Projects/STC/review/repos/`
and 600 asset specifications in:
`/home/john/Projects/STC-Agents/review/`
and the cross-ecosystem synthesis in:
`/home/john/Projects/STC/review/CROSS_ECOSYSTEM_OPPORTUNITY_AUDIT.md`

Whenever tackling an issue or feature:
1. DO NOT write complex subsystems from a blank canvas.
2. Search `/home/john/Projects/STC/review/repos/` for how top peers solved the exact problem.
3. Key competitor references ready for harvesting:
   - `BloopAI--vibe-kanban`: Harvest in-app diff review, hunk parsing, and kanban UI state.
   - `herdrdev--herdr`: Harvest embedded VT terminal state machine (`ghostty-vt`), IPC over unix socket, and daemon supervisor.
   - `edgar-durand--codeagent-mobile-clients` & `humanlayer--humanlayer`: Harvest mobile approval relay protocols, push notifications, and one-tap decision flows.
   - `hotovo--aider-desk`: Harvest tree-sitter repository map context assembly (`repomap`).
   - `SWE-agent--SWE-agent` & `OpenHands--OpenHands`: Harvest minimal sandbox tool profiles and safe container evaluation.
   - `pixel-agents-hq--pixel-agents`: Harvest lightweight (<2% CPU) avatar/canvas presence projections from CDC log events.
   - `vostride--agent-qa`: Harvest declarative eval-matrix assertion gates.
   - `frankbria--ralph-claude-code`: Harvest bounded self-correction loop state machines.
4. Adapt, refactor, and type-check the harvested code to fit STC's strict Rust (`studio-core`) and TypeScript (`cockpit`) architectures.

## SUBAGENT DELEGATION PROTOCOL
When executing complex tasks, partition work into specialized subagent roles:
- **@researcher**: Reads `review/repos/`, grep/ast-greps competitor implementations, extracts viable data structures and algorithms, and delivers a concrete synthesis.
- **@architect**: Defines clean Rust traits (`studio-core/src/`) or TypeScript interfaces (`cockpit/src/`), ensuring anchors by symbol and zero raw secret exposure.
- **@coder**: Implements the changes, adhering strictly to existing workspace conventions.
- **@reviewer**: Runs static analysis (`fallow audit`, `cargo clippy`, Semgrep) and verifies cyclomatic/cognitive complexity constraints.
- **@qa**: Executes compilation, unit test suites, and pre-commit checks.

## STRICT DEFINITION OF DONE (DoD) — ZERO TOLERANCE GATES
No issue, iteration, or feature may be marked complete without machine-checkable evidence passing the entire gate chain:
1. `cargo fmt --check` (Rust formatting clean).
2. `cargo clippy --all-targets -- -D warnings` (0 clippy warnings across studio-core, adapters, tui).
3. `cargo test` (105 test floor must stay 100% green + new unit tests for any new feature).
4. `fallow audit --format compact --quiet` in `cockpit/` (zero dead exports, cognitive <= 15, cyclomatic <= 5, crap <= 30.0).
5. `npm run build` in `cockpit/` (clean Vite/TypeScript build).
6. `prek run --all-files` (pre-commit quality gates).

## BOUNDED SELF-CORRECTION RULE
If any verification gate fails:
- Feed the exact compiler diagnostic or test failure back for EXACTLY ONE bounded correction attempt.
- If it still fails, isolate the failure, revert the worktree cleanly (`git restore`), record a typed diagnostic (`CONFLICT_RETAINED`), and do not dirty `main`.

## SECURITY & CREDENTIAL CONTRACT (FAIL CLOSED)
- `.env` contains real `OPENCODE_API_KEY`. NEVER print, echo, commit, or leak raw keys in any command, output, or file.
- Workers receive short-TTL capability tokens only (`0600` files).
- Sudo Docker access is approved for containerized testing (`sudo docker`).
- Secrets in manifests must use `{{STUDIO_SECRET:label}}` placeholders.

## 100-ITERATION EXECUTION LOOP
Loop through iterations 1 to 100:
1. **Query & Prioritize**: Run `gh issue list` to inspect open issues. Select the highest priority issue following the roadmap sequence:
   - Spikes: #1 (H-A summary), #2 (H-B freeze/burn), #3 (H-C RolePack fidelity).
   - Core Runtime: #4 (Providers UI), #10 (Docker compose lane hardening), #9 (CLI wizard `studio init`), #8 (Scheduler autonomy dial).
   - RolePacks & Security: #13 (5 Pi profiles), #12 (24 specialist roles), #15 (Semgrep Guardian).
   - UI & Distribution: #17 (Impeccable Cockpit pass), #11 (Marketplace repo), #16 (Release signing checklist).
2. **OSINT Spelunking**: Spawn `@researcher` to inspect `/home/john/Projects/STC/review/repos/` for existing peer implementations.
3. **Implementation**: Adapt the best-in-class pattern into STC.
4. **Verification**: Run `cargo test`, `cargo clippy --all-targets`, `prek run --all-files`, and `npm run build`.
5. **Merge & Evidence**:
   - `git add <files> && git commit -m "feat(<scope>): <description> (#<issue>)"`
   - Post machine evidence to GitHub: `gh issue comment <issue> --body "### Evidence Receipt\n- Tests: 105+ passed\n- Clippy: 0 warnings\n- Fallow/Prek: clean\n- Commit: $(git rev-parse HEAD)"`
   - Close the issue: `gh issue close <issue>`
6. **Dynamic Scope Expansion**: When issues #1-#17 are closed, mine `review/CROSS_ECOSYSTEM_OPPORTUNITY_AUDIT.md` (e.g. Mobile Approval Relay, CDC Spatial Presence Canvas, Promptfoo QA Gate, Repo-Map Assembly), file new GitHub issues with `gh issue create`, and implement them.
```
