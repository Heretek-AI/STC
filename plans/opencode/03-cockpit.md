# Studio — 03 Cockpit Design System & Views (Phase 3)

Philosophy: industrial-minimal, IDE-grade density. Quiet chrome, hosted tools (Monaco/xterm/Markdown) recede. Reuse > invention; hierarchy by weight/color not size.

## 1. Tokens (adopt, don’t invent)

- Surfaces (Paseo `theme.ts`, Orca `main.css`, AO `tokens.css` oklch dark-first): `--bg-primary/secondary/tertiary/elevated/sidebar + --border(7% white dark) + --input(4%)`; `--text-primary/muted/passive`; editor-surface darker than card for Monaco.
- Status single-source + `StatusBadge` on `surface3+border` shell: `working #60a5fa/#2563eb, needs-you #f06445/#c2412d, validating #facc15/#ca8a04, in-review #f59e0b/#d97706, ready #4ade80/#16a34a, merged #c084fc/#9333ea, idle muted`. Dots separate band if 6pt dots (Paseo `getStatusDotColor`).
- Diff: light `added #587c0c/modified #895503/deleted #ad0707/renamed #007acc/untracked #007100`, dark `added #81b88b/modified #e2c08d/deleted #c74e39/renamed #73c991`; ground/gutter `color-mix 13%/26% light, 16%/30% dark added; 11%/22% light 18%/32% dark removed`. Graph lanes 6 fixed. Terminal ANSI independent from status; `minimumContrastRatio` for xterm.
- Type: Geist/Inter UI + JetBrains/Geist Mono literal; body 13-14px (content 15-16 native), meta 11px uppercase 600 0.05em, sidebar 13px; `letter-spacing 0.01em`; icons lucide `size-4` (dense `3/3.5`), spinner `Loader2`. Radius controls 6, panels 8, dialogs 12+, badges full. Motion 120/150 + spin/pulse only; toast 160-200ms rise.
- Spacing 0,2,4,6,8,12,16,24,32,48,64; controls xs28/sm32/md44 (tight28/compact32/field44, header 26/36, mobile 56); sidebar 240-280 (drag 180-480), inspector 340-500 (max 55%), browser 460-900 (max 68%), reading col 720, composer 820, board cards 286px.
- Rule: `color-mix(in srgb, var(--primary)12%, var(--background))` for tints, never new hex; surface+foreground paired; open row 3:1 via lifted accent; `transition-colors` only; one primary CTA/surface, destructive in confirm only.

## 2. Layouts

### War Room (fleet)
AO `SessionsBoard` + iPolloWork `ProjectBoard` + AoE `Dashboard`: lanes `building→validating→in-review→ready` + archive overlay (not height shift); card title14 semibold clamp-2 + status pill + usage/cost `12.3k/200k (6%)·$0.42` + assignee/due + token gauge; column header h-11 1px tone bar + count + `+`; empty dashed CTA; drag `effectAllowed move + position+1024`, execution-bound locks drag/open. Topbar health/provisioning banners; token velocity sparkline; DAG `level→column NODE_WIDTH+COLUMN_GAP` centered Y.

### Agent Detail / Stream
Paseo pipeline + AoE structured view + Orchestrator `SessionView`:
- Daemon `AgentStreamCoalescer ≤1 msg/60ms` leading+trailing → `recordTimeline` → `agent_stream` WS → reducer queue 1 commit/frame + timer ceiling → store full text, reveal paced by backlog (`presentation.ts` per-block rows `${msg}:block:${n}`, first sight whole, `phase:streaming` snap on leave, `areLayoutItemsEquivalent` memo).
- Terminal: worker coalescer 5ms (≤1 IPC/5ms) → binary WS (2B header) → xterm batched; pooled retained terminals (`acquire/attach/detach` survives tab/fullscreen); fit coalesce quiet120/cap500 + rAF live; PTY size single claimant (`claim` transfers, others ignored); snapshot only on `256KB+4MB` gate; barriers via zero-length sentinel.
- TUI layout vertical `Min5 transcript / 3 approval / 0-1 queue / composer 1-6+1 chrome / 1 status`; popups above composer windowed 8 rows; approval shelf bordered accent/destructive; queue strip `Queued N`; composer `›` capped 6 rows; status `session·path·agent·mode·●working` + right usage + hint. Tool `✓ name·target·+a -r` collapsed ok else header+diff cap 20 lines + `… +N press o`.
- Badges: `blocked→needs you`, `typing→your draft`; avatar FSM `idle→alert→thinking→working→success→idle`, `Notification→blocked`, pane-gone→ghost 30s→archived.

### Audit & Gatekeeper
AoE `DiffPane/DiffFileList` + Orchestrator `WorkspaceDiffView/SessionInspector Reviews` + Orca diff tokens: banner (comments count + send/discard) + virtualized file list + per-file unified/split + `+a -r`; cap 20 + expand; Reviews tab PRs with merge/conflict/CI pills + reviewer terminal/chat handoff + per-hunk comment → `pr_feedback` loop. Debounce comments; `useVirtualizer translateY`.

### MCP Registry
AoE `McpServers`: sections effective/conflicts/kept-on-removal; row `name (transport) + provenance + redacted [env:,headers:] + shadows`; conflict modal AoE vs native; Keep/Drop; drift-paused notice. No separate colors — reuse status tokens. Add overflow-KV inspector + `doctor tools` parity + per-role matrix toggles.

## 3. Chrome & interaction

- Sidebar list rows `surface2` selected, status dot + count + kebab (`isHovered||isNative||isCompact`); glyph rails not boxes; hit areas grow outward; rows touch, dividers only; `<SettingsSection>` owns margin; `space-y-3` section / `space-y-2` compact.
- Overlays: Tooltip icon-only, HoverCard rich, Dropdown click, ContextMenu right-click, Popover arbitrary, Dialog decision, Sheet edge, Command+Popover searchable, sonner transient, Badge persistent. Wide popover + 5pt flyout 90/260ms; compact sheet + page push. Portal vs Modal with width contract, two-measure flash guard, keyboard shift host-relative.
- `useIsCompactFormFactor()` one check; list+detail 320px + detail min 400; tabs collapse compact / split desktop; `<440px` icons-only via RO.
- UX durations: 0-100ms none, 100ms-1s disabled only, 1-3s disabled+spinner, 3s+ stage labels; pre-reserve width; defer visible loading ~200ms for SSH, bind disabled immediately; ESC interrupts running.
- Forms: label + `text-xs muted` in `space-y-1`; trailing meta 11px muted; whole-list replace on patch (no merge).

Starter:
```ts
MAX_CONTENT_WIDTH=820; READING_COL=720; SIDEBAR=280; INSPECTOR=500/55%; BROWSER=900/68%
CONTROL_HEIGHTS={xs:28,sm:32,md:44}
<div flex h-full><CenterPane flex-1 min-w560>terminal+composer</><InspectorRail var(--ao-inspector-w)/></div>
<SessionsBoardGridView columns=[building,validating,inReview,ready] card={title,statusPill,usage}/>
<CommentsBanner/><DiffFileList virtualized previewLimit/>
<McpServers effective/conflicts/kept/>
```
