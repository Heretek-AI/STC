// Cockpit views: read projection of studio.db, polled every 100ms (CDC).
// The UI never owns state — if GUI and studio.db disagree, the GUI is wrong.
import { useEffect, useRef, useState } from "react";
import type { CSSProperties, ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import Providers from "./pages/Providers";

type Snapshot = {
  db_age_ms: number;
  source?: string;
  // Config-derived settings live in their own section (never a DB projection):
  // `source` is `config:env(...)`, not `studio.db`. Legacy top-level fields are
  // kept optional for back-compat with older servers.
  config?: { source: string; runtime_mode: string; autonomy_mode: string };
  runtime_mode?: string;
  autonomy_mode?: string;
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

function StaleBadge({ snap }: { snap: Snapshot }) {
  // Provenance is the projection's own `source`; never a hard-coded label.
  return (
    <span className="stale">
      source: {snap.source ?? "unknown"} · updated {snap.db_age_ms}ms ago
    </span>
  );
}

const LANE_COLORS: Record<string, string> = {
  building: "var(--status-building)",
  validating: "var(--status-validating)",
  "in-review": "var(--status-review)",
  ready: "var(--status-ready)",
};

function laneColor(lane: string): string {
  return LANE_COLORS[lane] ?? "var(--text-dim)";
}

function StatusPill({ lane, status }: { lane: string; status: string }) {
  return (
    <span className="pill" style={{ borderColor: laneColor(lane), color: laneColor(lane) }}>
      {status}
    </span>
  );
}

function Section({
  title,
  count,
  empty,
  children,
}: {
  title: string;
  count: number;
  empty: string;
  children: ReactNode;
}) {
  return (
    <section className="card">
      <h2>
        {title} <span className="count">{count}</span>
      </h2>
      {count === 0 ? <p className="empty">{empty}</p> : children}
    </section>
  );
}

function ErrorBanner({ message }: { message: string }) {
  return (
    <p className="error" role="alert">
      studio.db unreachable ({message}) — showing last snapshot; check the daemon.
    </p>
  );
}

function TabButton({
  label,
  hotkey,
  active,
  onSelect,
}: {
  label: string;
  hotkey: string;
  active: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      style={tabStyle(active)}
      onClick={onSelect}
      aria-pressed={active}
      accessKey={hotkey}
      title={`${label} (Alt+${hotkey})`}
    >
      {label}
    </button>
  );
}

function TabNav({
  tab,
  onSelect,
}: {
  tab: "war-room" | "providers";
  onSelect: (t: "war-room" | "providers") => void;
}) {
  return (
    <nav style={{ display: "flex", gap: 8, margin: "8px 0" }}>
      <TabButton label="War Room" hotkey="1" active={tab === "war-room"} onSelect={() => onSelect("war-room")} />
      <TabButton label="Providers" hotkey="2" active={tab === "providers"} onSelect={() => onSelect("providers")} />
    </nav>
  );
}

function ConnectState({ pollError }: { pollError: string | null }) {
  if (pollError) return <ErrorBanner message={pollError} />;
  return <div className="loading">connecting to studio.db…</div>;
}

function tabStyle(active: boolean): CSSProperties {
  return {
    background: active ? "var(--surface-2)" : "var(--surface-0)",
    color: active ? "var(--text-0)" : "var(--text-dim)",
    border: "1px solid var(--border)",
    borderRadius: 6,
    padding: "6px 12px",
    fontFamily: "var(--mono)",
    fontSize: 12,
    cursor: "pointer",
  };
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
  return snap.config?.runtime_mode ?? snap.runtime_mode ?? "rootless";
}

function autonomyModeOf(snap: Snapshot): string {
  return snap.config?.autonomy_mode ?? snap.autonomy_mode ?? "full";
}

function AutonomyBadge({ mode }: { mode: string }) {
  const key = autonomyStyleKey(mode);
  return (
    <span
      className={key === "advisory" ? "autonomy-advisory" : "autonomy-full"}
      title="source: config (STUDIO_LANE_RUNTIME defaults; project_autonomy lands in phase 02)"
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
      title="source: config (env STUDIO_LANE_RUNTIME) — not a studio.db projection"
      style={RUNTIME_STYLES[key]}
    >
      {runtimeLabel(mode)}
    </span>
  );
}

export default function App() {
  const [snap, setSnap] = useState<Snapshot | null>(null);
  const [pollError, setPollError] = useState<string | null>(null);
  const [tab, setTab] = useState<"war-room" | "providers">("war-room");
  const failures = useRef(0);
  useEffect(() => {
    const t = setInterval(() => {
      pollSnapshot()
        .then((s) => {
          failures.current = 0;
          setPollError(null);
          setSnap(s);
        })
        .catch((e: unknown) => {
          failures.current += 1;
          if (failures.current >= 3) {
            setPollError(e instanceof Error ? e.message : String(e));
          }
        });
    }, 100);
    return () => clearInterval(t);
  }, []);
  if (!snap) return <ConnectState pollError={pollError} />;
  return (
    <div className="cockpit">
      <header>
        <h1>STUDIO</h1> <StaleBadge snap={snap} />
        <RuntimeBadge mode={runtimeModeOf(snap)} />
        <AutonomyBadge mode={autonomyModeOf(snap)} />
        {pollError && <ErrorBanner message={pollError} />}
        <TabNav tab={tab} onSelect={setTab} />
      </header>
      {tab === "providers" ? (
        <Providers />
      ) : (
        <main className="grid">
          <Section title="War Room" count={snap.fleet.length} empty="No tasks yet — dispatch from the manager.">
            {snap.fleet.map((t) => (
              <div className="row" key={t.id}>
                <StatusPill lane={t.lane} status={t.status} />
                <code>{t.id}</code> <span className="dim">{t.kind}</span>
              </div>
            ))}
          </Section>
          <Section title="Agent Stream" count={snap.stream.length} empty="No events yet — activity appears here live.">
            {snap.stream.slice(0, 50).map((e) => (
              <div className="row" key={e.seq}>
                <span className="dim">#{e.seq}</span> <span>{e.kind}</span>{" "}
                <span className="dim">{e.payload}</span>
              </div>
            ))}
          </Section>
          <Section title="Audit & Gatekeeper" count={snap.receipts.length} empty="No receipts — nothing verified yet.">
            {snap.receipts.map((r) => (
              <div className="row" key={r.id}>
                <code>{r.id}</code> <span className="dim">task={r.task_id}</span>{" "}
                <span className="dim">{r.evidence}</span>
              </div>
            ))}
          </Section>
          <Section title="MCP Registry" count={snap.burn.length} empty="No token burn recorded.">
            {snap.burn.map((b) => (
              <div className="row" key={b.session}>
                <code>{b.session}</code>{" "}
                <span className="num">{b.total}</span>
              </div>
            ))}
          </Section>
        </main>
      )}
    </div>
  );
}
