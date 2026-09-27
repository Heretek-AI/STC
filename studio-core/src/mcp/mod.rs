//! Dynamic MCP gateway / Tool Lens (Phase 2).
//! Anchors (by symbol):
//! - opencode-swarm `agents/index.ts` (`AGENT_TOOL_MAP`, `TOOL_METADATA`), `hooks/shell-write-detect.ts`
//!   (`WriteCategory` 8 variants, `detectPosixWrites`, `resolveWriteTargets`),
//!   `hooks/scope-guard.ts` (`SCOPE_NOT_DECLARED`, `SCOPE_ROOT_ESCAPE`, `SCOPE_VIOLATION`),
//!   `config/constants.ts` (`WRITE_TOOL_NAMES`)
//! - oh-my-pi `xdev.ts` (`ToolLoadMode essential|discoverable`, `xd://` listing)

pub mod overflow;
pub mod process;
pub mod registry;
pub mod write_detect;

pub use overflow::{OverflowKv, OVERFLOW_TTL_SECS, WIRE_LIMIT_BYTES};
pub use process::{run_process, ProcessSpec, RunError};
pub use registry::{
    catalog, default_manifest, derive_tool_map, Access, AgentRole, Gateway, RoleManifest, ToolDef,
};
pub use write_detect::{detect_shell_writes, WriteCategory, WriteTarget};
