// Providers view: honest 4-state connect UI over the Phase 6 WS2 provider model.
// States: pick source → live ping proof → role assignment (billed account) → live status.
// Rules mirrored from studio-core/src/provider/mod.rs:
// - roles name slots, never providers (coder.primary, reviewer, researcher, planner, qa)
// - auto-routing forbidden on coder.primary + reviewer (review independence)
// - Connected is only ever rendered from a live probe (latency + model list),
//   never from a saved string; keys live in memory only, export emits
//   {{STUDIO_SECRET:label}} placeholders resolved at lane bind time.
// - UI never owns state: every badge stamps source + age; persistence is a
//   read-projection cache in localStorage that still requires re-ping.
import { useMemo, useState } from "react";
import type { CSSProperties } from "react";

// ---------------------------------------------------------------- types

type ProviderSource =
  | "opencode-go"
  | "opencode-zen"
  | "anthropic-byok"
  | "openai-byok"
  | "ollama-local";

type VerifyStatus = "idle" | "probing" | "verified" | "failed" | "stale";

interface ProbeEvidence {
  at: number; // Date.now() of a successful live probe
  latencyMs: number;
  endpoint: string;
  models: string[];
  httpStatus: number;
}

interface ProviderConnection {
  id: string;
  name: string;
  source: ProviderSource;
  baseUrl: string;
  authKind: "api_key" | "local" | "cli_delegated";
  billedAccount: string;
  status: VerifyStatus;
  evidence: ProbeEvidence | null;
  error: string | null;
}

type RoleSlotId =
  | "coder.primary"
  | "reviewer"
  | "researcher"
  | "planner"
  | "qa";

const ROLE_SLOTS: RoleSlotId[] = [
  "coder.primary",
  "reviewer",
  "researcher",
  "planner",
  "qa",
];

const CRITICAL_SLOTS: RoleSlotId[] = ["coder.primary", "reviewer"];

function isCriticalSlot(role: string): boolean {
  return (CRITICAL_SLOTS as string[]).includes(role);
}

interface TierRow {
  id: string;
  connection: string;
  model: string;
}

interface TierPreset {
  id: string;
  name: string;
  rows: TierRow[];
}

/** Raw transport result of one probe: endpoint, model list, HTTP status. */
interface ProbeTransport {
  endpoint: string;
  models: string[];
  httpStatus: number;
}

interface DiffRow {
  kind: "same" | "added" | "removed";
  text: string;
}

// ---------------------------------------------------------------- catalog

const SOURCE_META: Record<
  ProviderSource,
  {
    label: string;
    defaultBaseUrl: string;
    authKind: ProviderConnection["authKind"];
    billedNote: string;
    probeHint: string;
    needsKey: boolean;
  }
> = {
  "opencode-go": {
    label: "OpenCode Go",
    defaultBaseUrl: "https://go.opencode.ai/v1",
    authKind: "api_key",
    billedNote: "Billed to your OpenCode Go API account",
    probeHint: "GET {base}/models with Bearer key (OpenAI-compatible)",
    needsKey: true,
  },
  "opencode-zen": {
    label: "OpenCode Zen",
    defaultBaseUrl: "https://zen.opencode.ai/v1",
    authKind: "api_key",
    billedNote: "Billed to your OpenCode Zen API account",
    probeHint: "GET {base}/models with Bearer key (OpenAI-compatible)",
    needsKey: true,
  },
  "anthropic-byok": {
    label: "Anthropic BYOK",
    defaultBaseUrl: "https://api.anthropic.com/v1",
    authKind: "api_key",
    billedNote: "Billed to your Anthropic API account",
    probeHint: "GET {base}/models with x-api-key + anthropic-version 2023-06-01",
    needsKey: true,
  },
  "openai-byok": {
    label: "OpenAI BYOK",
    defaultBaseUrl: "https://api.openai.com/v1",
    authKind: "api_key",
    billedNote: "Billed to your OpenAI API account",
    probeHint: "GET {base}/models with Bearer key",
    needsKey: true,
  },
  "ollama-local": {
    label: "Ollama / Local",
    defaultBaseUrl: "http://localhost:11434",
    authKind: "local",
    billedNote: "Local inference — no billed account",
    probeHint: "GET {base}/api/tags (no key)",
    needsKey: false,
  },
};

const SOURCE_ORDER: ProviderSource[] = [
  "opencode-go",
  "opencode-zen",
  "anthropic-byok",
  "openai-byok",
  "ollama-local",
];

const STALE_AFTER_MS = 24 * 60 * 60 * 1000;

const DEFAULT_TIERS: TierPreset[] = [
  {
    id: "max-first",
    name: "Max-first",
    rows: [
      { id: "r1", connection: "claude-max", model: "claude-opus-4-6" },
      { id: "r2", connection: "zen-flagship", model: "zen-flagship" },
    ],
  },
  {
    id: "api-cheap",
    name: "API-cheap",
    rows: [
      { id: "r1", connection: "opencode-go", model: "qwen3-coder" },
      { id: "r2", connection: "zen-economy", model: "zen-economy" },
    ],
  },
  {
    id: "local-only",
    name: "Local-only",
    rows: [{ id: "r1", connection: "ollama-local", model: "qwen3:8b" }],
  },
];

// ---------------------------------------------------------------- small helpers

function uid(prefix: string): string {
  return `${prefix}-${Math.random().toString(36).slice(2, 8)}`;
}

function fmtAge(ms: number): string {
  const s = Math.floor(ms / 1000);
  if (s < 60) return `${s}s ago`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.floor(m / 60);
  if (h < 48) return `${h}h ago`;
  return `${Math.floor(h / 24)}d ago`;
}

function stalenessOf(evidence: ProbeEvidence | null): {
  label: string;
  stale: boolean;
} {
  if (!evidence) return { label: "NEVER VERIFIED", stale: true };
  const age = Date.now() - evidence.at;
  if (age > STALE_AFTER_MS)
    return { label: `STALE · verified ${fmtAge(age)} (>24h)`, stale: true };
  return { label: `FRESH · verified ${fmtAge(age)}`, stale: false };
}

function secretPlaceholder(label: string): string {
  return `{{STUDIO_SECRET:${label}}}`;
}

function secretLabelFor(conn: ProviderConnection): string {
  return `${conn.id}-key`;
}

/** localStorage is a read-projection cache only; quota failures are ignored. */
function writeCache(key: string, value: unknown): void {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    /* quota — UI cache only */
  }
}

// ---------------------------------------------------------------- probe model parsing

function openAIModelId(m: { id?: string } | string): string {
  if (typeof m === "string") return m;
  return m.id ?? "";
}

/** Parse an OpenAI-compatible `{ data: [{id} | string] }` model list. */
function parseOpenAIModelList(body: unknown): string[] {
  const data = (body as { data?: ({ id?: string } | string)[] }).data ?? [];
  return data.map(openAIModelId).filter(Boolean);
}

function ollamaModelName(m: { name?: string }): string {
  return m.name ?? "";
}

/** Parse an Ollama `{ models: [{name}] }` tag list. */
function parseOllamaModelList(body: unknown): string[] {
  const models = (body as { models?: { name?: string }[] }).models ?? [];
  return models.map(ollamaModelName).filter(Boolean);
}

// ---------------------------------------------------------------- probe transport

async function fetchJson(
  endpoint: string,
  init: RequestInit,
): Promise<{ res: Response; body: unknown }> {
  const res = await fetch(endpoint, init);
  const body: unknown = await res.json();
  return { res, body };
}

function requireOk(res: Response, endpoint: string): void {
  if (!res.ok) throw new Error(`HTTP ${res.status} from ${endpoint}`);
}

function requireApiKey(apiKey: string, message: string): void {
  if (!apiKey) throw new Error(message);
}

function anthropicHeaders(apiKey: string): Record<string, string> {
  return { "x-api-key": apiKey, "anthropic-version": "2023-06-01" };
}

function bearerHeaders(apiKey: string): Record<string, string> {
  return { Authorization: `Bearer ${apiKey}` };
}

async function probeModelListEndpoint(
  endpoint: string,
  init: RequestInit,
  parse: (body: unknown) => string[],
): Promise<ProbeTransport> {
  const { res, body } = await fetchJson(endpoint, init);
  requireOk(res, endpoint);
  return { endpoint, models: parse(body), httpStatus: res.status };
}

async function probeOllama(
  base: string,
  signal: AbortSignal,
): Promise<ProbeTransport> {
  return probeModelListEndpoint(`${base}/api/tags`, { signal }, parseOllamaModelList);
}

async function probeAnthropic(
  base: string,
  apiKey: string,
  signal: AbortSignal,
): Promise<ProbeTransport> {
  requireApiKey(apiKey, "API key required for Anthropic BYOK probe");
  const endpoint = `${base}/models`;
  const init = { signal, headers: anthropicHeaders(apiKey) };
  return probeModelListEndpoint(endpoint, init, parseOpenAIModelList);
}

async function probeOpenAICompatible(
  base: string,
  apiKey: string,
  signal: AbortSignal,
): Promise<ProbeTransport> {
  requireApiKey(apiKey, "API key required for probe");
  const endpoint = `${base}/models`;
  const init = { signal, headers: bearerHeaders(apiKey) };
  return probeModelListEndpoint(endpoint, init, parseOpenAIModelList);
}

async function runSourceProbe(
  source: ProviderSource,
  base: string,
  apiKey: string,
  signal: AbortSignal,
): Promise<ProbeTransport> {
  if (source === "ollama-local") return probeOllama(base, signal);
  if (source === "anthropic-byok") return probeAnthropic(base, apiKey, signal);
  return probeOpenAICompatible(base, apiKey, signal);
}

function normalizeProbeError(e: unknown): Error {
  if ((e as Error).name === "AbortError")
    return new Error("probe timed out after 15s");
  return e instanceof Error ? e : new Error(String(e));
}

/** Actual model probe. Returns evidence on HTTP 2xx + parseable model list. */
async function probeLive(
  source: ProviderSource,
  baseUrl: string,
  apiKey: string,
): Promise<ProbeEvidence> {
  const base = baseUrl.replace(/\/$/, "");
  const ctrl = new AbortController();
  const timeout = setTimeout(() => ctrl.abort(), 15000);
  const t0 = performance.now();
  try {
    const transport = await runSourceProbe(source, base, apiKey, ctrl.signal);
    return {
      at: Date.now(),
      latencyMs: Math.round(performance.now() - t0),
      ...transport,
    };
  } catch (e) {
    throw normalizeProbeError(e);
  } finally {
    clearTimeout(timeout);
  }
}

// ---------------------------------------------------------------- export doc + diff

function exportedModels(conn: ProviderConnection): string[] {
  if (conn.evidence && conn.evidence.models.length > 0)
    return conn.evidence.models;
  return ["<re-ping to fill model list>"];
}

function connectionExportBlock(conn: ProviderConnection): Record<string, unknown> {
  const block: Record<string, unknown> = {
    base_url: conn.baseUrl,
    models: exportedModels(conn),
  };
  if (conn.authKind === "api_key")
    block["api_key"] = secretPlaceholder(secretLabelFor(conn));
  return block;
}

/** Build the harness config export (placeholders only, never raw keys). */
function buildExportDoc(
  conns: ProviderConnection[],
  assignments: Record<RoleSlotId, string>,
): string {
  const providers: Record<string, unknown> = {};
  for (const c of conns) providers[c.id] = connectionExportBlock(c);
  return JSON.stringify(
    { providers, role_slots: assignments },
    null,
    2,
  );
}

/** Minimal line diff for the export preview (added/removed/unchanged). */
function diffLines(before: string, after: string): DiffRow[] {
  const beforeLines = before.split("\n");
  const beforeSet = new Set(beforeLines);
  const afterLines = after.split("\n");
  const afterSet = new Set(afterLines);
  const removed: DiffRow[] = beforeLines
    .filter((line) => !afterSet.has(line))
    .map((line) => ({ kind: "removed" as const, text: line }));
  const addedOrSame: DiffRow[] = afterLines.map((line) =>
    beforeSet.has(line)
      ? { kind: "same" as const, text: line }
      : { kind: "added" as const, text: line },
  );
  return [...removed, ...addedOrSame];
}

function diffRowColor(kind: DiffRow["kind"]): string {
  if (kind === "added") return "var(--diff-added)";
  if (kind === "removed") return "var(--diff-deleted)";
  return "var(--text-1)";
}

function diffRowPrefix(kind: DiffRow["kind"]): string {
  if (kind === "added") return "+ ";
  if (kind === "removed") return "- ";
  return "  ";
}

// ---------------------------------------------------------------- import preview (read-only)

function parseImportDoc(text: string): {
  providers?: Record<string, unknown>;
} {
  return JSON.parse(text) as { providers?: Record<string, unknown> };
}

function providerNamesOf(doc: unknown): string[] {
  const record = doc as { providers?: Record<string, unknown> };
  return Object.keys(record.providers ?? {});
}

function importParseError(e: unknown): string {
  const detail = e instanceof Error ? e.message : String(e);
  return `Invalid JSON: ${detail}`;
}

function extractProviderNames(
  importText: string,
): { names: string[] } | { error: string } {
  if (!importText.trim())
    return { error: "Paste a harness config (opencode.json) to preview." };
  let names: string[];
  try {
    names = providerNamesOf(parseImportDoc(importText));
  } catch (e) {
    return { error: importParseError(e) };
  }
  if (names.length === 0)
    return { error: "No `providers` block found — nothing to import." };
  return { names };
}

// ---------------------------------------------------------------- tier preset edits

function mapTierRows(
  tiers: TierPreset[],
  tierId: string,
  mapRows: (rows: TierRow[]) => TierRow[],
): TierPreset[] {
  return tiers.map((t) =>
    t.id === tierId ? { ...t, rows: mapRows(t.rows) } : t,
  );
}

function renameTier(tiers: TierPreset[], tierId: string, name: string): TierPreset[] {
  return tiers.map((t) => (t.id === tierId ? { ...t, name } : t));
}

function addTierRow(tiers: TierPreset[], tierId: string): TierPreset[] {
  const row: TierRow = { id: uid("row"), connection: "", model: "" };
  return mapTierRows(tiers, tierId, (rows) => [...rows, row]);
}

function removeTierRow(
  tiers: TierPreset[],
  tierId: string,
  rowId: string,
): TierPreset[] {
  return mapTierRows(tiers, tierId, (rows) =>
    rows.filter((r) => r.id !== rowId),
  );
}

function updateTierRowField(
  tiers: TierPreset[],
  tierId: string,
  rowId: string,
  patch: Partial<TierRow>,
): TierPreset[] {
  return mapTierRows(tiers, tierId, (rows) =>
    rows.map((r) => (r.id === rowId ? { ...r, ...patch } : r)),
  );
}

// ---------------------------------------------------------------- styles (oklch tokens from tokens.css)

const card: CSSProperties = {
  background: "var(--surface-1)",
  border: "1px solid var(--border)",
  borderRadius: 8,
  padding: 12,
  marginBottom: 12,
};
const h3: CSSProperties = {
  margin: "0 0 8px 0",
  fontSize: 13,
  color: "var(--text-0)",
  textTransform: "uppercase",
  letterSpacing: "0.06em",
};
const muted: CSSProperties = { color: "var(--text-dim)", fontSize: 12 };
const inputStyle: CSSProperties = {
  background: "var(--surface-0)",
  color: "var(--text-0)",
  border: "1px solid var(--border)",
  borderRadius: 6,
  padding: "6px 8px",
  fontFamily: "var(--mono)",
  fontSize: 12,
  width: "100%",
  boxSizing: "border-box",
};
const btn: CSSProperties = {
  background: "var(--surface-2)",
  color: "var(--text-0)",
  border: "1px solid var(--border)",
  borderRadius: 6,
  padding: "6px 12px",
  fontFamily: "var(--mono)",
  fontSize: 12,
  cursor: "pointer",
};
const btnPrimary: CSSProperties = {
  ...btn,
  background: "var(--status-building)",
  color: "oklch(0.18 0.01 260)",
  fontWeight: 700,
};

// ---------------------------------------------------------------- status pills + notes

const STATUS_COLORS: Record<VerifyStatus, string> = {
  verified: "var(--status-ready)",
  failed: "var(--diff-deleted)",
  probing: "var(--status-building)",
  stale: "var(--text-dim)",
  idle: "var(--text-dim)",
};

const STATUS_LABELS: Record<VerifyStatus, string> = {
  verified: "Verified (live ping)",
  failed: "Failed",
  probing: "Probing…",
  stale: "Stale — re-ping required",
  idle: "Not connected",
};

function StatusPill({ status }: { status: VerifyStatus }) {
  const color = STATUS_COLORS[status];
  return (
    <span
      style={{
        border: `1px solid ${color}`,
        color,
        borderRadius: 999,
        padding: "1px 8px",
        fontSize: 11,
      }}
    >
      {STATUS_LABELS[status]}
    </span>
  );
}

function RePingRow({
  conn,
  onPing,
}: {
  conn: ProviderConnection;
  onPing: (id: string, key: string) => void;
}) {
  const [key, setKey] = useState("");
  if (conn.authKind === "local") {
    return (
      <div style={{ marginTop: 6 }}>
        <button style={btn} onClick={() => onPing(conn.id, "")}>
          Re-run live ping
        </button>
      </div>
    );
  }
  return (
    <div style={{ display: "flex", gap: 8, marginTop: 6 }}>
      <input
        style={{ ...inputStyle, maxWidth: 260 }}
        type="password"
        value={key}
        onChange={(e) => setKey(e.target.value)}
        placeholder="API key for live re-ping (memory only)"
        aria-label={`API key for ${conn.id}`}
        autoComplete="off"
      />
      <button style={btn} onClick={() => onPing(conn.id, key)}>
        Re-run live ping
      </button>
    </div>
  );
}

function ProbeStatus({
  probeError,
  evidence,
}: {
  probeError: string | null;
  evidence: ProbeEvidence | null;
}) {
  if (probeError)
    return (
      <p style={{ color: "var(--diff-deleted)", fontSize: 12 }}>
        Probe failed: {probeError}
      </p>
    );
  if (evidence)
    return (
      <div
        style={{
          marginTop: 8,
          border: "1px solid var(--status-ready)",
          borderRadius: 6,
          padding: 8,
          fontSize: 12,
        }}
      >
        <strong style={{ color: "var(--status-ready)" }}>
          LIVE PROOF
        </strong>{" "}
        HTTP {evidence.httpStatus} ·{" "}
        {evidence.latencyMs}ms · {evidence.endpoint}
        <br />
        <span style={muted}>
          models ({evidence.models.length}):{" "}
          {evidence.models.slice(0, 8).join(", ") || "(none listed)"}
        </span>
      </div>
    );
  return (
    <p style={muted}>
      No probe yet — this draft is not connected to anything.
    </p>
  );
}

function DiffLine({ row }: { row: DiffRow }) {
  return (
    <span
      style={{
        display: "block",
        color: diffRowColor(row.kind),
      }}
    >
      {diffRowPrefix(row.kind)}
      {row.text}
    </span>
  );
}

// ---------------------------------------------------------------- role assignment rows

function BilledCell({ conn }: { conn: ProviderConnection | null }) {
  if (!conn)
    return (
      <td style={{ padding: "6px 4px", ...muted }}>
        No charge — nothing assigned
      </td>
    );
  return (
    <td style={{ padding: "6px 4px", ...muted }}>
      {conn.billedAccount}
      {conn.status !== "verified" && (
        <span style={{ color: "var(--diff-modified)" }}>
          {" "}
          · unverified (re-ping before use)
        </span>
      )}
    </td>
  );
}

function AutoRouteCell({
  critical,
  slot,
  auto,
  onAutoRoute,
}: {
  critical: boolean;
  slot: RoleSlotId;
  auto: boolean;
  onAutoRoute: (slot: RoleSlotId, value: boolean) => void;
}) {
  if (critical)
    return (
      <span
        style={muted}
        title="Auto-routing is forbidden on coder.primary and reviewer: pin one connection (review independence)."
      >
        Disabled — pin one connection
      </span>
    );
  return (
    <label style={{ ...muted, cursor: "pointer" }}>
      <input
        type="checkbox"
        checked={auto}
        onChange={(e) => onAutoRoute(slot, e.target.checked)}
        aria-label={`auto-routing for ${slot}`}
      />{" "}
      pool fallback
    </label>
  );
}

function RoleSlotRow({
  slot,
  connId,
  conn,
  connections,
  auto,
  onAssign,
  onAutoRoute,
}: {
  slot: RoleSlotId;
  connId: string;
  conn: ProviderConnection | null;
  connections: ProviderConnection[];
  auto: boolean;
  onAssign: (slot: RoleSlotId, connId: string) => void;
  onAutoRoute: (slot: RoleSlotId, value: boolean) => void;
}) {
  const critical = isCriticalSlot(slot);
  return (
    <tr key={slot} style={{ borderTop: "1px solid var(--border)" }}>
      <td style={{ padding: "6px 4px" }}>
        <code>{slot}</code>
        {critical && (
          <span style={{ ...muted, marginLeft: 6 }}>pinned</span>
        )}
      </td>
      <td>
        <select
          style={inputStyle}
          value={connId}
          onChange={(e) => onAssign(slot, e.target.value)}
          aria-label={`connection for ${slot}`}
        >
          <option value="">— unassigned —</option>
          {connections.map((c) => (
            <option key={c.id} value={c.id}>
              {c.name} ({c.status})
            </option>
          ))}
        </select>
      </td>
      <BilledCell conn={conn} />
      <td style={{ padding: "6px 4px" }}>
        <AutoRouteCell
          critical={critical}
          slot={slot}
          auto={auto}
          onAutoRoute={onAutoRoute}
        />
      </td>
    </tr>
  );
}

// ---------------------------------------------------------------- live status cards

function ConnectionCard({
  conn,
  onPing,
}: {
  conn: ProviderConnection;
  onPing: (id: string, key: string) => void;
}) {
  const st = stalenessOf(conn.evidence);
  const freshnessColor = st.stale
    ? "var(--diff-modified)"
    : "var(--status-ready)";
  return (
    <div
      key={conn.id}
      style={{
        border: "1px solid var(--border)",
        borderRadius: 6,
        padding: 8,
        marginBottom: 8,
        fontSize: 12,
      }}
    >
      <div
        style={{ display: "flex", gap: 8, alignItems: "center" }}
      >
        <strong>{conn.name}</strong>
        <span style={muted}>{SOURCE_META[conn.source].label}</span>
        <StatusPill status={conn.status} />
        <span style={{ color: freshnessColor, fontSize: 11 }}>
          {st.label}
        </span>
      </div>
      <div style={muted}>
        {conn.baseUrl} · {conn.billedAccount}
        {conn.evidence && (
          <>
            {" "}· last probe {conn.evidence.latencyMs}ms, HTTP{" "}
            {conn.evidence.httpStatus}, {conn.evidence.models.length} models
          </>
        )}
      </div>
      {conn.error && (
        <div style={{ color: "var(--diff-modified)", fontSize: 12 }}>
          {conn.error}
        </div>
      )}
      <RePingRow conn={conn} onPing={onPing} />
    </div>
  );
}

// ---------------------------------------------------------------- wizard sections

function PickSourceSection({
  picked,
  onPick,
}: {
  picked: ProviderSource;
  onPick: (s: ProviderSource) => void;
}) {
  return (
    <section style={card} data-step="pick-source">
      <h3 style={h3}>1 · Pick source</h3>
      <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
        {SOURCE_ORDER.map((s) => (
          <button
            key={s}
            style={picked === s ? btnPrimary : btn}
            onClick={() => onPick(s)}
            aria-pressed={picked === s}
          >
            {SOURCE_META[s].label}
          </button>
        ))}
      </div>
      <p style={muted}>{SOURCE_META[picked].probeHint}</p>
    </section>
  );
}

function ConnectProbeSection({
  picked,
  draftName,
  draftBaseUrl,
  apiKey,
  probing,
  probeError,
  evidence,
  onDraftName,
  onDraftBaseUrl,
  onApiKey,
  onProbe,
  onSave,
}: {
  picked: ProviderSource;
  draftName: string;
  draftBaseUrl: string;
  apiKey: string;
  probing: boolean;
  probeError: string | null;
  evidence: ProbeEvidence | null;
  onDraftName: (v: string) => void;
  onDraftBaseUrl: (v: string) => void;
  onApiKey: (v: string) => void;
  onProbe: () => void;
  onSave: () => void;
}) {
  return (
    <section style={card} data-step="connect-probe">
      <h3 style={h3}>2 · Connect with live ping proof</h3>
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "1fr 2fr",
          gap: 8,
          marginBottom: 8,
        }}
      >
        <input
          style={inputStyle}
          value={draftName}
          onChange={(e) => onDraftName(e.target.value)}
          placeholder="connection name"
          aria-label="connection name"
        />
        <input
          style={inputStyle}
          value={draftBaseUrl}
          onChange={(e) => onDraftBaseUrl(e.target.value)}
          placeholder="base URL"
          aria-label="base URL"
        />
      </div>
      {SOURCE_META[picked].needsKey && (
        <input
          style={{ ...inputStyle, marginBottom: 8 }}
          type="password"
          value={apiKey}
          onChange={(e) => onApiKey(e.target.value)}
          placeholder="API key (memory only — never saved)"
          aria-label="API key"
          autoComplete="off"
        />
      )}
      <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
        <button style={btnPrimary} onClick={onProbe} disabled={probing}>
          {probing ? "Pinging…" : "Run live ping"}
        </button>
        <button
          style={btn}
          onClick={onSave}
          disabled={!evidence}
          title={
            evidence
              ? "Save with probe evidence"
              : "Run a live ping first — never Connected-from-saved-string"
          }
        >
          Save as connection
        </button>
      </div>
      <ProbeStatus probeError={probeError} evidence={evidence} />
    </section>
  );
}

function RoleAssignmentSection({
  assignments,
  connections,
  autoRoute,
  hasVerified,
  onAssign,
  onAutoRoute,
}: {
  assignments: Record<RoleSlotId, string>;
  connections: ProviderConnection[];
  autoRoute: Record<RoleSlotId, boolean>;
  hasVerified: boolean;
  onAssign: (slot: RoleSlotId, connId: string) => void;
  onAutoRoute: (slot: RoleSlotId, value: boolean) => void;
}) {
  return (
    <section style={card} data-step="role-assignment">
      <h3 style={h3}>3 · Role assignment — which account gets billed</h3>
      <table style={{ width: "100%", fontSize: 12, borderCollapse: "collapse" }}>
        <thead>
          <tr style={{ color: "var(--text-dim)", textAlign: "left" }}>
            <th>Slot</th>
            <th>Connection</th>
            <th>Billed account</th>
            <th>Auto-routing</th>
          </tr>
        </thead>
        <tbody>
          {ROLE_SLOTS.map((slot) => {
            const connId = assignments[slot];
            const conn = connections.find((c) => c.id === connId) ?? null;
            return (
              <RoleSlotRow
                key={slot}
                slot={slot}
                connId={connId}
                conn={conn}
                connections={connections}
                auto={!!autoRoute[slot]}
                onAssign={onAssign}
                onAutoRoute={onAutoRoute}
              />
            );
          })}
        </tbody>
      </table>
      {!hasVerified && (
        <p style={muted}>
          No verified connections yet — assignments stay inert until a live
          ping proves one.
        </p>
      )}
    </section>
  );
}

function LiveStatusSection({
  connections,
  onPing,
}: {
  connections: ProviderConnection[];
  onPing: (id: string, key: string) => void;
}) {
  return (
    <section style={card} data-step="live-status">
      <h3 style={h3}>4 · Live status (24h staleness)</h3>
      {connections.length === 0 && (
        <p style={muted}>No connections. Complete steps 1–2 first.</p>
      )}
      {connections.map((c) => (
        <ConnectionCard key={c.id} conn={c} onPing={onPing} />
      ))}
    </section>
  );
}

function ImportSection({
  text,
  error,
  preview,
  onText,
  onPreview,
}: {
  text: string;
  error: string | null;
  preview: string[] | null;
  onText: (v: string) => void;
  onPreview: () => void;
}) {
  return (
    <section style={card} data-step="import">
      <h3 style={h3}>Import harness config (read-only)</h3>
      <p style={muted}>
        Paste an existing harness config to preview. Nothing changes until
        you explicitly add a connection above.
      </p>
      <textarea
        style={{ ...inputStyle, minHeight: 90, fontFamily: "var(--mono)" }}
        value={text}
        onChange={(e) => onText(e.target.value)}
        placeholder='{"providers": {...}}'
        aria-label="harness config to preview"
      />
      <div style={{ marginTop: 8 }}>
        <button style={btn} onClick={onPreview}>
          Preview import
        </button>
      </div>
      {error && (
        <p style={{ color: "var(--diff-deleted)", fontSize: 12 }}>
          {error}
        </p>
      )}
      {preview && (
        <p style={{ fontSize: 12 }}>
          Would import {preview.length} provider(s):{" "}
          <code>{preview.join(", ")}</code> — no changes made.
        </p>
      )}
    </section>
  );
}

function ExportSection({
  currentDoc,
  preview,
  diff,
  onPreview,
  onConfirm,
}: {
  currentDoc: string;
  preview: string | null;
  diff: DiffRow[] | null;
  onPreview: (doc: string) => void;
  onConfirm: () => void;
}) {
  return (
    <section style={card} data-step="export">
      <h3 style={h3}>Export to harness (explicit, diff preview)</h3>
      <p style={muted}>
        Secrets export as {secretPlaceholder("<label>")} placeholders only.
      </p>
      <div style={{ display: "flex", gap: 8, marginBottom: 8 }}>
        <button style={btn} onClick={() => onPreview(currentDoc)}>
          Preview diff
        </button>
        <button
          style={btnPrimary}
          disabled={preview === null}
          onClick={onConfirm}
        >
          Confirm export
        </button>
      </div>
      {diff && (
        <pre
          style={{
            ...inputStyle,
            maxHeight: 240,
            overflow: "auto",
            whiteSpace: "pre-wrap",
          }}
        >
          {diff.map((d, i) => (
            <DiffLine key={i} row={d} />
          ))}
        </pre>
      )}
    </section>
  );
}

function TierRowEditor({
  tierId,
  row,
  tiers,
  onTiers,
}: {
  tierId: string;
  row: TierRow;
  tiers: TierPreset[];
  onTiers: (next: TierPreset[]) => void;
}) {
  return (
    <div
      key={row.id}
      style={{
        display: "grid",
        gridTemplateColumns: "1fr 1fr auto",
        gap: 8,
        marginTop: 6,
      }}
    >
      <input
        style={inputStyle}
        value={row.connection}
        onChange={(e) =>
          onTiers(updateTierRowField(tiers, tierId, row.id, { connection: e.target.value }))
        }
        placeholder="connection id"
        aria-label="tier row connection"
      />
      <input
        style={inputStyle}
        value={row.model}
        onChange={(e) =>
          onTiers(updateTierRowField(tiers, tierId, row.id, { model: e.target.value }))
        }
        placeholder="model id"
        aria-label="tier row model"
      />
      <button
        style={btn}
        onClick={() => onTiers(removeTierRow(tiers, tierId, row.id))}
        aria-label="remove tier row"
      >
        ✕
      </button>
    </div>
  );
}

function TierEditor({
  tier,
  tiers,
  onTiers,
}: {
  tier: TierPreset;
  tiers: TierPreset[];
  onTiers: (next: TierPreset[]) => void;
}) {
  return (
    <div key={tier.id} style={{ marginBottom: 12 }}>
      <div
        style={{ display: "flex", gap: 8, alignItems: "center" }}
      >
        <input
          style={{ ...inputStyle, maxWidth: 220 }}
          value={tier.name}
          onChange={(e) => onTiers(renameTier(tiers, tier.id, e.target.value))}
          aria-label={`tier name ${tier.id}`}
        />
        <button
          style={btn}
          onClick={() => onTiers(addTierRow(tiers, tier.id))}
        >
          + row
        </button>
      </div>
      {tier.rows.map((row) => (
        <TierRowEditor
          key={row.id}
          tierId={tier.id}
          row={row}
          tiers={tiers}
          onTiers={onTiers}
        />
      ))}
    </div>
  );
}

function TierPresetsSection({
  tiers,
  onTiers,
}: {
  tiers: TierPreset[];
  onTiers: (next: TierPreset[]) => void;
}) {
  return (
    <section style={card} data-step="tiers">
      <h3 style={h3}>Tier presets (editable rows)</h3>
      {tiers.map((tier) => (
        <TierEditor key={tier.id} tier={tier} tiers={tiers} onTiers={onTiers} />
      ))}
    </section>
  );
}

function loadInitialConnections(): ProviderConnection[] {
  try {
    const raw = localStorage.getItem("stc.providers.connections");
    if (!raw) return [];
    const parsed = JSON.parse(raw) as ProviderConnection[];
    return parsed.map((c) => ({
      ...c,
      status: c.evidence ? ("stale" as VerifyStatus) : ("idle" as VerifyStatus),
      error: c.evidence
        ? "Cached — re-run live ping to reconnect (never Connected-from-saved-string)."
        : null,
    }));
  } catch {
    return [];
  }
}

const DEFAULT_ASSIGNMENTS: Record<RoleSlotId, string> = {
  "coder.primary": "",
  reviewer: "",
  researcher: "",
  planner: "",
  qa: "",
};

function loadInitialAssignments(): Record<RoleSlotId, string> {
  try {
    const raw = localStorage.getItem("stc.providers.assignments");
    if (raw) return JSON.parse(raw) as Record<RoleSlotId, string>;
  } catch {
    /* ignore */
  }
  return DEFAULT_ASSIGNMENTS;
}

const DEFAULT_AUTOROUTE: Record<RoleSlotId, boolean> = {
  "coder.primary": false,
  reviewer: false,
  researcher: false,
  planner: false,
  qa: false,
};

function loadInitialAutoRoute(): Record<RoleSlotId, boolean> {
  try {
    const raw = localStorage.getItem("stc.providers.autoroute");
    if (raw) return JSON.parse(raw) as Record<RoleSlotId, boolean>;
  } catch {
    /* ignore */
  }
  return DEFAULT_AUTOROUTE;
}

function loadInitialTiers(): TierPreset[] {
  try {
    const raw = localStorage.getItem("stc.providers.tiers");
    if (raw) return JSON.parse(raw) as TierPreset[];
  } catch {
    /* ignore */
  }
  return DEFAULT_TIERS;
}

function loadInitialLastExport(): string {
  try {
    return localStorage.getItem("stc.providers.lastExport") ?? "";
  } catch {
    return "";
  }
}

function updateConnectionStatus(
  list: ProviderConnection[],
  id: string,
  patch: Partial<ProviderConnection>,
): ProviderConnection[] {
  return list.map((c) => (c.id === id ? { ...c, ...patch } : c));
}

// ---------------------------------------------------------------- state hooks

/** Wizard draft: picked source + connection fields. Key stays memory-only. */
function useWizardDraft() {
  const [picked, setPicked] = useState<ProviderSource>("opencode-go");
  const [draftName, setDraftName] = useState("my-connection");
  const [draftBaseUrl, setDraftBaseUrl] = useState(
    SOURCE_META["opencode-go"].defaultBaseUrl,
  );
  const [apiKey, setApiKey] = useState("");
  return {
    picked,
    setPicked,
    draftName,
    setDraftName,
    draftBaseUrl,
    setDraftBaseUrl,
    apiKey,
    setApiKey,
  };
}

/** One live-probe attempt: spinner, error, and proof evidence. */
function useProbeAttempt() {
  const [probing, setProbing] = useState(false);
  const [probeError, setProbeError] = useState<string | null>(null);
  const [evidence, setEvidence] = useState<ProbeEvidence | null>(null);
  async function runProbe(
    source: ProviderSource,
    baseUrl: string,
    apiKey: string,
  ) {
    setProbing(true);
    setProbeError(null);
    try {
      setEvidence(await probeLive(source, baseUrl, apiKey));
    } catch (e) {
      setProbeError(e instanceof Error ? e.message : String(e));
      setEvidence(null);
    } finally {
      setProbing(false);
    }
  }
  return { probing, probeError, evidence, setEvidence, setProbeError, runProbe };
}

/** Read-only import draft: pasted text plus preview or error. */
function useImportDraft() {
  const [importText, setImportText] = useState("");
  const [importPreview, setImportPreview] = useState<string[] | null>(null);
  const [importError, setImportError] = useState<string | null>(null);
  function parsePreview() {
    setImportError(null);
    setImportPreview(null);
    const result = extractProviderNames(importText);
    if ("error" in result) setImportError(result.error);
    else setImportPreview(result.names);
  }
  return { importText, setImportText, importPreview, importError, parsePreview };
}

/** Explicit export flow: baseline plus pending preview. */
function useExportDraft() {
  const [lastExported, setLastExported] =
    useState<string>(loadInitialLastExport);
  const [exportPreview, setExportPreview] = useState<string | null>(null);
  function confirmExport() {
    if (exportPreview === null) return;
    writeCache("stc.providers.lastExport", exportPreview);
    setLastExported(exportPreview);
    setExportPreview(null);
  }
  return { lastExported, exportPreview, setExportPreview, confirmExport };
}

// ---------------------------------------------------------------- component

export default function Providers() {
  const draft = useWizardDraft();
  const probe = useProbeAttempt();
  const imp = useImportDraft();
  const exp = useExportDraft();

  const [connections, setConnections] = useState<ProviderConnection[]>(
    loadInitialConnections,
  );
  const [assignments, setAssignments] = useState<Record<RoleSlotId, string>>(
    loadInitialAssignments,
  );
  const [autoRoute, setAutoRoute] = useState<Record<RoleSlotId, boolean>>(
    loadInitialAutoRoute,
  );
  const [tiers, setTiers] = useState<TierPreset[]>(loadInitialTiers);

  function persistConns(next: ProviderConnection[]) {
    setConnections(next);
    writeCache("stc.providers.connections", next);
  }
  function persistAssignments(next: Record<RoleSlotId, string>) {
    setAssignments(next);
    writeCache("stc.providers.assignments", next);
  }
  function persistAutoRoute(next: Record<RoleSlotId, boolean>) {
    setAutoRoute(next);
    writeCache("stc.providers.autoroute", next);
  }
  function persistTiers(next: TierPreset[]) {
    setTiers(next);
    writeCache("stc.providers.tiers", next);
  }

  function pickSource(s: ProviderSource) {
    draft.setPicked(s);
    draft.setDraftBaseUrl(SOURCE_META[s].defaultBaseUrl);
    probe.setEvidence(null);
    probe.setProbeError(null);
  }

  function saveDraftAsConnection() {
    if (!probe.evidence) return; // never save as Connected without proof
    const meta = SOURCE_META[draft.picked];
    const name = draft.draftName.trim();
    const id = name.toLowerCase().replace(/[^a-z0-9-]+/g, "-") ||
      uid("conn");
    const conn: ProviderConnection = {
      id,
      name: name || id,
      source: draft.picked,
      baseUrl: draft.draftBaseUrl,
      authKind: meta.authKind,
      billedAccount: meta.billedNote,
      status: "verified",
      evidence: probe.evidence,
      error: null,
    };
    persistConns([...connections.filter((c) => c.id !== id), conn]);
    probe.setEvidence(null);
    draft.setApiKey("");
  }

  async function rePing(connId: string, keyForPing: string) {
    const target = connections.find((c) => c.id === connId);
    if (!target) return;
    persistConns(
      updateConnectionStatus(connections, connId, {
        status: "probing",
        error: null,
      }),
    );
    try {
      const ev = await probeLive(target.source, target.baseUrl, keyForPing);
      persistConns(
        updateConnectionStatus(connections, connId, {
          status: "verified",
          evidence: ev,
          error: null,
        }),
      );
    } catch (e) {
      persistConns(
        updateConnectionStatus(connections, connId, {
          status: "failed",
          error: e instanceof Error ? e.message : String(e),
        }),
      );
    }
  }

  const exportDoc = useMemo(
    () => buildExportDoc(connections, assignments),
    [connections, assignments],
  );
  const diff = useMemo(
    () =>
      exp.exportPreview !== null
        ? diffLines(
          exp.lastExported || "(no previous export)",
          exp.exportPreview,
        )
        : null,
    [exp.exportPreview, exp.lastExported],
  );

  const verifiedConns = connections.filter((c) => c.status === "verified");

  return (
    <div data-view="providers">
      <p style={muted}>
        source: studio.db read projection (cached in localStorage) · keys held
        in memory only · Connected requires a live probe
      </p>

      {/* ---- State 1: pick source ---- */}
      <PickSourceSection picked={draft.picked} onPick={pickSource} />

      {/* ---- State 2: connect with live ping proof ---- */}
      <ConnectProbeSection
        picked={draft.picked}
        draftName={draft.draftName}
        draftBaseUrl={draft.draftBaseUrl}
        apiKey={draft.apiKey}
        probing={probe.probing}
        probeError={probe.probeError}
        evidence={probe.evidence}
        onDraftName={draft.setDraftName}
        onDraftBaseUrl={draft.setDraftBaseUrl}
        onApiKey={draft.setApiKey}
        onProbe={() =>
          probe.runProbe(draft.picked, draft.draftBaseUrl, draft.apiKey)}
        onSave={saveDraftAsConnection}
      />

      {/* ---- State 3: role assignment (billed account per slot) ---- */}
      <RoleAssignmentSection
        assignments={assignments}
        connections={connections}
        autoRoute={autoRoute}
        hasVerified={verifiedConns.length > 0}
        onAssign={(slot, connId) =>
          persistAssignments({ ...assignments, [slot]: connId })
        }
        onAutoRoute={(slot, value) =>
          persistAutoRoute({ ...autoRoute, [slot]: value })
        }
      />

      {/* ---- State 4: live status with 24h staleness ---- */}
      <LiveStatusSection connections={connections} onPing={rePing} />

      {/* ---- Import (read-only) ---- */}
      <ImportSection
        text={imp.importText}
        error={imp.importError}
        preview={imp.importPreview}
        onText={imp.setImportText}
        onPreview={imp.parsePreview}
      />

      {/* ---- Export (explicit + diff) ---- */}
      <ExportSection
        currentDoc={exportDoc}
        preview={exp.exportPreview}
        diff={diff}
        onPreview={exp.setExportPreview}
        onConfirm={exp.confirmExport}
      />

      {/* ---- Tier presets as editable rows ---- */}
      <TierPresetsSection tiers={tiers} onTiers={persistTiers} />
    </div>
  );
}
