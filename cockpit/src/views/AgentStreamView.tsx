// Agent Stream: normalized AgentEvent union (f05-px-events clean-room,
// MIT) + HookProvider protocol record. Rows declaring an unsupported
// protocolVersion are REFUSED (red, never executed, never dropped silently);
// unknown tables with a supported version surface as unknown/unrefused.
// Cursor gap (Lost) renders the re-sync path, same latch as Status.
// Small single-branch pieces (complexity gate).
import type { AgentEventDto, AgentStreamDto, HookProviderDto } from "../api-types";
import ViewShell from "../components/ViewShell";
import type { ViewName } from "../hooks/useProjection";
import type { GapLatch } from "./StatusView";

function ProviderPills({ providers }: { providers: HookProviderDto[] }) {
  return (
    <>
      {providers.map((p) => (
        <span
          key={p.name}
          className="pill"
          title={p.compatible ? "supported version" : "unknown version — refused"}
        >
          {p.name} v{p.protocol_version}
          {p.compatible ? "" : " (refused)"}
        </span>
      ))}
    </>
  );
}

function RefusedBanner({ count }: { count: number }) {
  if (count === 0) return null;
  return (
    <span className="staleness badge-lost">
      {count} refused (unknown version — surfaced, never executed)
    </span>
  );
}

function GapNotice({ feed, onResync }: { feed: AgentStreamDto; onResync: () => void }) {
  if (feed.gap !== "Lost") return null;
  return (
    <div className="row">
      <span className="staleness badge-lost" data-state="Lost">
        lost · ring advanced past cursor (floor {feed.floor_seq}) — re-sync
        required
      </span>{" "}
      <button
        type="button"
        onClick={onResync}
        title="Re-sync the event cursor from the ring head"
      >
        Re-sync from head
      </button>
    </div>
  );
}

function EventRow({ event }: { event: AgentEventDto }) {
  return (
    <div className="row">
      <span className="num">{event.seq}</span>{" "}
      <span className={event.refused ? "staleness badge-lost" : "pill"}>
        {event.kind}
      </span>{" "}
      <span className="dim">
        {event.provider} v{event.protocol_version}
      </span>{" "}
      <code>{event.payload}</code>
      {event.refused ? <span> — refused: unknown provider version</span> : null}
    </div>
  );
}

function StreamBody({
  feed,
  gap,
  onResync,
}: {
  feed: AgentStreamDto;
  gap: GapLatch;
  onResync: () => void;
}) {
  const refused = feed.events.filter((e) => e.refused).length;
  const showGap = gap.lost || feed.gap === "Lost";
  return (
    <>
      <div className="row">
        <span className="dim">hook protocol</span>{" "}
        <span className="num">v{feed.provider_protocol_version}</span>{" "}
        <ProviderPills providers={feed.providers} />{" "}
        <RefusedBanner count={refused} />
      </div>
      {showGap ? <GapNotice feed={feed} onResync={onResync} /> : null}
      {feed.events.map((e) => (
        <EventRow key={e.seq} event={e} />
      ))}
    </>
  );
}

export default function AgentStreamView({
  feed,
  error,
  gap,
  onResync,
  onRetry,
}: {
  feed: AgentStreamDto | null;
  error: string | null;
  gap: GapLatch;
  onResync: () => void;
  onRetry: (name: ViewName) => void;
}) {
  if (feed === null) {
    return (
      <ViewShell
        title="Agent Stream"
        count={null}
        staleness={null}
        state={error === null ? "loading" : "error"}
        error={error}
        onRetry={() => onRetry("agent-stream")}
        emptyText=""
        testid="agent-stream"
      >
        {null}
      </ViewShell>
    );
  }
  if (feed.events.length === 0) {
    return (
      <ViewShell
        title="Agent Stream"
        count={0}
        staleness={feed.staleness}
        state="empty"
        error={error}
        onRetry={() => onRetry("agent-stream")}
        emptyText="No agent events yet — task mutations and hook payloads will stream here."
        testid="agent-stream"
      >
        {null}
      </ViewShell>
    );
  }
  return (
    <ViewShell
      title="Agent Stream"
      count={feed.events.length}
      staleness={feed.staleness}
      state="ready"
      error={error}
      onRetry={() => onRetry("agent-stream")}
      emptyText=""
      testid="agent-stream"
    >
      <StreamBody feed={feed} gap={gap} onResync={onResync} />
    </ViewShell>
  );
}
