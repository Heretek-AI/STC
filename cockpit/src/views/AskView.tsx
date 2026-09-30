// Ask queue (A1, P05): one-tap approve/deny through the P02 approval
// service (`POST /api/ask/:id/decide`, CAS on status='pending').
// Buttons are enabled ONLY where the service exposes a live decision path
// (`decision_enabled && status === 'pending'`); every decision lands in the
// durable ledger (`contract_approvals`); anything not service-backed stays
// visibly disabled with the reason. Desktop notification: attention payload
// patterns only (see WarRoomView) — no mobile code, no push infra.
// Small single-branch pieces (complexity gate): hook owns the attempt,
// components own one rendering each.
import { useState } from "react";
import type { AskFeedDto, AskItemDto } from "../api-types";
import { postDecide } from "../api";

function defaultReason(approved: boolean): string {
  return approved ? "approved from cockpit" : "denied from cockpit";
}

function finalReason(reason: string, approved: boolean): string {
  const why = reason.trim();
  return why === "" ? defaultReason(approved) : why;
}

function useDecide(item: AskItemDto, onDecided: () => void) {
  const [reason, setReason] = useState("");
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<string | null>(null);
  const [problem, setProblem] = useState<string | null>(null);

  async function decide(approved: boolean) {
    setBusy(true);
    setProblem(null);
    try {
      const out = await postDecide(item.id, {
        approved,
        reason: finalReason(reason, approved),
      });
      setResult(`${out.status} (ledger, decided_ms ${out.decided_ms ?? "?"})`);
      onDecided();
    } catch (e) {
      setProblem(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  return { reason, setReason, busy, result, problem, decide };
}

function LiveButtons({
  busy,
  onDecide,
}: {
  busy: boolean;
  onDecide: (approved: boolean) => void;
}) {
  return (
    <>
      <button
        type="button"
        onClick={() => void onDecide(true)}
        disabled={busy}
        title="Approve via the P02 approval service (single-shot CAS, lands in the ledger)"
      >
        {busy ? "…" : "Approve"}
      </button>{" "}
      <button
        type="button"
        onClick={() => void onDecide(false)}
        disabled={busy}
        title="Deny via the P02 approval service (single-shot CAS, lands in the ledger)"
      >
        {busy ? "…" : "Deny"}
      </button>
    </>
  );
}

function DeadButtons({ item }: { item: AskItemDto }) {
  const why =
    item.status === "pending"
      ? "No live decision path for this request — stays disabled, never fake-enabled."
      : `Already ${item.status}: decisions are single-shot — stays disabled.`;
  return (
    <>
      <button type="button" disabled title={why}>
        Approve
      </button>{" "}
      <button type="button" disabled title={why}>
        Deny
      </button>
    </>
  );
}

function DecisionOutcome({
  result,
  problem,
}: {
  result: string | null;
  problem: string | null;
}) {
  if (result !== null) return <span className="dim"> — {result}</span>;
  if (problem !== null)
    return (
      <span className="error" role="alert">
        {" "}
        — {problem}
      </span>
    );
  return null;
}

function DecideRow({
  item,
  onDecided,
}: {
  item: AskItemDto;
  onDecided: () => void;
}) {
  const live = item.decision_enabled && item.status === "pending";
  const d = useDecide(item, onDecided);
  return (
    <div className="row" key={item.id}>
      <code>{item.id}</code>{" "}
      <span className="dim">
        task={item.task_id} tool={item.tool}
      </span>{" "}
      <span className="pill">{item.status}</span>{" "}
      {live ? (
        <>
          <input
            type="text"
            className="field"
            value={d.reason}
            onChange={(e) => d.setReason(e.target.value)}
            placeholder="reason (optional)"
            aria-label={`decision reason for ${item.id}`}
            maxLength={1024}
            disabled={d.busy}
          />{" "}
          <LiveButtons busy={d.busy} onDecide={(a) => void d.decide(a)} />
        </>
      ) : (
        <DeadButtons item={item} />
      )}
      <DecisionOutcome result={d.result} problem={d.problem} />
    </div>
  );
}

export default function AskView({
  feed,
  onDecided,
}: {
  feed: AskFeedDto;
  onDecided: () => void;
}) {
  return (
    <main className="grid" data-view="ask">
      <section className="card">
        <h2>
          Ask queue <span className="count">{feed.items.length}</span>
        </h2>
        <p className="dim">{feed.decision_contract}</p>
        {feed.items.length === 0 ? (
          <p className="empty">No approval requests — nothing waiting.</p>
        ) : (
          feed.items.map((item) => (
            <DecideRow key={item.id} item={item} onDecided={onDecided} />
          ))
        )}
      </section>
    </main>
  );
}
