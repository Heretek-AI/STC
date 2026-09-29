// Projection polling hook (P04): reads /api/* on a 500ms cadence and owns
// the event-cursor gap latch. Kept in its own module with single-branch
// helpers so every function stays under the complexity gate.
import { useEffect, useRef, useState } from "react";
import type { AskFeedDto, HealthDto, StatusDto } from "../api-types";
import { fetchAsk, fetchEventsSince, fetchHealth, fetchStatus } from "../api";
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

export type Projection = {
  status: StatusDto | null;
  ask: AskFeedDto | null;
  health: HealthDto | null;
  gap: GapLatch;
  error: string | null;
  resync: () => void;
};

export function useProjection(): Projection {
  const [status, setStatus] = useState<StatusDto | null>(null);
  const [ask, setAsk] = useState<AskFeedDto | null>(null);
  const [health, setHealth] = useState<HealthDto | null>(null);
  const [gap, setGap] = useState<GapLatch>({ lost: false, head: 0 });
  const [error, setError] = useState<string | null>(null);
  const failures = useRef(0);
  const cursor = useRef(0);

  useEffect(() => {
    let alive = true;
    const noteOk = (s: StatusDto, a: AskFeedDto, h: HealthDto) => {
      failures.current = 0;
      setError(null);
      setStatus(s);
      setAsk(a);
      setHealth(h);
    };
    const noteErr = (e: unknown) => {
      failures.current += 1;
      if (failures.current >= 3) {
        setError(e instanceof Error ? e.message : String(e));
      }
    };
    const poll = () => {
      Promise.all([
        fetchStatus(),
        fetchAsk(),
        fetchHealth(),
        fetchEventsSince(cursor.current),
      ]).then(
        ([s, a, h, ev]) => {
          if (alive) {
            noteOk(s, a, h);
            applyEvents(ev, cursor, setGap);
          }
        },
        (e: unknown) => {
          if (alive) noteErr(e);
        }
      );
    };
    poll();
    const t = setInterval(poll, 500);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, []);

  const resync = () => {
    cursor.current = gap.head;
    setGap({ lost: false, head: gap.head });
  };

  return { status, ask, health, gap, error, resync };
}
