# STC — Software Team Collective

**The Dev Agency OS for builders outgrowing single agents.** Talk with a team of agents to shape goals, spec, and roadmap — like briefing a real company. Then the agency executes: roles, handoffs, verification gates, delivery pipeline. Local-first, MIT-licensed, built for 24/7 homelab operation.

## 5-minute quickstart

Prerequisites: Docker, a homelab box or VPS (Strix Halo / DGX Spark-class for 24/7), and **one** of: an API key, an OpenCode Go subscription, or an OpenCode Zen balance.

```bash
git clone https://github.com/Heretek-AI/STC && cd STC
cargo run -q -p studio-cli -- init --repo .   # writes .env (rootless default)
studio up                   # brings up the full compose stack
studio logs --follow        # watch the agency work
```

`studio init` refuses `privileged-dev` without typed confirmation and stamps
whether the credential was live-pinged (wizard-written keys are marked
NOT live-pinged — verify them in the cockpit Providers UI, which holds the
live `/models` proof flow). Fallback: `cp .env.example .env` and edit by hand.

Open the cockpit (WebUI beta) at the printed address, or run `studio-tui` for the terminal.

Homelab hardening + 24/7 ops: see `docs/homelab-quickstart.md` (lane image
build, UID/GID smoke test, volume drill, dev-mode guardrails).

## The autonomy dial

New projects start at **full authority**: the LLM manager negotiates scope with you, dispatches into the DAG, and lands through the gates. Nothing needs a tap — the audit log is the safety net.

Prefer supervision? Turn the dial to **advisory**: scope decisions, dispatches, and merges pause for your approval (desktop, TUI, or phone via the approval relay). Per-project setting, remembered per repo.

## How work flows

`scope → DAG → dispatch → review → merge`, driven from the cockpit:

1. **Scope** — converse with the manager: goals, spec, roadmap.
2. **Dispatch** — zero-model scheduler assigns lanes (pi specialists for cost, opencode for heavy sessions, routed by measured $/task).
3. **Review** — evidence receipts (tests, syntax, fallow audit, Semgrep); tap-outs ("done" with no evidence) are blocked and sent back.
4. **Merge** — freeze → one bounded correction → burned acknowledgement → Bors lands. Nothing lands unverified.

## Roles

v1 ships the full catalog (~30 roles: engineers, QA/security, creatives, management, ops) authored once and emitted to pi, opencode, and omp targets. Community roles live in the separate [STC-Marketplace](https://github.com/Heretek-AI/STC-Marketplace) repo — core-team published, PR-reviewed.

## Models

Bring your own keys, or point at **OpenCode Go** (flat-rate infinite-coding tiers) or **Zen** (pay-per-request) as presets. Preconfigured tiers per role (flagship / workhorse / disposable); override anything per role in the cockpit model console.

## Docs

- `AGENTS.md` — working contract for agents (and humans) in this repo. Generated from code sources; never hand-edit derived sections.
- `GOAL.md` — standing operating goals for agents.
- `plans/` — roadmaps, spike receipts, audits.

## Status

v0.1 target: full docker stack + WebUI beta. See `GOAL.md` for operating goals and `plans/` for the roadmap.
