# Approval relay transport decision (issue #5)

Question: how does an Ask-policy approval reach a phone (one-tap
approve/deny) and return a ledger receipt?

## Candidates (peer-anchored)

- **Self-hosted relay (Happier-style, SSE + poll + ack queue):** codeagent
  `command-relay.service.ts` (SSE primary, poll fallback 2s→30s, ack-before-
  dispatch, 600s queue expiry as `command_expired` event, per-pairing HMAC
  headers). Full control, most work, phone app required.
- **ntfy:** agent-deck `internal/watcher/ntfy.go` (topic + NDJSON stream,
  `since=` resume, exp backoff 2s→30s; threat model = topic secrecy).
  Zero infra, existing phone apps (FCM-backed), plain HTTP both legs.
- **Tailscale / ZeroTier:** transport only (hermes-agent `tailscale serve`
  pattern), not approval protocols. Private underlay: no public relay, but
  needs the phone on the mesh + a decision callback endpoint.

## Measured (this repo, 2026-09-27)

- ntfy.sh publish: ~75–90ms, HTTP 200. Cache replay throttled on the public
  tier (no `since=all` replay observed) — fine for phone push (Firebase
  path), insufficient for machine-readable delivery proof.
- Self-hosted ntfy 2.23.0 (docker): publish 14ms, poll replay 40ms —
  full request → push → poll → decide → receipt roundtrip green
  (`scripts/approval_roundtrip.sh`, log `/tmp/opencode/stc-approval-roundtrip.log`).
- ZeroTier: interface live (10.121.15.33/24) but daemon CLI needs root —
  usable as underlay, not manageable from lanes.
- Ledger legs: `approval.pending` receipt + `approval.requested` event on
  request; `approval.granted/denied` + `approval.resolved` on tap;
  double-tap refused (`already decided`), unknown ids refused.

## Decision

**ntfy as the push surface, self-hosted instance for lane traffic**
(zero phone-app work, HTTP both legs, replayable for receipts), with
**Tailscale/ZeroTier as the private underlay option** when relays must stay
off public infra. Next (not this spike): signed decision-callback URL the
phone taps (replacing the `approval-decide` stand-in), per-pairing tokens
(codeagent HMAC pattern), 600s request expiry surfaced as an event.
