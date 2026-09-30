// Browser-first API client (P04 read-only + P05 A1 decide).
// Reads: GET only. The ONE write is POST /api/ask/:id/decide (one-tap
// decision through the P02 approval service CAS); every other POST/PUT/
// DELETE/PATCH is 405 by construction (pinned by write_surface_is_decide_only).
import type {
  AgentStreamDto,
  AskFeedDto,
  AuditDto,
  DecideBody,
  DecideOutcomeDto,
  HealthDto,
  HistoryDto,
  McpRegistryDto,
  ProvidersDto,
  RunsDto,
  StatusDto,
  WarRoomDto,
} from "./api-types";

async function throwForError(res: Response): Promise<never> {
  const body = await res.json().catch(() => ({}));
  const detail =
    typeof body.detail === "string" ? body.detail : `HTTP ${res.status}`;
  throw new Error(`${body.error ?? "request_failed"}: ${detail}`);
}

async function getJson<T>(path: string): Promise<T> {
  const res = await fetch(path, { headers: { Accept: "application/json" } });
  if (!res.ok) await throwForError(res);
  return (await res.json()) as T;
}

export const fetchStatus = () => getJson<StatusDto>("/api/status");
export const fetchAsk = () => getJson<AskFeedDto>("/api/ask");
export const fetchHealth = () => getJson<HealthDto>("/api/health");
export const fetchEventsSince = (since: number) =>
  getJson<HistoryDto>(`/api/events?since=${since}&limit=128`);
export const fetchWarRoom = () => getJson<WarRoomDto>("/api/war-room");
export const fetchAgentStream = (since: number) =>
  getJson<AgentStreamDto>(`/api/agent-stream?since=${since}&limit=128`);
export const fetchAudit = () => getJson<AuditDto>("/api/audit");
export const fetchMcp = () => getJson<McpRegistryDto>("/api/mcp");
export const fetchProviders = () => getJson<ProvidersDto>("/api/providers");
export const fetchRuns = () => getJson<RunsDto>("/api/runs");

export async function postDecide(
  id: string,
  body: DecideBody
): Promise<DecideOutcomeDto> {
  const res = await fetch(`/api/ask/${encodeURIComponent(id)}/decide`, {
    method: "POST",
    headers: { "Content-Type": "application/json", Accept: "application/json" },
    body: JSON.stringify(body),
  });
  if (!res.ok) await throwForError(res);
  return (await res.json()) as DecideOutcomeDto;
}
