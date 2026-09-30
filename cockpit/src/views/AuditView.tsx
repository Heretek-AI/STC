// Audit + Gatekeeper: kanban board + in-app review (f05-vk-kanban-diff
// clean-room, Apache-2.0). Cards carry receipt evidence ids; feedback rows
// are head-SHA-fenced: a note applies only at its recorded head —
// current_head=false renders as a fenced (superseded) review, never
// silently applied to a newer head. Small single-branch pieces.
import type { AuditDto, FeedbackDto, KanbanCardDto, KanbanColumnDto } from "../api-types";
import ViewShell from "../components/ViewShell";
import type { ViewName } from "../hooks/useProjection";

function EvidenceTag({ card }: { card: KanbanCardDto }) {
  if (card.evidence.length > 0) {
    return <span className="dim">evidence: {card.evidence.join(", ")}</span>;
  }
  return <span className="dim">no evidence yet</span>;
}

function KanbanColumn({ column }: { column: KanbanColumnDto }) {
  return (
    <section>
      <h3>
        {column.name} <span className="count num">{column.cards.length}</span>
      </h3>
      {column.cards.length === 0 ? (
        <p className="empty">lane clear.</p>
      ) : (
        column.cards.map((c) => (
          <div className="row" key={c.id}>
            <code>{c.id}</code> <span className="dim">{c.kind}</span>{" "}
            <span className="pill">{c.status}</span> <EvidenceTag card={c} />
          </div>
        ))
      )}
    </section>
  );
}

function FenceTag({ feedback }: { feedback: FeedbackDto }) {
  if (feedback.current_head) return <span className="pill">current head</span>;
  return (
    <span
      className="staleness badge-stale"
      title="Recorded against an older head — review applies only at its head"
    >
      fenced (superseded)
    </span>
  );
}

function TaskLink({ feedback }: { feedback: FeedbackDto }) {
  if (feedback.task_id !== "") {
    return (
      <span>
        task <code>{feedback.task_id}</code>
      </span>
    );
  }
  return <span className="dim">unlinked fence</span>;
}

function FeedbackRow({ feedback }: { feedback: FeedbackDto }) {
  return (
    <div className="row">
      <code>{feedback.id}</code> <TaskLink feedback={feedback} />{" "}
      <span className="dim">
        head {feedback.head_sha.slice(0, 12)} · {feedback.target_ref} ·{" "}
        {feedback.tier}
      </span>{" "}
      <FenceTag feedback={feedback} /> <span>{feedback.body}</span>
    </div>
  );
}

function FeedbackSection({ feed }: { feed: AuditDto }) {
  if (feed.feedback.length === 0) {
    return (
      <section>
        <h3>
          Review feedback <span className="count num">0</span>
        </h3>
        <p className="empty">no frozen review heads recorded.</p>
      </section>
    );
  }
  return (
    <section>
      <h3>
        Review feedback{" "}
        <span className="count num">{feed.feedback.length}</span>
      </h3>
      {feed.feedback.map((f) => (
        <FeedbackRow key={f.id} feedback={f} />
      ))}
    </section>
  );
}

function AuditBody({ feed }: { feed: AuditDto }) {
  return (
    <>
      {feed.columns.map((col) => (
        <KanbanColumn key={col.name} column={col} />
      ))}
      <FeedbackSection feed={feed} />
    </>
  );
}

export default function AuditView({
  feed,
  error,
  onRetry,
}: {
  feed: AuditDto | null;
  error: string | null;
  onRetry: (name: ViewName) => void;
}) {
  if (feed === null) {
    return (
      <ViewShell
        title="Audit"
        count={null}
        staleness={null}
        state={error === null ? "loading" : "error"}
        error={error}
        onRetry={() => onRetry("audit")}
        emptyText=""
        testid="audit"
      >
        {null}
      </ViewShell>
    );
  }
  const cards = feed.columns.reduce((n, c) => n + c.cards.length, 0);
  const items = cards + feed.feedback.length;
  if (items === 0) {
    return (
      <ViewShell
        title="Audit"
        count={0}
        staleness={feed.staleness}
        state="empty"
        error={error}
        onRetry={() => onRetry("audit")}
        emptyText="Nothing to review — the board fills as tasks move through lanes."
        testid="audit"
      >
        {null}
      </ViewShell>
    );
  }
  return (
    <ViewShell
      title="Audit"
      count={cards}
      staleness={feed.staleness}
      state="ready"
      error={error}
      onRetry={() => onRetry("audit")}
      emptyText=""
      testid="audit"
    >
      <AuditBody feed={feed} />
    </ViewShell>
  );
}
