// Staleness badge: renders the server-computed fresh/stale/lost taxonomy.
// The label text comes from the DTO verbatim (ms1_gate.sh asserts on it);
// the class only colors the state word. Time-stale vs sequence-lost are
// visually distinct: stale = amber, lost = red + re-sync hint.
import type { StalenessDto } from "../api-types";

const CLASS: Record<StalenessDto["state"], string> = {
  Fresh: "badge-fresh",
  Stale: "badge-stale",
  Lost: "badge-lost",
};

export default function StalenessBadge({ value }: { value: StalenessDto }) {
  return (
    <span
      className={`staleness ${CLASS[value.state]}`}
      data-state={value.state}
      title={
        value.state === "Lost"
          ? "sequence-lost: the view fell behind the event ring — re-syncing from head"
          : value.state === "Stale"
            ? "time-stale: last successful DB read is older than budget"
            : "fresh: live projection data"
      }
    >
      {value.label}
    </span>
  );
}
