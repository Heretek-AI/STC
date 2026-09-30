// War Room: attention-first task buckets (f05-paseo-buckets clean-room,
// Apache-2.0) + permission notification kinds. Bucket order is server-
// computed (needs_input > failed > running > attention > done); the view
// renders that order verbatim. Desktop notification is an opt-in attention
// payload pattern only (Notification API, no mobile code, no push infra).
// Small single-branch pieces (complexity gate).
import { useEffect, useRef, useState } from "react";
import type { BucketDto, NotificationDto, WarRoomDto } from "../api-types";
import ViewShell from "../components/ViewShell";
import type { ViewName } from "../hooks/useProjection";

const BUCKET_CLASS: Record<string, string> = {
  needs_input: "badge-lost",
  failed: "badge-lost",
  running: "badge-fresh",
  attention: "badge-stale",
  done: "badge-fresh",
};

function bucketClass(name: string): string {
  return BUCKET_CLASS[name] ?? "badge-fresh";
}

function pendingTop(
  top: NotificationDto | undefined,
  state: string | null,
  seen: Set<string>
): NotificationDto | null {
  if (top === undefined) return null;
  if (state !== "on") return null;
  if (seen.has(top.id)) return null;
  return top;
}

function useAttentionNotify(top: NotificationDto | undefined): {
  state: string | null;
  enable: () => void;
} {
  const [state, setState] = useState<string | null>(null);
  const seen = useRef<Set<string>>(new Set());
  useEffect(() => {
    const next = pendingTop(top, state, seen.current);
    if (next === null) return;
    seen.current.add(next.id);
    try {
      new Notification("STUDIO needs input", { body: next.label });
    } catch {
      setState("blocked: browser denied desktop notification");
    }
  }, [top, state]);
  return { state, enable: () => grantNotification(setState) };
}

function grantNotification(setState: (s: string) => void) {
  if (Notification.permission === "granted") {
    setState("on");
    return;
  }
  if (Notification.permission === "denied") {
    setState("blocked: browser denied desktop notification");
    return;
  }
  Notification.requestPermission().then((p) =>
    setState(p === "granted" ? "on" : `permission ${p}`)
  );
}

function NotifyOptIn({ feed }: { feed: WarRoomDto }) {
  const { state, enable } = useAttentionNotify(feed.notifications[0]);
  if (!("Notification" in window)) {
    return <p className="dim">desktop notification unsupported here.</p>;
  }
  if (state !== null) return <p className="dim">desktop attention: {state}.</p>;
  return (
    <button
      type="button"
      onClick={enable}
      title="Opt in to desktop attention payloads for needs_input items (pattern only — no push infra)"
    >
      Notify me on needs_input
    </button>
  );
}

function NotificationList({ items }: { items: NotificationDto[] }) {
  return (
    <>
      {items.map((n) => (
        <div className="row" key={n.id}>
          <span className="pill">{n.kind}</span> <code>{n.task_id}</code>{" "}
          <span className="dim">{n.tool}</span> <span>{n.label}</span>
        </div>
      ))}
    </>
  );
}

function BucketSection({ bucket }: { bucket: BucketDto }) {
  if (bucket.tasks.length === 0) {
    return (
      <section>
        <BucketHead bucket={bucket} />
        <p className="empty">bucket clear.</p>
      </section>
    );
  }
  return (
    <section>
      <BucketHead bucket={bucket} />
      {bucket.tasks.map((t) => (
        <div className="row" key={t.id}>
          <code>{t.id}</code> <span className="dim">{t.kind}</span>{" "}
          <span className="pill">{t.status}</span>
        </div>
      ))}
    </section>
  );
}

function BucketHead({ bucket }: { bucket: BucketDto }) {
  return (
    <h3>
      <span className={`staleness ${bucketClass(bucket.name)}`}>
        {bucket.name}
      </span>{" "}
      <span className="count num">{bucket.tasks.length}</span>
    </h3>
  );
}

function WarRoomBody({ feed }: { feed: WarRoomDto }) {
  return (
    <>
      <div className="row">
        <span className="dim">attention queue</span>{" "}
        <span className="num">{feed.notifications.length}</span>{" "}
        <NotifyOptIn feed={feed} />
      </div>
      <NotificationList items={feed.notifications} />
      {feed.buckets.map((b) => (
        <BucketSection key={b.name} bucket={b} />
      ))}
    </>
  );
}

export default function WarRoomView({
  feed,
  error,
  onRetry,
}: {
  feed: WarRoomDto | null;
  error: string | null;
  onRetry: (name: ViewName) => void;
}) {
  if (feed === null) {
    return (
      <ViewShell
        title="War Room"
        count={null}
        staleness={null}
        state={error === null ? "loading" : "error"}
        error={error}
        onRetry={() => onRetry("war-room")}
        emptyText=""
        testid="war-room"
      >
        {null}
      </ViewShell>
    );
  }
  const total = feed.buckets.reduce((n, b) => n + b.tasks.length, 0);
  const items = total + feed.notifications.length;
  if (items === 0) {
    return (
      <ViewShell
        title="War Room"
        count={0}
        staleness={feed.staleness}
        state="empty"
        error={error}
        onRetry={() => onRetry("war-room")}
        emptyText="No tasks in any bucket — dispatch from the manager to fill the board."
        testid="war-room"
      >
        {null}
      </ViewShell>
    );
  }
  return (
    <ViewShell
      title="War Room"
      count={total}
      staleness={feed.staleness}
      state="ready"
      error={error}
      onRetry={() => onRetry("war-room")}
      emptyText=""
      testid="war-room"
    >
      <WarRoomBody feed={feed} />
    </ViewShell>
  );
}
