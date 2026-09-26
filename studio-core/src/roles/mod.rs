//! Agent templates + identity (Phase 5).
//! Versioned `RolePack`s with skill lockfiles + registration-parity checks;
//! multi-profile isolation via per-lane config dirs; capability-token secret broker
//! (workers never see raw creds; one-shot approval facts for OAuth'd writes).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RolePack {
    pub name: String,
    pub version: String,
    pub tools: Vec<String>,
    /// sha256 of the skill lockfile content
    pub skill_lockfile_hash: String,
}

impl RolePack {
    pub fn lockfile_hash(content: &str) -> String {
        let mut h = Sha256::new();
        h.update(content.as_bytes());
        hex::encode(h.finalize())
    }

    /// Parity: every tool must exist in the MCP catalog; no extras unregistered.
    pub fn check_parity(&self, catalog: &[String]) -> Result<(), String> {
        for t in &self.tools {
            if !catalog.contains(t) {
                return Err(format!("phantom tool in RolePack {}: {t}", self.name));
            }
        }
        Ok(())
    }
}

/// Per-lane profile isolation: config dir per lane (never shared credentials).
pub fn profile_dir(base: &str, lane: &str) -> String {
    format!("{base}/profiles/{lane}")
}

/// Capability-token secret broker: mint scoped one-shot tokens; workers present
/// tokens, never raw creds.
#[derive(Debug, Default)]
pub struct SecretBroker {
    tokens: HashMap<String, (String, bool)>, // token -> (scope, consumed)
}

impl SecretBroker {
    pub fn mint(&mut self, scope: &str) -> String {
        let tok = format!("cap-{}-{}", scope, self.tokens.len());
        self.tokens.insert(tok.clone(), (scope.into(), false));
        tok
    }

    /// One-shot redeem: second use fails closed.
    pub fn redeem(&mut self, token: &str, scope: &str) -> Result<(), String> {
        match self.tokens.get_mut(token) {
            Some((s, consumed)) if s == scope && !*consumed => {
                *consumed = true;
                Ok(())
            }
            _ => Err("invalid or consumed capability token".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parity_catches_phantom() {
        let pack = RolePack {
            name: "coder".into(),
            version: "1".into(),
            tools: vec!["read".into(), "ghost".into()],
            skill_lockfile_hash: RolePack::lockfile_hash("skills"),
        };
        assert!(pack.check_parity(&["read".into()]).is_err());
        assert!(pack.check_parity(&["read".into(), "ghost".into()]).is_ok());
    }

    #[test]
    fn tokens_are_one_shot() {
        let mut b = SecretBroker::default();
        let t = b.mint("write:a.rs");
        assert!(b.redeem(&t, "write:a.rs").is_ok());
        assert!(b.redeem(&t, "write:a.rs").is_err());
    }
}
