// Providers: health-probe cache with TTL + budgets (f05-openfang-providers
// clean-room, MIT OR Apache-2.0) + per-harness ACP auth-status probes
// (f05-oh-acp-authprobe clean-room, MIT). Spend comes from the real
// token_ledger (cache hits tracked separately, never netted); auth probes
// check credential-file presence only — secrets are never read.
// Small single-branch pieces (complexity gate).
import type { AuthProbeDto, BudgetDto, ProbeDto, ProvidersDto } from "../api-types";
import ViewShell from "../components/ViewShell";
import type { ViewName } from "../hooks/useProjection";

const AUTH_CLASS: Record<string, string> = {
  present: "badge-fresh",
  missing: "badge-stale",
  unknown: "badge-stale",
};

function authClass(status: string): string {
  return AUTH_CLASS[status] ?? "badge-stale";
}

function probeBadge(fresh: boolean): string {
  return fresh ? "badge-fresh" : "badge-stale";
}

function ProbeRow({ probe }: { probe: ProbeDto }) {
  return (
    <div className="row">
      <strong>{probe.name}</strong>{" "}
      <span
        className={`staleness ${probeBadge(probe.fresh)}`}
        title={
          probe.fresh
            ? "served from the TTL cache — no new probe ran"
            : "just re-probed this request"
        }
      >
        {probe.fresh ? "cached fresh" : "re-probed"}
      </span>{" "}
      <span className="dim">
        ttl <span className="num">{probe.ttl_ms}ms</span>
      </span>
      <br />
      <span className="dim">{probe.detail}</span>
    </div>
  );
}

function ProbeSection({ feed }: { feed: ProvidersDto }) {
  return (
    <section>
      <h3>Health probes (TTL)</h3>
      {feed.probes.map((p) => (
        <ProbeRow key={p.name} probe={p} />
      ))}
    </section>
  );
}

function BudgetRow({ budget }: { budget: BudgetDto }) {
  return (
    <div className="row">
      <code>{budget.session}</code> billed{" "}
      <span className="num">{budget.total}</span>{" "}
      <span className="dim">
        cache hits <span className="num">{budget.cache_hits}</span> (tracked,
        never netted)
      </span>
    </div>
  );
}

function BudgetSection({ feed }: { feed: ProvidersDto }) {
  if (feed.budgets.length === 0) {
    return (
      <section>
        <h3>
          Budgets{" "}
          <span className="count">
            lifetime <span className="num">{feed.lifetime_total}</span>
          </span>
        </h3>
        <p className="empty">no billed spend recorded yet.</p>
      </section>
    );
  }
  return (
    <section>
      <h3>
        Budgets{" "}
        <span className="count">
          lifetime <span className="num">{feed.lifetime_total}</span>
        </span>
      </h3>
      {feed.budgets.map((b) => (
        <BudgetRow key={b.session} budget={b} />
      ))}
    </section>
  );
}

function AuthRow({ probe }: { probe: AuthProbeDto }) {
  return (
    <div className="row">
      <code>{probe.harness}</code> <span className="dim">{probe.method}</span>{" "}
      <span className={`staleness ${authClass(probe.status)}`}>
        {probe.status}
      </span>{" "}
      <span className="dim">{probe.detail}</span>
    </div>
  );
}

function AuthSection({ feed }: { feed: ProvidersDto }) {
  return (
    <section>
      <h3>Auth status</h3>
      {feed.auth.map((a) => (
        <AuthRow key={`${a.harness}-${a.method}`} probe={a} />
      ))}
    </section>
  );
}

function ProvidersBody({ feed }: { feed: ProvidersDto }) {
  return (
    <>
      <ProbeSection feed={feed} />
      <BudgetSection feed={feed} />
      <AuthSection feed={feed} />
    </>
  );
}

export default function ProvidersView({
  feed,
  error,
  onRetry,
}: {
  feed: ProvidersDto | null;
  error: string | null;
  onRetry: (name: ViewName) => void;
}) {
  if (feed === null) {
    return (
      <ViewShell
        title="Providers"
        count={null}
        staleness={null}
        state={error === null ? "loading" : "error"}
        error={error}
        onRetry={() => onRetry("providers")}
        emptyText=""
        testid="providers"
      >
        {null}
      </ViewShell>
    );
  }
  const items =
    feed.probes.length + feed.budgets.length + feed.auth.length;
  if (items === 0) {
    return (
      <ViewShell
        title="Providers"
        count={0}
        staleness={feed.staleness}
        state="empty"
        error={error}
        onRetry={() => onRetry("providers")}
        emptyText="No probes, budgets, or auth checks — the ledger is empty and no harness was probed."
        testid="providers"
      >
        {null}
      </ViewShell>
    );
  }
  return (
    <ViewShell
      title="Providers"
      count={feed.budgets.length}
      staleness={feed.staleness}
      state="ready"
      error={error}
      onRetry={() => onRetry("providers")}
      emptyText=""
      testid="providers"
    >
      <ProvidersBody feed={feed} />
    </ViewShell>
  );
}
