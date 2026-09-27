// Cockpit views: read projection of studio.db, polled every 100ms (CDC).
// The UI never owns state — if GUI and studio.db disagree, the GUI is wrong.
import { useEffect, useState } from "react";
import type { CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
import Providers from "./pages/Providers";

type Snapshot = {
  db_age_ms: number;
  runtime_mode: string;
  autonomy_mode: string;
  fleet: { id: string; kind: string; status: string; lane: string }[];
  stream: { seq: number; kind: string; payload: string }[];
  receipts: { id: string; task_id: string; kind: string; evidence: string }[];
  burn: { session: string; total: number }[];
};

const DB_PATH = "studio.db";

async function pollSnapshot(): Promise<Snapshot> {
  // Desktop shell: Tauri `snapshot` command (canonical). Browser dev: /snapshot HTTP.
  try {
    const raw: string = await invoke("snapshot", { dbPath: DB_PATH });
    return JSON.parse(raw) as Snapshot;
  } catch {
    const res = await fetch("/snapshot");
    return (await res.json()) as Snapshot;
  }
}

function StaleBadge({ ms }: { ms: number }) {
  return <span className="stale">source: studio.db · updated {ms}ms ago</span>;
}

function isDevMode(mode: string): boolean {
  return mode === "privileged-dev";
}

function runtimeLabel(mode: string): string {
  if (isDevMode(mode)) return "DEV-MODE · socket-mounted (root-equivalent)";
  return `runtime: ${mode}`;
}

const RUNTIME_STYLES: Record<string, CSSProperties> = {
  dev: {
    border: "1px solid var(--diff-deleted)",
    color: "var(--diff-deleted)",
    borderRadius: 999,
    padding: "1px 8px",
    fontSize: 11,
    marginLeft: 8,
  },
  rootless: {
    border: "1px solid var(--border)",
    color: "var(--text-dim)",
    borderRadius: 999,
    padding: "1px 8px",
    fontSize: 11,
    marginLeft: 8,
  },
};

function runtimeStyleKey(mode: string): string {
  return isDevMode(mode) ? "dev" : "rootless";
}

function isAdvisoryMode(mode: string): boolean {
  return mode === "advisory";
}

function autonomyLabel(mode: string): string {
  if (isAdvisoryMode(mode)) return "ADVISORY · pauses for approval";
  return "full authority";
}

const AUTONOMY_STYLES: Record<string, CSSProperties> = {
  advisory: {
    border: "1px solid var(--status-building)",
    color: "var(--status-building)",
    borderRadius: 999,
    padding: "1px 8px",
    fontSize: 11,
    marginLeft: 8,
  },
  full: {
    border: "1px solid var(--border)",
    color: "var(--text-dim)",
    borderRadius: 999,
    padding: "1px 8px",
    fontSize: 11,
    marginLeft: 8,
  },
};

function autonomyStyleKey(mode: string): string {
  return isAdvisoryMode(mode) ? "advisory" : "full";
}

function runtimeModeOf(snap: Snapshot): string {
  if (snap.runtime_mode) return snap.runtime_mode;
  return "rootless";
}

function autonomyModeOf(snap: Snapshot): string {
  if (snap.autonomy_mode) return snap.autonomy_mode;
  return "full";
}

function AutonomyBadge({ mode }: { mode: string }) {
  const key = autonomyStyleKey(mode);
  return (
    <span
      className={key === "advisory" ? "autonomy-advisory" : "autonomy-full"}
      title="source: studio.db project_autonomy (per-project dial, default full)"
      style={AUTONOMY_STYLES[key]}
    >
      {autonomyLabel(mode)}
    </span>
  );
}

function RuntimeBadge({ mode }: { mode: string }) {
  const key = runtimeStyleKey(mode);
  return (
    <span
      className={key === "dev" ? "runtime-dev" : "runtime-rootless"}
      title="source: studio.db snapshot.runtime_mode (STUDIO_LANE_RUNTIME)"
      style={RUNTIME_STYLES[key]}
    >
      {runtimeLabel(mode)}
    </span>
  );
}

export default function App() {
  const [snap, setSnap] = useState<Snapshot | null>(null);
  const [tab, setTab] = useState<"war-room" | "providers">("war-room");
  useEffect(() => {
    const t = setInterval(() => pollSnapshot().then(setSnap).catch(() => {}), 100);
    return () => clearInterval(t);
  }, []);
  if (!snap) return <div>connecting to studio.db…</div>;
  const tabStyle = (active: boolean): CSSProperties => ({
    background: active ? "var(--surface-2)" : "var(--surface-0)",
    color: active ? "var(--text-0)" : "var(--text-dim)",
    border: "1px solid var(--border)",
    borderRadius: 6,
    padding: "6px 12px",
    fontFamily: "var(--mono)",
    fontSize: 12,
    cursor: "pointer",
  });
  return (
    <div>
      <header>
        <h1>STUDIO</h1> <StaleBadge ms={snap.db_age_ms} />
        <RuntimeBadge mode={runtimeModeOf(snap)} />
        <AutonomyBadge mode={autonomyModeOf(snap)} />
        <nav style={{ display: "flex", gap: 8, margin: "8px 0" }}>
          <button
            style={tabStyle(tab === "war-room")}
            onClick={() => setTab("war-room")}
            aria-pressed={tab === "war-room"}
          >
            War Room
          </button>
          <button
            style={tabStyle(tab === "providers")}
            onClick={() => setTab("providers")}
            aria-pressed={tab === "providers"}
          >
            Providers
          </button>
        </nav>
      </header>
      {tab === "providers" ? (
        <Providers />
      ) : (
        <>
      <section data-view="war-room">
        <h2>War Room</h2>
        {snap.fleet.map((t) => (
          <div key={t.id}>{t.lane} {t.id} {t.kind} {t.status}</div>
        ))}
      </section>
      <section data-view="agent-stream">
        <h2>Agent Stream</h2>
        {snap.stream.slice(0, 50).map((e) => (
          <div key={e.seq}>#{e.seq} {e.kind} {e.payload}</div>
        ))}
      </section>
      <section data-view="audit">
        <h2>Audit &amp; Gatekeeper</h2>
        {snap.receipts.map((r) => (
          <div key={r.id}>{r.id} task={r.task_id} evidence={r.evidence}</div>
        ))}
      </section>
      <section data-view="registry">
        <h2>MCP Registry</h2>
        {snap.burn.map((b) => (
          <div key={b.session}>{b.session} {b.total}</div>
        ))}
      </section>
        </>
      )}
    </div>
  );
}
