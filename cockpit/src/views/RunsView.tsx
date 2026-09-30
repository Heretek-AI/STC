// Runs: multi-run compare slots (f05-openchamber-multirun clean-room,
// MIT) — up to 5 live worktree rows with receipt evidence counts. Scoped
// to what the projection supports: per-model columns + guided changes
// walkthrough are DEFERRED (tasks carry no run/model linkage in schema v2,
// so a per-model matrix would be mock data) — stated in `deferred`, below.
import type { RunsDto } from "../api-types";
import ViewShell from "../components/ViewShell";
import type { ViewName } from "../hooks/useProjection";

export default function RunsView({
  feed,
  error,
  onRetry,
}: {
  feed: RunsDto | null;
  error: string | null;
  onRetry: (name: ViewName) => void;
}) {
  if (!feed) {
    return (
      <ViewShell
        title="Runs"
        count={null}
        staleness={null}
        state={error ? "error" : "loading"}
        error={error}
        onRetry={() => onRetry("runs")}
        emptyText=""
        testid="runs"
      >
        {null}
      </ViewShell>
    );
  }
  if (feed.runs.length === 0) {
    return (
      <ViewShell
        title="Runs"
        count={0}
        staleness={feed.staleness}
        state="empty"
        error={error}
        onRetry={() => onRetry("runs")}
        emptyText="No runs yet — per-run worktrees appear here (up to 5) once lanes start."
        testid="runs"
      >
        {null}
      </ViewShell>
    );
  }
  return (
    <ViewShell
      title="Runs"
      count={feed.runs.length}
      staleness={feed.staleness}
      state="ready"
      error={error}
      onRetry={() => onRetry("runs")}
      emptyText=""
      testid="runs"
    >
      {feed.runs.map((r) => (
        <div className="row" key={r.slug}>
          <code>{r.slug}</code> <span className="pill">{r.state}</span>{" "}
          <span className="dim">{r.path}</span>{" "}
          <span className="dim">
            evidence receipts <span className="num">{r.receipts}</span>
          </span>
        </div>
      ))}
      <p className="dim">deferred: {feed.deferred}</p>
    </ViewShell>
  );
}
