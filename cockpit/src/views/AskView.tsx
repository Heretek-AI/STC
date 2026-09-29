// Ask queue (A1 read scope, P04): read-only surfacing over the P02 approval
// store. One-tap decision buttons render VISIBLY DISABLED with the reason:
// the approval service exposes no decide endpoint yet (decide-later wiring
// lands with P05). Never fake-enabled.
import type { AskFeedDto } from "../api-types";

export default function AskView({ feed }: { feed: AskFeedDto }) {
  return (
    <main className="grid">
      <section className="card">
        <h2>
          Ask queue <span className="count">{feed.items.length}</span>
        </h2>
        <p className="dim">{feed.decision_contract}</p>
        {feed.items.length === 0 ? (
          <p className="empty">No approval requests — nothing waiting.</p>
        ) : (
          feed.items.map((item) => (
            <div className="row" key={item.id}>
              <code>{item.id}</code>{" "}
              <span className="dim">
                task={item.task_id} tool={item.tool}
              </span>{" "}
              <span className="pill">{item.status}</span>{" "}
              <button
                type="button"
                disabled
                title="Decide-later (P05): the approval service exposes no decide endpoint yet — this button is disabled, never fake-enabled."
              >
                Approve (P05)
              </button>{" "}
              <button
                type="button"
                disabled
                title="Decide-later (P05): the approval service exposes no decide endpoint yet — this button is disabled, never fake-enabled."
              >
                Deny (P05)
              </button>
            </div>
          ))
        )}
      </section>
    </main>
  );
}
