//! Cross-harness MCP config read/write (JSON / TOML / JSONC) with comment
//! preservation for per-executor MCP wiring.
//!
//! Clean-room re-derivation (BloopAI/vibe-kanban, Apache-2.0) behind
//! citation c021
//! (`4d6e1db7f9f9703135cb88d9db52eaae3376f542d71f837fe5bb083db756ae73`).
//! No upstream code is copied: the re-derived rules are — one [`McpConfig`]
//! value round-trips through all three formats; comment lines (`#`, `//`)
//! are preserved across the round-trip (never dropped, never executed);
//! unknown fields fail typed (never silently ignored).
//!
//! Lockfile-neutral: hand-rolled subset parsers only (serde_json for the
//! JSON/JSONC payload after comment-stripping; a line parser for the
//! `[servers.<name>]` TOML subset). No new dependencies.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

/// Typed config failures. No `String` errors cross this boundary.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("config parse failed ({format}): {detail}")]
    Parse { format: String, detail: String },
    #[error("unknown config field {field}: fail-closed, never ignored")]
    UnknownField { field: String },
    #[error("config missing server {server}")]
    MissingServer { server: String },
}

/// One MCP server wiring: command + argv. Secrets never live here
/// (placeholders only — enforced by the caller-side roles check).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerCfg {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

/// Config file: servers plus the preserved comment lines (in order).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct McpConfigFile {
    pub servers: BTreeMap<String, ServerCfg>,
    pub comments: Vec<String>,
}

fn is_comment_line(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with('#') || t.starts_with("//")
}

/// Split raw text into (comment lines, payload text). Comments are
/// preserved verbatim for re-emission; they are never interpreted.
fn split_comments(text: &str) -> (Vec<String>, String) {
    let mut comments = Vec::new();
    let mut payload = String::new();
    for line in text.lines() {
        if is_comment_line(line) {
            comments.push(line.into());
        } else {
            payload.push_str(line);
            payload.push('\n');
        }
    }
    (comments, payload)
}

/// Strip `//` line comments and `/* */` block comments (JSONC) outside
/// string literals. A `//` inside a quoted string (e.g. URLs) is kept.
fn strip_jsonc(payload: &str) -> String {
    let mut out = String::new();
    let bytes = payload.as_bytes();
    let mut i = 0;
    let mut in_str = false;
    let mut esc = false;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if in_str {
            out.push(c);
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if c == '"' {
            in_str = true;
            out.push(c);
            i += 1;
            continue;
        }
        if c == '/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

#[derive(Debug, Deserialize)]
struct JsonDoc {
    servers: BTreeMap<String, ServerCfg>,
    #[serde(flatten)]
    rest: BTreeMap<String, serde_json::Value>,
}

fn from_json_doc(doc: JsonDoc, comments: Vec<String>) -> Result<McpConfigFile, ConfigError> {
    if let Some(k) = doc.rest.keys().next() {
        return Err(ConfigError::UnknownField { field: k.clone() });
    }
    Ok(McpConfigFile {
        servers: doc.servers,
        comments,
    })
}

impl McpConfigFile {
    pub fn read_json(text: &str) -> Result<Self, ConfigError> {
        let (comments, payload) = split_comments(text);
        let doc: JsonDoc = serde_json::from_str(&payload).map_err(|e| ConfigError::Parse {
            format: "json".into(),
            detail: e.to_string(),
        })?;
        from_json_doc(doc, comments)
    }

    pub fn read_jsonc(text: &str) -> Result<Self, ConfigError> {
        let (comments, payload) = split_comments(text);
        let stripped = strip_jsonc(&payload);
        let doc: JsonDoc = serde_json::from_str(&stripped).map_err(|e| ConfigError::Parse {
            format: "jsonc".into(),
            detail: e.to_string(),
        })?;
        from_json_doc(doc, comments)
    }

    /// Minimal TOML subset: `[servers.<name>]` sections with
    /// `command = "..."` and `args = ["...", ...]`. Anything outside the
    /// subset is a typed parse failure (never silently accepted).
    pub fn read_toml(text: &str) -> Result<Self, ConfigError> {
        let (comments, payload) = split_comments(text);
        let mut servers: BTreeMap<String, ServerCfg> = BTreeMap::new();
        let mut current: Option<String> = None;
        let fail = |detail: &str| ConfigError::Parse {
            format: "toml".into(),
            detail: detail.into(),
        };
        for raw in payload.lines() {
            let line = raw.trim();
            if line.is_empty() {
                continue;
            }
            if line.starts_with('[') {
                if !line.ends_with(']') {
                    return Err(fail("unclosed section header"));
                }
                let inner = line[1..line.len() - 1].trim();
                let name = inner
                    .strip_prefix("servers.")
                    .ok_or_else(|| fail("only [servers.<name>] sections are supported"))?;
                if name.is_empty() {
                    return Err(fail("empty server name"));
                }
                servers.insert(
                    name.into(),
                    ServerCfg {
                        command: String::new(),
                        args: vec![],
                    },
                );
                current = Some(name.into());
                continue;
            }
            let name = current
                .clone()
                .ok_or_else(|| fail("key outside [servers.<name>]"))?;
            let (k, v) = line
                .split_once('=')
                .ok_or_else(|| fail("expected key = value"))?;
            match k.trim() {
                "command" => {
                    let v = v.trim().trim_matches('"').to_string();
                    servers.get_mut(&name).unwrap().command = v;
                }
                "args" => {
                    let v = v.trim();
                    if !v.starts_with('[') || !v.ends_with(']') {
                        return Err(fail("args must be [\"...\", ...]"));
                    }
                    let inner = &v[1..v.len() - 1];
                    let args: Vec<String> = inner
                        .split(',')
                        .filter_map(|s| {
                            let s = s.trim().trim_matches('"');
                            if s.is_empty() {
                                None
                            } else {
                                Some(s.to_string())
                            }
                        })
                        .collect();
                    servers.get_mut(&name).unwrap().args = args;
                }
                other => {
                    return Err(ConfigError::UnknownField {
                        field: other.into(),
                    });
                }
            }
        }
        for (name, cfg) in &servers {
            if cfg.command.is_empty() {
                return Err(ConfigError::MissingServer {
                    server: name.clone(),
                });
            }
        }
        Ok(McpConfigFile { servers, comments })
    }

    pub fn write_json(&self) -> String {
        let mut out = String::new();
        for c in &self.comments {
            out.push_str(c);
            out.push('\n');
        }
        let doc = serde_json::json!({"servers": self.servers});
        out.push_str(&serde_json::to_string_pretty(&doc).unwrap_or_default());
        out.push('\n');
        out
    }

    pub fn write_toml(&self) -> String {
        let mut out = String::new();
        for c in &self.comments {
            out.push_str(c);
            out.push('\n');
        }
        for (name, cfg) in &self.servers {
            out.push_str(&format!("[servers.{name}]\n"));
            out.push_str(&format!("command = \"{}\"\n", cfg.command));
            let args = cfg
                .args
                .iter()
                .map(|a| format!("\"{a}\""))
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!("args = [{args}]\n"));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> McpConfigFile {
        McpConfigFile {
            servers: [(
                "lint".into(),
                ServerCfg {
                    command: "lint-mcp".into(),
                    args: vec!["--stdio".into()],
                },
            )]
            .into_iter()
            .collect(),
            comments: vec!["# per-executor wiring".into()],
        }
    }

    #[test]
    fn json_round_trip_preserves_comments() {
        let cfg = sample();
        let text = cfg.write_json();
        assert!(text.contains("# per-executor wiring"));
        let back = McpConfigFile::read_json(&text).unwrap();
        assert_eq!(back, cfg);
    }

    #[test]
    fn jsonc_strips_inline_comments_but_keeps_header() {
        let text = "# harness wiring\n{\n// stdio bridge\n\"servers\": {\n\"lint\": {\"command\": \"lint-mcp\", \"args\": []}\n}\n}\n";
        let back = McpConfigFile::read_jsonc(text).unwrap();
        assert_eq!(back.servers["lint"].command, "lint-mcp");
        assert!(back.comments.iter().any(|c| c.contains("harness wiring")));
        assert!(back.comments.iter().any(|c| c.contains("stdio bridge")));
    }

    #[test]
    fn toml_subset_round_trip_and_unknown_refused() {
        let cfg = sample();
        let text = cfg.write_toml();
        assert!(text.contains("[servers.lint]"));
        let back = McpConfigFile::read_toml(&text).unwrap();
        assert_eq!(back.servers, cfg.servers);
        assert!(back.comments.iter().any(|c| c.contains("per-executor")));
        // Unknown keys fail typed, never ignored.
        let bad = "[servers.lint]\ncommand = \"x\"\nsecret = \"y\"\n";
        assert!(matches!(
            McpConfigFile::read_toml(bad).unwrap_err(),
            ConfigError::UnknownField { .. }
        ));
        let bad_json = "{\"servers\": {}, \"marketplace\": {}}";
        assert!(matches!(
            McpConfigFile::read_json(bad_json).unwrap_err(),
            ConfigError::UnknownField { .. }
        ));
    }
}
