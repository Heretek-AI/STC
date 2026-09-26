//! `LocalCliCapabilities` shim for non-ACP CLIs (resume/instruction/ingress/permission).
//! Anchor: Orkas `local_agents/registry.ts` (`LocalCliCapabilities`).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResumeStrategy {
    Native,
    ReplayBootstrap,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstructionChannel {
    SystemPrompt,
    DeveloperMessage,
    BootstrapFile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalCliCapabilities {
    pub cli: String,
    pub resume: ResumeStrategy,
    pub instruction_channel: InstructionChannel,
    /// durable instructions survive a new process resuming the same session
    pub durable_instructions: bool,
    pub permission_policies: Vec<String>,
}

impl LocalCliCapabilities {
    pub fn for_cli(cli: &str) -> Self {
        match cli {
            "claude" => Self {
                cli: cli.into(),
                resume: ResumeStrategy::Native,
                instruction_channel: InstructionChannel::SystemPrompt,
                durable_instructions: false,
                permission_policies: vec!["deny-by-default".into()],
            },
            "codex" => Self {
                cli: cli.into(),
                resume: ResumeStrategy::ReplayBootstrap,
                instruction_channel: InstructionChannel::DeveloperMessage,
                durable_instructions: true,
                permission_policies: vec!["deny-by-default".into()],
            },
            _ => Self {
                cli: cli.into(),
                resume: ResumeStrategy::None,
                instruction_channel: InstructionChannel::BootstrapFile,
                durable_instructions: false,
                permission_policies: vec!["deny-by-default".into()],
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_clis_have_native_paths() {
        assert_eq!(
            LocalCliCapabilities::for_cli("claude").resume,
            ResumeStrategy::Native
        );
        assert!(LocalCliCapabilities::for_cli("codex").durable_instructions);
        assert_eq!(
            LocalCliCapabilities::for_cli("other").resume,
            ResumeStrategy::None
        );
    }
}
