//! f06-orca-versioned-skills: version-matched skills served by the binary.
//!
//! Clean-room stub pattern (c059
//! `cd1b364bf35781bad06bf75ab1766afa8d6cec69cb2060529691d89871a2098a`,
//! c060 `51bc9c91c586be3943ba3aa4d44bcd3a2eb412b5c663d54b33686007285ee949`,
//! MIT): the checked-in `SKILL.md` is a *discovery stub*, never the usage
//! guide — the full version-matched reference is served by the binary
//! (`stc skills get <name>`), so the stub can never drift from the binary
//! that will actually run the commands. Version mismatch is a typed refusal.

use super::RoleError;

/// Binary that serves the version-matched references.
pub const SKILL_BINARY: &str = "stc";

/// Discovery stub for one skill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillStub {
    pub name: String,
    pub description: String,
    pub binary: String,
    pub version: String,
}

impl SkillStub {
    /// Render the discovery stub: frontmatter (`name`/`description`) + the
    /// fixed stub body pointing at the version-matched reference. The stub
    /// carries NO usage guide by construction.
    pub fn render_stub(&self) -> String {
        format!(
            "---\nname: {}\ndescription: >-\n  {}\n---\n\n# {}\n\nThis file is a discovery stub, not the usage guide. The full, version-matched\nreference is served by the `{}` binary itself — kept out of this file on\npurpose so it can never drift from the binary that will actually run your commands.\n\nLoad the version-matched guide before running commands:\n\n```text\n{} skills get {}\n```\n",
            self.name, self.description, self.name, self.binary, self.binary, self.name
        )
    }

    /// Resolve a version-matched reference: exact match only, anything else
    /// is a typed refusal (never a silent nearest-match).
    pub fn resolve_reference(&self, binary_version: &str) -> Result<String, RoleError> {
        if binary_version == self.version {
            Ok(format!(
                "{} skills get {} (version {})",
                self.binary, self.name, self.version
            ))
        } else {
            Err(RoleError::SkillViolation {
                detail: format!(
                    "skill {} stub v{} does not match binary v{binary_version}; update the binary or the stub",
                    self.name, self.version
                ),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_points_at_binary_never_embeds_guide() {
        let stub = SkillStub {
            name: "orchestration".into(),
            description: "Coordinate supervised workers.".into(),
            binary: SKILL_BINARY.into(),
            version: "2.0.0".into(),
        };
        let md = stub.render_stub();
        assert!(md.contains("name: orchestration"));
        assert!(md.contains("stc skills get orchestration"));
        assert!(md.contains("discovery stub, not the usage guide"));
    }

    #[test]
    fn version_mismatch_is_typed_never_silent() {
        let stub = SkillStub {
            name: "orchestration".into(),
            description: "d".into(),
            binary: SKILL_BINARY.into(),
            version: "2.0.0".into(),
        };
        assert!(stub.resolve_reference("2.0.0").is_ok());
        let err = stub.resolve_reference("2.0.1").unwrap_err();
        assert!(matches!(err, RoleError::SkillViolation { .. }));
        assert!(err.to_string().contains("2.0.0"));
    }
}
