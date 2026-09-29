// Status view — the ONE real P04 view: live task projection + counts.
// Every row is DB data (zero mock); the badge carries the taxonomy.
import type { ReactNode } from "react";
import type { StatusDto } from "../api-types";
import StalenessBadge from "../components/StalenessBadge";

export type GapLatch = { lost: true; head: number } | { lost: false; head: number };

const LANE_COLORS: Record<string, string> = {
  building: "var(--status-building)",
  validating: "var(--status-validating)",
  "in-review": "var(--status-review)",
  ready: "var(--status-ready)",
};

function StatusPill({ lane, status }: { lane: string; status: string }) {
  const color = LANE_COLORS[lane] ?? "var(--text-dim)";
  return (
    <span className="pill" style={{ borderColor: color, color }}>
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

export default function StatusView({
  status,
  gap,
  onResync,
}: {
  status: StatusDto;
  gap: GapLatch;
  onResync: () => void;
}) {
  const c = status.counts;
  return (
    <main className="grid">
      <Section title="Tasks" count={status.tasks.length} empty="No tasks yet — dispatch from the manager.">
        {status.tasks.map((t) => (
          <div className="row" key={t.id}>
            <StatusPill lane={t.lane} status={t.status} />
            <code>{t.id}</code> <span className="dim">{t.kind}</span>
          </div>
        ))}
      </Section>
      <Section
        title="Lanes"
        count={c.building + c.validating + c.in_review + c.ready}
        empty="No lane activity."
      >
        <div className="row">
          <span>building</span> <span className="num">{c.building}</span>
        </div>
        <div className="row">
          <span>validating</span> <span className="num">{c.validating}</span>
        </div>
        <div className="row">
          <span>in-review</span> <span className="num">{c.in_review}</span>
        </div>
        <div className="row">
          <span>ready</span> <span className="num">{c.ready}</span>
        </div>
      </Section>
      <Section title="Projection" count={1} empty="">
        <div className="row">
          <StalenessBadge value={status.staleness} />
        </div>
        {gap.lost ? (
          <div className="row">
            <span className="staleness badge-lost" data-state="Lost">
              lost · source: {status.source} · ring advanced past cursor —
              re-sync required (head {gap.head})
            </span>{" "}
            <button type="button" onClick={onResync} title="Re-sync the event cursor from the ring head">
              Re-sync from head
            </button>
          </div>
        ) : null}
        <div className="row">
          <span className="dim">change_seq</span>{" "}
          <span className="num">{status.change_seq}</span>
        </div>
        <div className="row">
          <span className="dim">daemon</span>{" "}
          <span>{status.daemon_reachable ? "reachable" : "offline (read-only)"}</span>
        </div>
      </Section>
    </main>
  );
}
