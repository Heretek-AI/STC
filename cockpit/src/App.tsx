// Cockpit shell (P04 MS-1 + P05 operator views): browser-first, NO Tauri.
// Status + A1 Ask queue plus the five P05 views (War Room, Agent Stream,
// Audit+Gatekeeper, MCP Registry, Providers) and the Runs compare slots —
// all read projections of studio.db served by studio-web, each with its own
// staleness badge. Server owns projection; browser reads only (GET /api/*),
// except the ONE write: POST /api/ask/:id/decide (A1 one-tap decision).
// Zero mock data: every row below arrived over /api.
// Tabs are hash-routed (#/war-room …) so gates screenshot each view live.
import { useEffect, useState } from "react";
import type { CSSProperties, ReactNode } from "react";
import type { HealthDto, StatusDto, AskFeedDto } from "./api-types";
import StalenessBadge from "./components/StalenessBadge";
import { useProjection } from "./hooks/useProjection";
import type { ViewName } from "./hooks/useProjection";
import StatusView from "./views/StatusView";
import AskView from "./views/AskView";
import WarRoomView from "./views/WarRoomView";
import AgentStreamView from "./views/AgentStreamView";
import AuditView from "./views/AuditView";
import McpView from "./views/McpView";
import ProvidersView from "./views/ProvidersView";
import RunsView from "./views/RunsView";

type Tab = "status" | ViewName | "ask";

const TABS: { id: Tab; label: string }[] = [
  { id: "status", label: "Status" },
  { id: "war-room", label: "War Room" },
  { id: "agent-stream", label: "Stream" },
  { id: "audit", label: "Audit" },
  { id: "mcp", label: "MCP" },
  { id: "providers", label: "Providers" },
  { id: "runs", label: "Runs" },
  { id: "ask", label: "Ask" },
];

function tabFromHash(): Tab {
  const h = window.location.hash.replace(/^#\/?/, "");
  return (TABS.some((t) => t.id === h) ? h : "status") as Tab;
}

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

type ShellData = ReturnType<typeof useProjection> & {
  status: StatusDto;
  ask: AskFeedDto;
};

function StatusBody({ data }: { data: ShellData }) {
  return (
    <StatusView status={data.status} gap={data.gap} onResync={data.resync} />
  );
}

function AskBody({ data }: { data: ShellData }) {
  return <AskView feed={data.ask} onDecided={data.refreshAsk} />;
}

function WarRoomBody({ data }: { data: ShellData }) {
  return (
    <WarRoomView
      feed={data.warRoom}
      error={data.viewErrors["war-room"]}
      onRetry={data.retryView}
    />
  );
}

function StreamBody({ data }: { data: ShellData }) {
  return (
    <AgentStreamView
      feed={data.agentStream}
      error={data.viewErrors["agent-stream"]}
      gap={data.gap}
      onResync={data.resync}
      onRetry={data.retryView}
    />
  );
}

function AuditBody({ data }: { data: ShellData }) {
  return (
    <AuditView
      feed={data.audit}
      error={data.viewErrors.audit}
      onRetry={data.retryView}
    />
  );
}

function McpBody({ data }: { data: ShellData }) {
  return (
    <McpView feed={data.mcp} error={data.viewErrors.mcp} onRetry={data.retryView} />
  );
}

function ProvidersBody({ data }: { data: ShellData }) {
  return (
    <ProvidersView
      feed={data.providers}
      error={data.viewErrors.providers}
      onRetry={data.retryView}
    />
  );
}

function RunsBody({ data }: { data: ShellData }) {
  return (
    <RunsView
      feed={data.runs}
      error={data.viewErrors.runs}
      onRetry={data.retryView}
    />
  );
}

// Tab rendering as data (complexity gate): one single-branch body per tab,
// selected by lookup — no ternary chain.
const TAB_BODY: Record<Tab, (data: ShellData) => ReactNode> = {
  status: (d) => <StatusBody data={d} />,
  ask: (d) => <AskBody data={d} />,
  "war-room": (d) => <WarRoomBody data={d} />,
  "agent-stream": (d) => <StreamBody data={d} />,
  audit: (d) => <AuditBody data={d} />,
  mcp: (d) => <McpBody data={d} />,
  providers: (d) => <ProvidersBody data={d} />,
  runs: (d) => <RunsBody data={d} />,
};

function ConnectedShell(data: ShellData) {
  const [tab, setTab] = useState<Tab>(tabFromHash);
  useEffect(() => {
    const onHash = () => setTab(tabFromHash());
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);
  const select = (t: Tab) => {
    window.location.hash = `#/${t}`;
    setTab(t);
  };
  return (
    <div className="cockpit">
      <header>
        <h1>STUDIO</h1> <StalenessBadge value={data.status.staleness} />
        <nav style={{ display: "flex", gap: 8, margin: "8px 0", flexWrap: "wrap" }} aria-label="Operator views">
          {TABS.map((t) => (
            <TabButton key={t.id} label={t.label} active={tab === t.id} onSelect={() => select(t.id)} />
          ))}
        </nav>
        <DaemonBanner health={data.health} />
        <PollError error={data.error} />
      </header>
      {TAB_BODY[tab](data)}
    </div>
  );
}

export default function App() {
  const proj = useProjection();
  const { status, ask, error } = proj;

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

  return <ConnectedShell {...proj} status={status} ask={ask} />;
}
