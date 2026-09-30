// ViewShell: the shared P05 view envelope (V7 amortization artifact).
// Every operator view renders badge + designed states through this shell:
// loading (skeleton, not blank), error (legible + SCOPED retry — refetches
// only this view), empty (designed, with the next action), ready (children).
// Per-view rendering (buckets vs kanban vs probes) stays hand-written in
// each view file — the shell owns only the envelope. One branch per tiny
// body component (complexity gate: small functions, no nesting).
import type { ReactNode } from "react";
import type { StalenessDto } from "../api-types";
import StalenessBadge from "./StalenessBadge";

export type ViewState = "loading" | "error" | "empty" | "ready";

export type ShellProps = {
  title: string;
  count: number | null;
  staleness: StalenessDto | null;
  state: ViewState;
  error: string | null;
  onRetry: () => void;
  emptyText: string;
  children: ReactNode;
  testid: string;
};

function ShellHead({
  title,
  count,
  staleness,
}: Pick<ShellProps, "title" | "count" | "staleness">) {
  return (
    <h2>
      {title}{" "}
      {count !== null ? <span className="count num">{count}</span> : null}{" "}
      {staleness ? <StalenessBadge value={staleness} /> : null}
    </h2>
  );
}

function LoadingBody({ title }: { title: string }) {
  return <p className="loading">loading {title} projection…</p>;
}

function ErrorBody({
  title,
  error,
  onRetry,
}: Pick<ShellProps, "title" | "error" | "onRetry">) {
  return (
    <div>
      <p className="error" role="alert">
        {title} unavailable ({error ?? "unknown error"}) — underlying data
        kept; nothing was lost.
      </p>
      <button type="button" onClick={onRetry} title={`Re-fetch ${title} only`}>
        Retry {title}
      </button>
    </div>
  );
}

function EmptyBody({ emptyText }: { emptyText: string }) {
  return <p className="empty">{emptyText}</p>;
}

// Ready-state refresh notice (F1): a poll failure with last-good data must
// not render silently — the server badge is frozen at its last-good value,
// so the shell adds this non-blocking error + scoped retry line above the
// last-good children. Total-down is still the Connecting banner (App).
function ReadyNotice({
  title,
  error,
  onRetry,
}: Pick<ShellProps, "title" | "error" | "onRetry">) {
  return (
    <div className="row">
      <span className="staleness badge-stale" role="status">
        refresh failed ({error ?? "unknown error"}) — showing last good data
      </span>{" "}
      <button
        type="button"
        onClick={onRetry}
        title={`Re-fetch ${title} only`}
      >
        Retry {title}
      </button>
    </div>
  );
}

function ShellBody(props: ShellProps) {
  // F1 + F1-empty: both ready and empty render the refresh notice above
  // last-good content when a poll failed — an empty board with a live error
  // is last-good data too (the DB answered empty, then stopped answering).
  const readyNotice =
    props.error === null ? null : <ReadyNotice {...props} />;
  const bodies: Record<ViewState, ReactNode> = {
    loading: <LoadingBody title={props.title} />,
    error: <ErrorBody {...props} />,
    empty: (
      <>
        {readyNotice}
        <EmptyBody emptyText={props.emptyText} />
      </>
    ),
    ready: (
      <>
        {readyNotice}
        {props.children}
      </>
    ),
  };
  return <>{bodies[props.state]}</>;
}

export default function ViewShell(props: ShellProps) {
  return (
    <main className="grid" data-view={props.testid}>
      <section className="card">
        <ShellHead
          title={props.title}
          count={props.count}
          staleness={props.staleness}
        />
        <ShellBody {...props} />
      </section>
    </main>
  );
}
