//! Schema-to-CLI generation: typed subcommands + `--help` + SKILL.md.
//!
//! Clean-room re-derivation (jlowin/fastmcp, Apache-2.0) behind citation
//! c020 (`dce670f72e1b281fa22214308b05f249371fe286c8bb7d394490832e835ee38e`).
//! No upstream code is copied: the re-derived rules are — each tool schema
//! yields one subcommand (registry tool-name grammar), each `properties`
//! entry yields one `--kebab-case` flag with its description, required
//! properties are marked, and the SKILL.md companion documents the same
//! set so CLI and skill text can never disagree (both render from the
//! same [`CliCommand`] values).

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Typed CLI-codegen failures. No `String` errors cross this boundary.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CliError {
    #[error("tool {tool} has no object schema to generate from")]
    NoObjectSchema { tool: String },
    #[error("tool {tool} name fails CLI grammar")]
    BadName { tool: String },
}

/// One generated subcommand.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CliCommand {
    pub subcommand: String,
    pub help: String,
    pub flags: Vec<CliFlag>,
}

/// One generated flag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CliFlag {
    pub flag: String,
    pub help: String,
    pub required: bool,
}

fn kebab(name: &str) -> String {
    name.replace('_', "-")
}

/// Generate a typed subcommand from a tool name + description + JSON
/// Schema value. Non-object schemas are a typed refusal (no untyped
/// catch-all subcommands).
pub fn generate(
    tool: &str,
    description: &str,
    schema: &serde_json::Value,
) -> Result<CliCommand, CliError> {
    crate::mcp::registry::validate_tool_name(tool)
        .map_err(|_| CliError::BadName { tool: tool.into() })?;
    let obj = schema
        .get("properties")
        .and_then(|p| p.as_object())
        .ok_or_else(|| CliError::NoObjectSchema { tool: tool.into() })?;
    let required: Vec<&str> = schema
        .get("required")
        .and_then(|r| r.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    let mut flags: Vec<CliFlag> = obj
        .iter()
        .map(|(k, v)| CliFlag {
            flag: format!("--{}", kebab(k)),
            help: v
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .into(),
            required: required.contains(&k.as_str()),
        })
        .collect();
    flags.sort_by(|a, b| a.flag.cmp(&b.flag));
    Ok(CliCommand {
        subcommand: tool.into(),
        help: description.into(),
        flags,
    })
}

/// Render `--help` text for one command. Deterministic (flags sorted at
/// generation), so help output is diff-stable.
pub fn render_help(cmd: &CliCommand) -> String {
    let mut out = format!("{} — {}\n", cmd.subcommand, cmd.help);
    for f in &cmd.flags {
        let req = if f.required { " (required)" } else { "" };
        out.push_str(&format!("  {}{}: {}\n", f.flag, req, f.help));
    }
    out
}

/// Render the SKILL.md companion from the SAME [`CliCommand`] values the
/// CLI renders from (single source — the two can never disagree).
pub fn render_skill_md(commands: &[CliCommand]) -> String {
    let mut out = String::from("# Skills\n\nUse the CLI subcommands below.\n\n");
    for c in commands {
        out.push_str(&format!("## {}\n\n{}\n\n", c.subcommand, c.help));
        for f in &c.flags {
            let req = if f.required { "required" } else { "optional" };
            out.push_str(&format!("- `{}` ({req}): {}\n", f.flag, f.help));
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schema() -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "file to read"},
                "limit": {"type": "integer", "description": "max lines"}
            },
            "required": ["path"]
        })
    }

    #[test]
    fn generates_subcommand_help_and_skill_md() {
        let cmd = generate("read", "read a file", &schema()).unwrap();
        assert_eq!(cmd.subcommand, "read");
        assert_eq!(cmd.flags.len(), 2);
        assert!(cmd.flags.iter().any(|f| f.flag == "--path" && f.required));
        assert!(cmd.flags.iter().any(|f| f.flag == "--limit" && !f.required));
        let help = render_help(&cmd);
        assert!(help.contains("--path (required)"));
        let md = render_skill_md(std::slice::from_ref(&cmd));
        assert!(md.contains("## read"));
        assert!(md.contains("`--path`"));
        // CLI and skill text agree on the flag set.
        for f in &cmd.flags {
            assert!(help.contains(&f.flag));
            assert!(md.contains(&f.flag));
        }
    }

    #[test]
    fn non_object_schema_and_bad_names_refuse_typed() {
        assert!(matches!(
            generate("read", "d", &serde_json::json!({"type": "string"})).unwrap_err(),
            CliError::NoObjectSchema { .. }
        ));
        assert!(matches!(
            generate("has space", "d", &schema()).unwrap_err(),
            CliError::BadName { .. }
        ));
    }
}
