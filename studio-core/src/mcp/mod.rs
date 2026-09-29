//! Dynamic MCP gateway / Tool Lens (phase 03).
//!
//! Clean-room gateway behind the Q9 bar. Fail-closed posture throughout:
//! unknown tools deny typed, never silently widen; full schemas load
//! on demand via [`registry::Registry::tool_open`] (the index carries
//! name + description only, never schemas); every refusal is a typed
//! [`registry::RegistryError`] variant.
//!
//! Harvest provenance (no source copied; rules re-derived):
//!
//! - Approval tiers + object policies + unknown→exec + MCP=write (oh-my-pi,
//!   MIT) behind citation c014
//!   (`f6e4638194f716ab52c1e23fd3dcfd8cbc74fe4ec5e8263fe8449ea7d78ea59f`):
//!   see [`tiers`].
//! - MCP adapter charter — manifest→capabilities, admission ceilings,
//!   schema/description bounds, tool-name grammar, typed failures, single
//!   egress seam (nearai/ironclaw, Apache-2.0 dual) behind citations c015
//!   (`7ad76256c19b661dfe96a07af5d1a8b8eb82f035b7a5483c819721e3ef8af4fc`)
//!   and c016
//!   (`3e283a44f89a1936337e7c53dc67ee58ab647b3db7a3c0b85921e4c15adeba51`):
//!   see [`registry`].
//! - Tool-lens primitives — enable/disable keys+tags (disabled = unlisted +
//!   uncallable), search-tools transform, version-range filters
//!   (jlowin/fastmcp, Apache-2.0) behind citations c017
//!   (`329fd541c0ad35de30fb80a5f7cf8be90523213aa2647678c8117bf7f562784`),
//!   c018 (`27d825c8e4e3506303caabf851ef575c32132005bd7ad8cd8ac943fd67d3a641`),
//!   c019 (`f4d13cb0bcadbb3dd16b0ead778cfe229ac05bb15ff2ebe028d03162dbcd0640`):
//!   see [`lens`].
//! - Schema-to-CLI generation + SKILL.md companion (jlowin/fastmcp,
//!   Apache-2.0) behind citation c020
//!   (`dce670f72e1b281fa22214308b05f249371fe286c8bb7d394490832e835ee38e`):
//!   see [`cli`].
//! - Cross-harness MCP config read/write (JSON/TOML/JSONC) with comment
//!   preservation (BloopAI/vibe-kanban, Apache-2.0) behind citation c021
//!   (`4d6e1db7f9f9703135cb88d9db52eaae3376f542d71f837fe5bb083db756ae73`):
//!   see [`config`]. Lockfile-neutral: hand-rolled subset parsers, no new
//!   dependencies.
//! - Composable minimal ACI tool bundles + submit/review gate as YAML config
//!   (SWE-agent/SWE-agent, MIT) behind citations c022
//!   (`96aeb863cbfaa768044527155f8555c9b401c6644e937b5b6b0bba5538b6eee4`)
//!   and c023
//!   (`91a7a214299997fa28b3988a911cef4640e99b69e87ba78fa1e03d5b0c979c51`):
//!   see [`bundles`]. Lockfile-neutral: hand-rolled YAML subset, no new deps.
//! - Tool-name wildcard grammar with hard bounds (openchamber/openchamber,
//!   MIT) behind citation c024
//!   (`54a88c0c137eba522e05e199b0b5d75a65ee5793387a9e8c4d768fa3654b792b`):
//!   see [`toolmatch`].
//! - MCP process pool lifecycle + FD-leak regression tests
//!   (asheshgoplani/agent-deck, MIT) behind citations c025
//!   (`b037e3a1d7ca6dc1fe23845eff87ca58cc9351e8ad5fd4f0f0cd91204f42bf65`)
//!   and c026
//!   (`00bd17ff74dee9170bdbf9c28beea40999a28fb883193e4294001843c2b5b315`):
//!   see [`pool`].
//! - Vector V3 Tool Misbehavior Fixture Farm (in-repo stdio-shaped fixtures,
//!   in-process, no network): see [`fixtures`]. Charter honesty note: the
//!   farm SIMULATES hostile behaviors per the no-network charter — hang is
//!   a deadline-exceeding wait, secret-echo a canned credential the
//!   redactor must catch, egress a denied argument, rugpull a pinned
//!   schema changed mid-session. What the farm ports is the BEHAVIORAL
//!   CONTRACT (every hostile outcome a typed denial/timeout/redaction),
//!   not subprocess or socket machinery; nothing here touches the network.
//!
//! Two-tier bound, two rationales (QA-R1 fix 7, bound option): the tool
//! index cap (`MAX_INDEX_BYTES`) bounds per-call CONTEXT (what a model
//! sees — name + description only, schemas never counted); the schema
//! store cap (`MAX_SCHEMA_STORE_BYTES`) bounds process MEMORY (what the
//! gateway holds across all manifests). Per-tool schema bytes are
//! ceilinged by `MAX_SCHEMA_BYTES`; the store cap bounds their sum.
//!
//! ## Tier/gate interaction (P02 contract)
//!
//! [`tiers::tier_of`] classifies every tool (unknown→exec, `mcp__*`≥write).
//! The tier selects the ACP gate class enforced by `crate::acp::decide`
//! (phase 02, c010): Read tier needs a read grant, Write tier needs a
//! session grant + fencing lease, Exec tier always requires approval. The
//! gateway never allows what the ACP gate denies — `Registry::invoke`
//! consults the tier mapping for write-intent labelling, and the ACP
//! `decide` remains the enforcement point for the actual call. Unknown
//! tools are denied by BOTH layers (registry `UnknownTool`, ACP
//! `UnknownTool`): the pairing is deliberate defense in depth, documented
//! here so the two `UnknownTool` variants are not mistaken for redundancy.

pub mod bundles;
pub mod cli;
pub mod config;
pub mod fixtures;
pub mod lens;
pub mod pool;
pub mod registry;
pub mod tiers;
pub mod toolmatch;
