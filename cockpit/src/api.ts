// Browser-first API client (P04). Read-only: GET only — there are no write
// endpoints, so there is nothing here that can mutate server state.
import type { AskFeedDto, HealthDto, HistoryDto, StatusDto } from "./api-types";

async function getJson<T>(path: string): Promise<T> {
  const res = await fetch(path, { headers: { Accept: "application/json" } });
  if (!res.ok) {
    const body = await res.json().catch(() => ({}));
    const detail =
      typeof body.detail === "string" ? body.detail : `HTTP ${res.status}`;
    throw new Error(`${body.error ?? "request_failed"}: ${detail}`);
  }
  return (await res.json()) as T;
}

export const fetchStatus = () => getJson<StatusDto>("/api/status");
export const fetchAsk = () => getJson<AskFeedDto>("/api/ask");
export const fetchHealth = () => getJson<HealthDto>("/api/health");
export const fetchEventsSince = (since: number) =>
  getJson<HistoryDto>(`/api/events?since=${since}&limit=128`);
