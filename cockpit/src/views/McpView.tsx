// MCP Registry: versioned per-agent detection manifests (f05-herdr-detect
// clean-room, Apache-2.0 — PATTERN ONLY; ghostty-vt explicitly NOT a
// dependency). Bundled entries PATH-probe real binaries; the override slot
// reads STUDIO_MCP_MANIFEST when configured; remote is listed, never
// fetched silently. Flags are real-signal derived: detected × daemon.
import type { McpRegistryDto } from "../api-types";
import ViewShell from "../components/ViewShell";
import type { ViewName } from "../hooks/useProjection";

const STATE_CLASS: Record<string, string> = {
  working: "badge-fresh",
  blocked: "badge-lost",
  idle: "badge-stale",
};

export default function McpView({
  feed,
  error,
  onRetry,
}: {
  feed: McpRegistryDto | null;
  error: string | null;
  onRetry: (name: ViewName) => void;
}) {
  if (!feed) {
    return (
      <ViewShell
        title="MCP Registry"
        count={null}
        staleness={null}
        state={error ? "error" : "loading"}
        error={error}
        onRetry={() => onRetry("mcp")}
        emptyText=""
        testid="mcp"
      >
        {null}
      </ViewShell>
    );
  }
  if (feed.agents.length === 0) {
    return (
      <ViewShell
        title="MCP Registry"
        count={0}
        staleness={feed.staleness}
        state="empty"
        error={error}
        onRetry={() => onRetry("mcp")}
        emptyText="No detection manifests — the bundled agent list failed to load."
        testid="mcp"
      >
        {null}
      </ViewShell>
    );
  }
  return (
    <ViewShell
      title="MCP Registry"
      count={feed.agents.length}
      staleness={feed.staleness}
      state="ready"
      error={error}
      onRetry={() => onRetry("mcp")}
      emptyText=""
      testid="mcp"
    >
      {feed.agents.map((a) => (
        <div className="row" key={`${a.manifest_source}-${a.name}`}>
          <strong>{a.name}</strong>{" "}
          <span className="dim">
            {a.manifest_source} v{a.version}
          </span>{" "}
          <span
            className={`staleness ${STATE_CLASS[a.state] ?? "badge-stale"}`}
            title={
              a.state === "working"
                ? "binary detected and daemon reachable"
                : a.state === "blocked"
                  ? "binary detected but daemon unreachable (or manifest unreadable)"
                  : "binary not detected — idle"
            }
          >
            {a.state}
          </span>
          <br />
          <span className="dim">explain: {a.explain}</span>
        </div>
      ))}
    </ViewShell>
  );
}
