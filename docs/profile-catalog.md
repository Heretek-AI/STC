# Pi Profile Catalog (issue #13)

Five task profiles as `RolePack`s (`RolePack::pi_library()` in
`studio-core/src/roles/mod.rs`). Authored once against the omp
`AgentDefinition` contract; emitted to 3 targets via `emit_pack`
(omp drop + base-pi shim + `.opencode` mirror); pinned via
`rolepack.yaml/.lock + verify_lock`.

Provenance: prompts adapted from `review/STC_ROLEPACK_SPECIFICATION.md`
(§0 shared conventions + §§1–5 role text, hand-reviewed templates).
Bodies are native STC authoring, not copied peer content — stub origins:
none beyond the spec; no unverified-template-origin material.

## Profiles

| Profile | Purpose | Model slot | Harness | Tier rationale |
|---|---|---|---|---|
| coder-web | Flagship web coding (implement declared task, evidence before claims) | `coder.primary` | omp | Flagship tier; auto-routing forbidden |
| reviewer-security | OWASP/STRIDE review, severity-rated file:line findings (§2.2) | `reviewer` | pi | Workhorse tier; auto-routing forbidden |
| researcher-docs | Source-of-record research + docs authoring (§4.5) | `researcher` | pi | Economy tier; pool fallback allowed |
| planner-spec | Specs/RFCs/interfaces, proposal-only, never implementation (§1.1) | `planner` | pi | Economy tier; pool fallback allowed |
| qa-tests | Failing-test-first synthesis, red→green demonstrated (§2.3) | `qa` | pi | Workhorse tier; pool fallback allowed |

## $/task routing note (pi-first justification)

Pi is the custom-agent substrate (most configurable) and the cheapest
adequate runner for bounded profiles: reviewer/researcher/planner/qa prompts
are narrow enough for economy/workhorse tiers, reserving flagship spend for
`coder.primary` execution. Measured $/task per profile is pending live-model
measurement (blocked:credentials) — when credentials exist, record routing
decisions from the token ledger (`billed_kind` per session) and revisit tiers.

## Verification

```bash
cargo test -p studio-core roles   # parity + manifest + secrets + roundtrip + emit/verify + lock per pack
```

`pi_library_packs_are_coherent` asserts all five packs pass
`check_parity + check_manifest + check_no_raw_secrets`, survive save/load
roundtrip, emit + verify across all 3 targets, and verify `--locked`.
`pi_library_spawns_never_escalate` asserts no write-class inherited scopes.
