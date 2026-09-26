//! Session bridge: ACP v1 over NDJSON + LocalCliCapabilities shim (Phase 1/4).
//! Anchors (by symbol):
//! - agent-of-empires `acp/acp_client/connection` (`session/request_permission`
//!   allow/deny responders, `agent_client_protocol::schema::v1`)
//! - Orkas `local_agents/registry.ts` (`LocalCliCapabilities`: resume /
//!   instructionChannel / ingress / permissionPolicies)

pub mod acp;
pub mod cli_shim;

pub use acp::{AcpError, AcpFrame, AcpSession, PermissionDecision};
pub use cli_shim::{InstructionChannel, LocalCliCapabilities, ResumeStrategy};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionBinding {
    pub task_id: String,
    pub role: String,
    pub protocol: String,
}
