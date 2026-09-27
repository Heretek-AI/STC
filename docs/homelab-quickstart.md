# STC Homelab Quickstart (issue #10)

Target: 24/7 homelab operation (Strix Halo / DGX Spark-class) plus a VPS variant.
Control plane (studio-core daemon, cockpit) runs **native on the host**.
Compose provides lane workers + persistent volumes only.

## Prerequisites

- Docker + Compose v2, 8GB+ RAM, 20GB disk.
- One model credential: API key, OpenCode Go subscription, or Zen balance.
- Linux UID/GID of the invoking user (lanes run as you — no root-owned files).

## Fresh install

```bash
git clone https://github.com/Heretek-AI/STC && cd STC
cargo run -q -p studio-cli -- init --repo .     # writes .env (rootless default)
cargo run -q -p studio-cli -- up                # brings up lane stack
```

`studio up --dev` prints the privileged banner and requires the
`compose.override.dev.yaml` socket mount explicitly. Never use `--dev`
with untrusted prompts or on shared machines. The cockpit header shows a
persistent `DEV-MODE` badge while `STUDIO_LANE_RUNTIME=privileged-dev`.

## Verify hardening

```bash
bash scripts/compose_lane_smoke.sh     # image build + UID/GID + socket guard
bash scripts/compose_volume_drill.sh   # teardown/re-pull, zero data loss
```

Evidence logs land in `/tmp/opencode/stc-*.log`.

## Volumes (what survives updates)

- `studio-state` — studio.db + WAL (engine-owned).
- `studio-evidence` — content-addressed evidence blobs.
- `studio-logs` — engine/scheduler/adapter JSONL.
- `studio-harness` — per-harness config + auth (per-lane subdir).
- `studio-toolchains` — node/python/uv/rustup + model caches.

Repos and worktrees are host-identical bind mounts; the engine owns all
`git worktree add/lock/prune`. Workers never receive `docker.sock`,
`~/.ssh`, `~/.aws`, `~/.gnupg`, or the host keychain.

## VPS variant

Same flow; ensure `HOST_UID/HOST_GID` match the deploy user in `.env`,
open only the cockpit port, and keep `STUDIO_LANE_RUNTIME=rootless`.
Back up with `studio backup` / `studio export` before image updates.
