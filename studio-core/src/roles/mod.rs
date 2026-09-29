//! Agent contract: versioned `RolePack`s + non-escalating spawn policy.
//!
//! Port of v1 `studio-core/src/roles/mod.rs`
//! (`a414bc61fb12a41c11fe188c806efdaec0719be2de13fb5ec2ecd1464d8731ab`,
//! FILE_HASH_ONLY) **behind the Q9 bar** — deliberately narrowed, not copied
//! whole:
//!
//! - KEPT: `RolePack` identity (`name`, `version`, `tools`,
//!   `skill_lockfile_hash`, `system_prompt`, `model_slot`, `harness_profile`,
//!   `spawns`, `output_schema`), catalog parity ("no phantom tools"),
//!   raw-secret refusal (placeholders only), content-hash lock helpers, and
//!   `SpawnPolicy` non-escalation (`Isolated` / `Supervised` with inheritable
//!   scopes; over-authority requests are typed rejections, never silent
//!   downgrades).
//! - DROPPED for v2: `mcp_manifest` enforcement. The v2 tree has no `mcp`
//!   module (that surface returns in phase 03, MCP pool); carrying a manifest
//!   type with no gateway to enforce it would be contract theater. Manifest
//!   double-enforcement is re-introduced with the pool. `save_pack` /
//!   `load_pack` / `emit_pack` / fidelity scoring stay in v1 (marketplace
//!   surface — scope firewall) and return with the marketplace phases.
//! - CHANGED: every `String` error is now a typed [`RoleError`] /
//!   [`SpawnDenied`] (Q9: typed failure reasons everywhere).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Typed role-contract failures. No `String` errors cross this boundary.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RoleError {
    #[error("phantom tool in RolePack {pack}: {tool} not in catalog")]
    PhantomTool { pack: String, tool: String },
    #[error("raw secret material in {field}")]
    RawSecret { field: String },
    #[error("rolepack.lock drift: {detail}")]
    LockDrift { detail: String },
    #[error("lock names {lock_name} v{lock_version}, pack is {pack_name} v{pack_version}")]
    LockIdentityMismatch {
        lock_name: String,
        lock_version: String,
        pack_name: String,
        pack_version: String,
    },
}

/// Versioned agent template. Roles name model slots, never providers; workers
/// never see raw credentials (capability tokens only — enforced by callers).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RolePack {
    pub name: String,
    pub version: String,
    pub tools: Vec<String>,
    /// sha256 of the skill lockfile content.
    pub skill_lockfile_hash: String,
    /// Authored system prompt.
    #[serde(default)]
    pub system_prompt: String,
    /// Model slot name (roles name slots, never providers).
    #[serde(default)]
    pub model_slot: String,
    #[serde(default)]
    pub harness_profile: HarnessProfile,
    #[serde(default)]
    pub spawns: SpawnPolicy,
    /// Optional JSON Schema for structured agent output.
    #[serde(default)]
    pub output_schema: Option<serde_json::Value>,
}

/// Which harness a pack targets.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HarnessProfile {
    pub harness: String,
    pub version_req: String,
}

impl Default for HarnessProfile {
    fn default() -> Self {
        Self {
            harness: "omp".into(),
            version_req: ">=1".into(),
        }
    }
}

/// Spawn policy: children never escalate above the parent. A request above
/// the parent's authority is a typed rejection, never a silent downgrade.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum SpawnPolicy {
    #[default]
    Isolated,
    Supervised {
        inheritable_scopes: Vec<String>,
    },
}

/// Typed spawn refusal: names the offending scope and the authority it
/// exceeded. Acceptance criterion "spawn escalation rejected typed".
#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[error("spawn denied: child scope {scope} exceeds inheritable authority [{allowed}]")]
pub struct SpawnDenied {
    pub scope: String,
    pub allowed: String,
}

impl SpawnPolicy {
    /// Child scopes must be a subset of the inheritable scopes intersected
    /// with what the parent actually holds. Anything else fails closed.
    ///
    /// Two hardening rules (QA-R2): an EMPTY child scope list is denied
    /// outright (spawning nothing is a malformed request or a policy probe,
    /// never a legitimate grant), and callers MUST NOT supply `parent_scopes`
    /// from an unverified claim — the enforced entry point is
    /// [`RolePack::request_spawn`], which binds the parent basis to the
    /// pack's own declared `tools`. This primitive trusts its inputs by
    /// construction (it cannot tell a forged basis from a real one); the
    /// forged-parent contrast is pinned in `tests/spawn_matrix.rs`.
    pub fn request(
        &self,
        child_scopes: &[String],
        parent_scopes: &[String],
    ) -> Result<(), SpawnDenied> {
        let allowed: Vec<&String> = match self {
            SpawnPolicy::Isolated => vec![],
            SpawnPolicy::Supervised { inheritable_scopes } => inheritable_scopes
                .iter()
                .filter(|s| parent_scopes.contains(s))
                .collect(),
        };
        if child_scopes.is_empty() {
            return Err(SpawnDenied {
                scope: "<empty-child>".into(),
                allowed: allowed
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(","),
            });
        }
        for scope in child_scopes {
            if !allowed.contains(&scope) {
                let list = allowed
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(",");
                return Err(SpawnDenied {
                    scope: scope.clone(),
                    allowed: list,
                });
            }
        }
        Ok(())
    }
}

/// Raw secret patterns: never persisted in packs or locks. Placeholders
/// (`{{STUDIO_SECRET:label}}`) pass; raw `sk-`/`AKIA`/`xox`/`ghp_` fail.
///
/// Full-text CONTAINS scan (QA-R3): the previous whitespace-token scan missed
/// `key=sk-...`, JSON-embedded `"token":"sk-..."`, and prefixed secrets. Every
/// occurrence is examined with its trailing run, so embedding cannot hide it.
/// Length thresholds on the trailing run keep prose (`desk-review`,
/// `flask-app`) passing: only secret-shaped runs refuse.
fn reject_raw_secrets(field: &str, text: &str) -> Result<(), RoleError> {
    let refused = |_: &str| {
        Err(RoleError::RawSecret {
            field: field.into(),
        })
    };
    // `sk-` + secret run (>= 16 base64-ish chars after the prefix).
    let bytes = text.as_bytes();
    let mut i = 0;
    while i + 3 < bytes.len() {
        if &bytes[i..i + 3] == b"sk-" {
            let run = bytes[i + 3..]
                .iter()
                .take_while(|c| c.is_ascii_alphanumeric() || **c == b'-' || **c == b'_')
                .count();
            if run >= 16 {
                return refused("sk-");
            }
            i += 3;
        } else {
            i += 1;
        }
    }
    // `AKIA` + 16 (AWS access key id shape).
    let mut j = 0;
    while j + 4 < bytes.len() {
        if &bytes[j..j + 4] == b"AKIA" {
            let run = bytes[j + 4..]
                .iter()
                .take_while(|c| c.is_ascii_alphanumeric())
                .count();
            if run >= 16 {
                return refused("AKIA");
            }
            j += 4;
        } else {
            j += 1;
        }
    }
    // Distinctive prefixes: any occurrence refuses.
    for marker in ["xoxb-", "xoxp-", "ghp_"] {
        if text.contains(marker) {
            return refused(marker);
        }
    }
    Ok(())
}

impl RolePack {
    pub fn lockfile_hash(content: &str) -> String {
        let mut h = Sha256::new();
        h.update(content.as_bytes());
        hex::encode(h.finalize())
    }

    /// Parity: every tool must exist in the catalog; no phantom tools.
    pub fn check_parity(&self, catalog: &[String]) -> Result<(), RoleError> {
        for t in &self.tools {
            if !catalog.contains(t) {
                return Err(RoleError::PhantomTool {
                    pack: self.name.clone(),
                    tool: t.clone(),
                });
            }
        }
        Ok(())
    }

    /// Refuse raw secret material anywhere in the pack (placeholders only).
    pub fn check_no_raw_secrets(&self) -> Result<(), RoleError> {
        reject_raw_secrets("system_prompt", &self.system_prompt)
    }

    /// Enforced spawn entry point (QA-R2): the parent basis is bound to THIS
    /// pack's declared `tools` — a caller-supplied basis is never accepted,
    /// so a forged "the parent holds write" claim cannot escalate. For the
    /// raw primitive (which trusts its basis by construction), see
    /// [`SpawnPolicy::request`].
    pub fn request_spawn(&self, child_scopes: &[String]) -> Result<(), SpawnDenied> {
        self.spawns.request(child_scopes, &self.tools)
    }

    /// Content-hash lock over the pack identity + skill lockfile hash.
    /// Canonical JSON (sorted keys) so `HashMap` order can never cause
    /// phantom drift between compute and verify.
    pub fn compute_lock_hash(&self) -> String {
        let v = serde_json::json!({
            "name": self.name,
            "version": self.version,
            "tools": self.tools,
            "skill_lockfile_hash": self.skill_lockfile_hash,
            "model_slot": self.model_slot,
        });
        Self::lockfile_hash(&serde_json::to_string(&v).unwrap_or_default())
    }

    /// `--locked` verification: recompute and compare. Any drift fails closed.
    pub fn verify_lock(&self, lock: &RolePackLock) -> Result<(), RoleError> {
        if lock.pack_name != self.name || lock.pack_version != self.version {
            return Err(RoleError::LockIdentityMismatch {
                lock_name: lock.pack_name.clone(),
                lock_version: lock.pack_version.clone(),
                pack_name: self.name.clone(),
                pack_version: self.version.clone(),
            });
        }
        if lock.pack_hash != self.compute_lock_hash() {
            return Err(RoleError::LockDrift {
                detail: format!("pack {} content drifted from lock", self.name),
            });
        }
        Ok(())
    }

    /// Minimal flagship pack: web coding tasks, coder model slot. Scope
    /// deliberately narrow (acceptance needs a coherent pack, not the v1
    /// 35-pack marketplace library — scope firewall).
    pub fn coder_web() -> Self {
        Self {
            name: "coder-web".into(),
            version: "1".into(),
            tools: vec![
                "read".into(),
                "write".into(),
                "edit".into(),
                "runProcess".into(),
                "code_search".into(),
                "tool_open".into(),
                "kv_get".into(),
            ],
            skill_lockfile_hash: Self::lockfile_hash("coder-web-skills-v1"),
            system_prompt: "You are a web-coding specialist. Implement exactly the declared task; \
                request scope expansion instead of improvising. Evidence before claims: every \
                completed file must be verified by tests or checks before reporting done."
                .into(),
            model_slot: "coder.primary".into(),
            harness_profile: HarnessProfile::default(),
            spawns: SpawnPolicy::Supervised {
                inheritable_scopes: vec!["read".into(), "code_search".into()],
            },
            output_schema: None,
        }
    }
}

/// Pinned content lock for one pack (`rolepack.lock` moral equivalent).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RolePackLock {
    pub pack_name: String,
    pub pack_version: String,
    pub pack_hash: String,
}

pub fn compute_lock(pack: &RolePack) -> RolePackLock {
    RolePackLock {
        pack_name: pack.name.clone(),
        pack_version: pack.version.clone(),
        pack_hash: pack.compute_lock_hash(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> Vec<String> {
        vec![
            "read".into(),
            "write".into(),
            "edit".into(),
            "patch".into(),
            "runProcess".into(),
            "code_search".into(),
            "tool_open".into(),
            "kv_get".into(),
            "web_search".into(),
            "plan_open".into(),
            "dag_commit".into(),
            "syntax_check".into(),
            "sast".into(),
            "sbom".into(),
            "retrieve_docs".into(),
        ]
    }

    #[test]
    fn parity_catches_phantom_typed() {
        let mut pack = RolePack::coder_web();
        pack.tools.push("ghost".into());
        let err = pack.check_parity(&catalog()).unwrap_err();
        assert!(matches!(err, RoleError::PhantomTool { .. }));
        assert!(err.to_string().contains("ghost"));
        pack.tools.pop();
        assert!(pack.check_parity(&catalog()).is_ok());
    }

    #[test]
    fn pack_rejects_raw_secrets_typed() {
        let mut pack = RolePack::coder_web();
        pack.system_prompt = "key sk-live-0123456789abcdef".into();
        let err = pack.check_no_raw_secrets().unwrap_err();
        assert!(matches!(err, RoleError::RawSecret { .. }));
        // Placeholders pass.
        pack.system_prompt = "token {{STUDIO_SECRET:lane}}".into();
        assert!(pack.check_no_raw_secrets().is_ok());
    }

    #[test]
    fn secret_scan_catches_embedded_prefixed_and_json_bypass() {
        // QA-R3 bypass 1: `key=...` form (no whitespace boundary).
        let mut pack = RolePack::coder_web();
        pack.system_prompt = "config api_key=sk-live-0123456789abcdef done".into();
        assert!(matches!(
            pack.check_no_raw_secrets().unwrap_err(),
            RoleError::RawSecret { .. }
        ));
        // QA-R3 bypass 2: JSON-embedded secret.
        pack.system_prompt = r#"auth {"token": "sk-ant-0123456789abcdef"} ok"#.into();
        assert!(matches!(
            pack.check_no_raw_secrets().unwrap_err(),
            RoleError::RawSecret { .. }
        ));
        // QA-R3 bypass 3: prefixed secret (glued to a non-space prefix).
        pack.system_prompt = "prefixsk-test-0123456789abcdef".into();
        assert!(matches!(
            pack.check_no_raw_secrets().unwrap_err(),
            RoleError::RawSecret { .. }
        ));
        // Non-secret prose with `sk-`/`AKIA` substrings still passes.
        pack.system_prompt = "desk-review of the flask-app; AKIA is an airport code".into();
        assert!(pack.check_no_raw_secrets().is_ok());
    }

    #[test]
    fn spawn_never_escalates_typed() {
        let parent = vec!["read".to_string(), "code_search".to_string()];
        let pol = SpawnPolicy::Supervised {
            inheritable_scopes: vec!["read".to_string()],
        };
        assert!(pol.request(&["read".to_string()], &parent).is_ok());
        // write is neither inheritable nor held: typed rejection, not downgrade.
        let err = pol.request(&["write".to_string()], &parent).unwrap_err();
        assert_eq!(err.scope, "write");
        assert!(err.to_string().contains("write"));
        // Isolated never spawns, even for scopes the parent holds.
        let err = SpawnPolicy::Isolated
            .request(&["read".to_string()], &parent)
            .unwrap_err();
        assert_eq!(err.scope, "read");
        // Supervised cannot launder scopes the parent does not hold.
        let pol = SpawnPolicy::Supervised {
            inheritable_scopes: vec!["write".to_string()],
        };
        assert!(pol.request(&["write".to_string()], &parent).is_err());
    }

    #[test]
    fn empty_child_spawn_is_denied_not_vacuously_allowed() {
        // QA-R2: `Isolated.request([], anything)` used to Ok via the empty
        // loop — a policy probe must fail closed, both policies.
        let parent = vec!["read".to_string()];
        let err = SpawnPolicy::Isolated.request(&[], &parent).unwrap_err();
        assert_eq!(err.scope, "<empty-child>");
        let pol = SpawnPolicy::Supervised {
            inheritable_scopes: vec!["read".to_string()],
        };
        let err = pol.request(&[], &parent).unwrap_err();
        assert_eq!(err.scope, "<empty-child>");
        // The enforced entry point denies empty spawns too.
        assert_eq!(
            RolePack::coder_web().request_spawn(&[]).unwrap_err().scope,
            "<empty-child>"
        );
    }

    #[test]
    fn forged_parent_basis_cannot_escalate_through_pack() {
        // QA-R2: the primitive trusts its basis — a forged claim makes the
        // PRIMITIVE allow scopes the pack does not hold. The enforced pack
        // entry point binds the basis to declared tools, so the same request
        // through the pack denies. Contrast needs an inheritable-but-unheld
        // scope (a dispatcher shape: `sast` inheritable on paper, not held).
        let pack = RolePack {
            name: "dispatcher-shape".into(),
            version: "1".into(),
            tools: vec!["read".into(), "plan_open".into()],
            skill_lockfile_hash: RolePack::lockfile_hash("x"),
            system_prompt: String::new(),
            model_slot: "manager".into(),
            harness_profile: Default::default(),
            spawns: SpawnPolicy::Supervised {
                inheritable_scopes: vec!["read".into(), "plan_open".into(), "sast".into()],
            },
            output_schema: None,
        };
        assert!(!pack.tools.contains(&"sast".to_string()));
        let forged_basis = vec!["read".into(), "plan_open".into(), "sast".into()];
        assert!(
            pack.spawns
                .request(&["sast".to_string()], &forged_basis)
                .is_ok(),
            "primitive must be shown trusting its basis (contrast premise)"
        );
        let err = pack.request_spawn(&["sast".to_string()]).unwrap_err();
        assert_eq!(err.scope, "sast");
        // And declared inheritable scopes still propagate through the pack.
        assert!(pack.request_spawn(&["read".to_string()]).is_ok());
    }

    #[test]
    fn lock_detects_drift_typed() {
        let pack = RolePack::coder_web();
        let lock = compute_lock(&pack);
        assert!(pack.verify_lock(&lock).is_ok());
        let mut drifted = pack.clone();
        drifted.tools.push("sast".into());
        let err = drifted.verify_lock(&lock).unwrap_err();
        assert!(matches!(err, RoleError::LockDrift { .. }));
        let wrong = RolePackLock {
            pack_name: "other".into(),
            ..lock
        };
        assert!(matches!(
            pack.verify_lock(&wrong).unwrap_err(),
            RoleError::LockIdentityMismatch { .. }
        ));
    }

    #[test]
    fn flagship_pack_is_coherent() {
        let pack = RolePack::coder_web();
        pack.check_parity(&catalog()).unwrap();
        pack.check_no_raw_secrets().unwrap();
        assert_eq!(pack.model_slot, "coder.primary");
        // Supervised packs inherit read-only scopes only.
        match &pack.spawns {
            SpawnPolicy::Isolated => {}
            SpawnPolicy::Supervised { inheritable_scopes } => {
                for s in inheritable_scopes {
                    assert!(
                        ["read", "code_search", "plan_open"].contains(&s.as_str()),
                        "inherits write-class scope {s}"
                    );
                }
            }
        }
    }
}
