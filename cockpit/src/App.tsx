// Cockpit shell (P04, MS-1): browser-first, NO Tauri.
// One real view (Status) + A1 read-only Ask queue, both read projections of
// studio.db served by studio-web. Server owns projection; browser reads only
// (GET /api/*). Zero mock data: every row below arrived over /api.
import { useState } from "react";
import type { CSSProperties } from "react";
import type { HealthDto } from "./api-types";
import StalenessBadge from "./components/StalenessBadge";
import { useProjection } from "./hooks/useProjection";
import StatusView from "./views/StatusView";
import AskView from "./views/AskView";

type Tab = "status" | "ask";

function TabButton({
  label,
  active,
  onSelect,
}: {
  label: string;
  active: boolean;
  onSelect: () => void;
}) {
  const style: CSSProperties = {
    background: active ? "var(--surface-2)" : "var(--surface-0)",
    color: active ? "var(--text-0)" : "var(--text-dim)",
    border: "1px solid var(--border)",
    borderRadius: 6,
    padding: "6px 12px",
    fontFamily: "var(--mono)",
    fontSize: 12,
    cursor: "pointer",
  };
  return (
    <button style={style} onClick={onSelect} aria-pressed={active}>
      {label}
    </button>
  );
}

function DaemonBanner({ health }: { health: HealthDto | null }) {
  if (health?.daemon.reachable) return null;
  const detail = health
    ? health.daemon.detail
    : "daemon status unknown — /api/health unreachable";
  return (
    <p className="error" role="alert" data-testid="daemon-banner">
      {detail}
    </p>
  );
}

function Connecting({ error }: { error: string | null }) {
  if (error) {
    return (
      <p className="error" role="alert">
        projection unavailable ({error}) — is studio-web running against an
        initialized studio.db?
      </p>
    );
  }
  return <div className="loading">connecting to studio-web…</div>;
}

function PollError({ error }: { error: string | null }) {
  if (!error) return null;
  return (
    <p className="error" role="alert">
      {error}
    </p>
  );
}

function ConnectedShell({
  status,
  ask,
  health,
  gap,
  error,
  resync,
}: {
  status: NonNullable<ReturnType<typeof useProjection>["status"]>;
  ask: NonNullable<ReturnType<typeof useProjection>["ask"]>;
  health: ReturnType<typeof useProjection>["health"];
  gap: ReturnType<typeof useProjection>["gap"];
  error: string | null;
  resync: () => void;
}) {
  const [tab, setTab] = useState<Tab>("status");
  const body =
    tab === "ask" ? (
      <AskView feed={ask} />
    ) : (
      <StatusView status={status} gap={gap} onResync={resync} />
    );
  return (
    <div className="cockpit">
      <header>
        <h1>STUDIO</h1> <StalenessBadge value={status.staleness} />
        <nav style={{ display: "flex", gap: 8, margin: "8px 0" }}>
          <TabButton label="Status" active={tab === "status"} onSelect={() => setTab("status")} />
          <TabButton label="Ask" active={tab === "ask"} onSelect={() => setTab("ask")} />
        </nav>
        <DaemonBanner health={health} />
        <PollError error={error} />
      </header>
      {body}
    </div>
  );
}

export default function App() {
  const { status, ask, health, gap, error, resync } = useProjection();

  if (!status || !ask) {
    return (
      <div className="cockpit">
        <header>
          <h1>STUDIO</h1>
        </header>
        <Connecting error={error} />
      </div>
    );
  }

  return (
    <ConnectedShell
      status={status}
      ask={ask}
      health={health}
      gap={gap}
      error={error}
      resync={resync}
    />
  );
}
