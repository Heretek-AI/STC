// Projection polling hook (P04 shell + P05 six views): reads /api/* on a 1s
// cadence and owns the event-cursor gap latch. Every view keeps its last
// good payload plus its own error slot, so one failing view never blanks
// the others; `retryView` refetches exactly one view (scoped retry).
import { useCallback, useEffect, useRef, useState } from "react";
import type {
  AgentStreamDto,
  AskFeedDto,
  AuditDto,
  HealthDto,
  McpRegistryDto,
  ProvidersDto,
  RunsDto,
  StatusDto,
  WarRoomDto,
} from "../api-types";
import {
  fetchAgentStream,
  fetchAsk,
  fetchAudit,
  fetchEventsSince,
  fetchHealth,
  fetchMcp,
  fetchProviders,
  fetchRuns,
  fetchStatus,
  fetchWarRoom,
} from "../api";
import type { GapLatch } from "../views/StatusView";

// Event-cursor advance for one poll cycle: Lost latches (cursor frozen until
// the operator re-syncs); a contiguous page advances the cursor but never
// clears an existing latch.
function applyEvents(
  ev: { gap: "None" | "Lost"; head_seq: number },
  cursor: { current: number },
  setGap: React.Dispatch<React.SetStateAction<GapLatch>>
) {
  if (ev.gap === "Lost") {
    setGap((g) => (g.lost ? g : { lost: true, head: ev.head_seq }));
    return;
  }
  cursor.current = ev.head_seq;
  setGap((g) => (g.lost ? g : { lost: false, head: ev.head_seq }));
}

export type ViewName =
  | "war-room"
  | "agent-stream"
  | "audit"
  | "mcp"
  | "providers"
  | "runs";

export type Projection = {
  status: StatusDto | null;
  ask: AskFeedDto | null;
  health: HealthDto | null;
  gap: GapLatch;
  error: string | null;
  resync: () => void;
  refreshAsk: () => void;
  warRoom: WarRoomDto | null;
  agentStream: AgentStreamDto | null;
  audit: AuditDto | null;
  mcp: McpRegistryDto | null;
  providers: ProvidersDto | null;
  runs: RunsDto | null;
  viewErrors: Record<ViewName, string | null>;
  retryView: (name: ViewName) => void;
};

// One view fetch: on success clears that view's error slot, on failure sets
// it — other views are untouched.
async function fetchOne(
  name: ViewName,
  run: () => Promise<unknown>,
  setViewErrors: ErrorSetter
) {
  try {
    await run();
    setViewErrors((e) => ({ ...e, [name]: null }));
  } catch (e) {
    setViewErrors((prev) => ({
      ...prev,
      [name]: e instanceof Error ? e.message : String(e),
    }));
  }
}

const EMPTY_ERRORS: Record<ViewName, string | null> = {
  "war-room": null,
  "agent-stream": null,
  audit: null,
  mcp: null,
  providers: null,
  runs: null,
};

type ErrorSetter = React.Dispatch<
  React.SetStateAction<Record<ViewName, string | null>>
>;

// Core poll (P04 shell): status + ask + health + event cursor on a 1s
// cadence. Owns the gap latch; failures surface after 3 consecutive misses.
function useCorePoll() {
  const [status, setStatus] = useState<StatusDto | null>(null);
  const [ask, setAsk] = useState<AskFeedDto | null>(null);
  const [health, setHealth] = useState<HealthDto | null>(null);
  const [gap, setGap] = useState<GapLatch>({ lost: false, head: 0 });
  const [error, setError] = useState<string | null>(null);
  const failures = useRef(0);
  const cursor = useRef(0);

  useEffect(() => {
    let alive = true;
    const poll = () => {
      Promise.all([
        fetchStatus(),
        fetchAsk(),
        fetchHealth(),
        fetchEventsSince(cursor.current),
      ]).then(
        ([s, a, h, ev]) => {
          if (!alive) return;
          failures.current = 0;
          setError(null);
          setStatus(s);
          setAsk(a);
          setHealth(h);
          applyEvents(ev, cursor, setGap);
        },
        (e: unknown) => {
          if (!alive) return;
          failures.current += 1;
          if (failures.current >= 3) {
            setError(e instanceof Error ? e.message : String(e));
          }
        }
      );
    };
    poll();
    const t = setInterval(poll, 1000);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, []);

  const resync = useCallback(() => {
    cursor.current = gap.head;
    setGap({ lost: false, head: gap.head });
  }, [gap.head]);

  const refreshAsk = useCallback(() => {
    fetchAsk().then(
      (a) => setAsk(a),
      () => undefined
    );
  }, []);

  return { status, ask, health, gap, error, resync, refreshAsk, cursor };
}

type ViewData = {
  warRoom: WarRoomDto | null;
  agentStream: AgentStreamDto | null;
  audit: AuditDto | null;
  mcp: McpRegistryDto | null;
  providers: ProvidersDto | null;
  runs: RunsDto | null;
};

// P05 view poll: all six operator views, each with its own error slot so
// one failing view never blanks the others. Scoped retry refetches one.
function useViewPoll(cursor: { current: number }) {
  const [warRoom, setWarRoom] = useState<WarRoomDto | null>(null);
  const [agentStream, setAgentStream] = useState<AgentStreamDto | null>(null);
  const [audit, setAudit] = useState<AuditDto | null>(null);
  const [mcp, setMcp] = useState<McpRegistryDto | null>(null);
  const [providers, setProviders] = useState<ProvidersDto | null>(null);
  const [runs, setRuns] = useState<RunsDto | null>(null);
  const [viewErrors, setViewErrors] =
    useState<Record<ViewName, string | null>>(EMPTY_ERRORS);

  const fetchers = useCallback(
    (name: ViewName): (() => Promise<unknown>) => {
      const table: Record<ViewName, () => Promise<unknown>> = {
        "war-room": () =>
          fetchWarRoom().then((v) => {
            setWarRoom(v);
          }),
        "agent-stream": () =>
          fetchAgentStream(cursor.current).then((v) => {
            setAgentStream(v);
          }),
        audit: () =>
          fetchAudit().then((v) => {
            setAudit(v);
          }),
        mcp: () =>
          fetchMcp().then((v) => {
            setMcp(v);
          }),
        providers: () =>
          fetchProviders().then((v) => {
            setProviders(v);
          }),
        runs: () =>
          fetchRuns().then((v) => {
            setRuns(v);
          }),
      };
      return table[name];
    },
    [cursor]
  );

  const pollViews = useCallback(() => {
    for (const name of Object.keys(EMPTY_ERRORS) as ViewName[]) {
      void fetchOne(name, fetchers(name), setViewErrors);
    }
  }, [fetchers]);

  const retryView = useCallback(
    (name: ViewName) => {
      void fetchOne(name, fetchers(name), setViewErrors);
    },
    [fetchers]
  );

  useEffect(() => {
    pollViews();
    const t = setInterval(pollViews, 1000);
    return () => clearInterval(t);
  }, [pollViews]);

  const data: ViewData = { warRoom, agentStream, audit, mcp, providers, runs };
  return { ...data, viewErrors, retryView };
}

export function useProjection(): Projection {
  const core = useCorePoll();
  const views = useViewPoll(core.cursor);
  return { ...core, ...views };
}
