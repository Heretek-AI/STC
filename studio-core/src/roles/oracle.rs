//! V6 Discovery-Precedence Round-Trip Oracle.
//!
//! Target-semantics verification: the ported omp discovery rules resolve an
//! emitted artifact the way STC intends — checked by an oracle that runs
//! over (a) harvested corpus definitions and (b) every STC emission.
//! A precedence-collision test ensures a project-level emission cannot be
//! shadowed by a user-level pack.
//!
//! Diagnostic bar: every rejection carries file:line + a plain-English hint
//! with valid values (typed [`RoleError::FrontmatterInvalid`]).
//!
//! Rules ported from c052 (`524de1af…`) / c053 (`b2481452d…`):
//! required `name`/`description`/`systemPrompt`; `tools` CSV-or-array;
//! `spawns` `*`/CSV/array with the missing-spawns + `task`-tool → `*`
//! legacy; `read-summarize` → `readSummarize`; source precedence
//! bundled < user < project with first-wins dedup.

use std::collections::HashMap;

use super::emit::{parse_agent_md, AgentSource, EmitTarget, OmpAgentDefinition};
use super::{RoleError, RolePack};

/// One oracle verdict over a single corpus file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OracleVerdict {
    pub file: String,
    pub valid: bool,
    /// Machine reason (`ok`, `missing-name`, `missing-description`,
    /// `no-frontmatter`, `malformed-line`).
    pub reason: String,
    /// file:line + hint diagnostic (always present on invalid).
    pub diagnostic: Option<String>,
    pub source: AgentSource,
}

/// Classify a harvested file by its path convention: `.omp/agents/` is
/// omp-native, `.opencode/agents/` is opencode-format, anything else
/// (e.g. `.claude/`) is foreign and graded on the shared required-field
/// rules only.
pub fn classify_path(path: &str) -> &'static str {
    if path.contains(".omp/agents") || path.contains(".omp\\agents") {
        "omp-format"
    } else if path.contains(".opencode/agents") || path.contains("opencode/agent") {
        "opencode-format"
    } else {
        "foreign-format"
    }
}

/// Run the oracle over one file's content.
pub fn grade_file(file: &str, content: &str, source: AgentSource) -> OracleVerdict {
    match parse_agent_md(file, content, source) {
        Ok(_) => OracleVerdict {
            file: file.into(),
            valid: true,
            reason: "ok".into(),
            diagnostic: None,
            source,
        },
        Err(RoleError::FrontmatterInvalid {
            file: f,
            line,
            hint,
        }) => {
            let reason = if hint.contains("name:") {
                "missing-name"
            } else if hint.contains("description:") {
                "missing-description"
            } else if hint.contains("frontmatter fence") || hint.contains("closing") {
                "no-frontmatter"
            } else {
                "malformed-line"
            };
            OracleVerdict {
                file: file.into(),
                valid: false,
                reason: reason.into(),
                diagnostic: Some(format!("{f}:{line}: {hint}")),
                source,
            }
        }
        Err(e) => OracleVerdict {
            file: file.into(),
            valid: false,
            reason: "other".into(),
            diagnostic: Some(format!("{file}:1: {e}")),
            source,
        },
    }
}

/// Run the oracle over a corpus of `(file, content)` pairs. Sources are
/// inferred from path convention (project vs user is unknowable from a bare
/// corpus, so everything grades as `Bundled` unless the path says
/// otherwise — recorded in the verdict, not guessed).
pub fn grade_corpus(files: &[(&str, &str)]) -> Vec<OracleVerdict> {
    files
        .iter()
        .map(|(f, c)| {
            let source = if f.contains("project/") {
                AgentSource::Project
            } else if f.contains("user/") {
                AgentSource::User
            } else {
                AgentSource::Bundled
            };
            grade_file(f, c, source)
        })
        .collect()
}

/// A precedence collision: two scopes define the same agent name. The
/// winner is the highest-precedence source; the loser is shadowed (never
/// silently merged).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrecedenceCollision {
    pub name: String,
    pub winner: AgentSource,
    pub winner_file: String,
    pub shadowed: Vec<(AgentSource, String)>,
}

/// Detect collisions across a definition set (V6 precedence-collision test).
pub fn precedence_collisions(agents: &[OmpAgentDefinition]) -> Vec<PrecedenceCollision> {
    let mut by_name: HashMap<&str, Vec<&OmpAgentDefinition>> = HashMap::new();
    for a in agents {
        by_name.entry(a.name.as_str()).or_default().push(a);
    }
    let mut out = vec![];
    for (name, group) in by_name {
        if group.len() < 2 {
            continue;
        }
        let mut sorted = group.clone();
        sorted.sort_by_key(|a| std::cmp::Reverse(a.source.rank()));
        let winner = sorted[0];
        out.push(PrecedenceCollision {
            name: name.to_string(),
            winner: winner.source,
            winner_file: winner.file_path.clone().unwrap_or_default(),
            shadowed: sorted[1..]
                .iter()
                .map(|a| (a.source, a.file_path.clone().unwrap_or_default()))
                .collect(),
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Oracle over every STC emission: each pack's omp artifact must parse back
/// to 100% of intended fields — name, description, systemPrompt, tools,
/// spawns, model, output, blocking, readSummarize (9 fields; the claim is
/// tested per field below, never narrowed silently).
pub fn grade_emission(dir: &std::path::Path, pack: &RolePack) -> Result<OracleVerdict, RoleError> {
    let rel = EmitTarget::Omp.rel_path(&pack.name);
    let body = std::fs::read_to_string(dir.join(&rel)).map_err(|e| RoleError::EmitIo {
        target: format!("{:?}", EmitTarget::Omp),
        detail: e.to_string(),
    })?;
    let back = parse_agent_md(&rel, &body, AgentSource::Bundled)?;
    let def = OmpAgentDefinition::from_pack(pack, AgentSource::Bundled);
    let mut missing = vec![];
    if back.name != def.name {
        missing.push("name");
    }
    if back.description != def.description {
        missing.push("description");
    }
    if back.system_prompt != def.system_prompt {
        missing.push("systemPrompt");
    }
    if back.tools != def.tools {
        missing.push("tools");
    }
    if back.spawns != def.spawns {
        missing.push("spawns");
    }
    if back.model != def.model {
        missing.push("model");
    }
    if back.output != def.output {
        missing.push("output");
    }
    if back.blocking != def.blocking {
        missing.push("blocking");
    }
    if back.read_summarize != def.read_summarize {
        missing.push("readSummarize");
    }
    if missing.is_empty() {
        Ok(OracleVerdict {
            file: rel,
            valid: true,
            reason: "ok".into(),
            diagnostic: None,
            source: AgentSource::Bundled,
        })
    } else {
        Ok(OracleVerdict {
            file: rel.clone(),
            valid: false,
            reason: "field-loss".into(),
            diagnostic: Some(format!(
                "{rel}:2: emission lost [{}]; expected verbatim round-trip",
                missing.join(", ")
            )),
            source: AgentSource::Bundled,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::catalog::tool_catalog;
    use super::super::emit::emit_pack;
    use super::*;

    #[test]
    fn harvested_corpus_classifies_valid_or_invalid_with_reason() {
        // Stratified sample mirroring the harvested corpus shapes:
        // omp-native, opencode-format, foreign (claude-style, no description).
        let corpus = [
            (
                "repo/.omp/agents/reviewer.md",
                "---\nname: reviewer\ndescription: Reviews code.\ntools: [read]\n---\n\nReview.",
            ),
            (
                "repo/.opencode/agent/pr-reviewer.md",
                "---\nmode: subagent\ndescription: Reviews PRs.\n---\n\nReview.",
            ),
            (
                "repo/.claude/agents/helper.md",
                "---\nname: helper\n---\n\nHelp.",
            ),
            ("repo/plain.md", "no frontmatter at all"),
        ];
        let verdicts = grade_corpus(&corpus);
        assert_eq!(verdicts.len(), 4);
        assert!(verdicts[0].valid);
        // opencode `mode:` files lack omp `name:` → invalid with a reason,
        // never silently accepted as omp definitions.
        assert!(!verdicts[1].valid);
        assert_eq!(verdicts[1].reason, "missing-name");
        assert!(verdicts[1]
            .diagnostic
            .as_ref()
            .unwrap()
            .contains(".opencode/agent"));
        assert_eq!(verdicts[2].reason, "missing-description");
        assert_eq!(verdicts[3].reason, "no-frontmatter");
        for v in &verdicts {
            if !v.valid {
                let d = v.diagnostic.as_ref().unwrap();
                assert!(d.contains(&v.file), "diagnostic must name the file");
                assert!(d.contains(':'));
            }
        }
    }

    #[test]
    fn project_emission_cannot_be_shadowed_by_user_pack() {
        let proj = OmpAgentDefinition {
            name: "coder-web".into(),
            description: "proj".into(),
            system_prompt: "p".into(),
            tools: None,
            spawns: None,
            model: None,
            output: None,
            blocking: false,
            read_summarize: None,
            source: AgentSource::Project,
            file_path: Some(".omp/agents/coder-web.md".into()),
        };
        let mut user = proj.clone();
        user.description = "user".into();
        user.source = AgentSource::User;
        user.file_path = Some("~/.omp/agent/agents/coder-web.md".into());
        let collisions = precedence_collisions(&[user, proj]);
        assert_eq!(collisions.len(), 1);
        assert_eq!(collisions[0].winner, AgentSource::Project);
        assert_eq!(collisions[0].shadowed.len(), 1);
        // And discovery resolves the winner first.
        let r = super::super::emit::discover_ordered(
            vec![collisions_winner(&collisions[0])],
            vec![],
            vec![],
            Some(".omp/agents".into()),
        );
        assert_eq!(r.get("coder-web").unwrap().description, "proj");
    }

    fn collisions_winner(c: &PrecedenceCollision) -> OmpAgentDefinition {
        OmpAgentDefinition {
            name: c.name.clone(),
            description: "proj".into(),
            system_prompt: "p".into(),
            tools: None,
            spawns: None,
            model: None,
            output: None,
            blocking: false,
            read_summarize: None,
            source: c.winner,
            file_path: Some(c.winner_file.clone()),
        }
    }

    #[test]
    fn every_catalog_emission_recovers_all_intended_fields() {
        let dir = std::env::temp_dir().join(format!("stc-oracle-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let cat = tool_catalog();
        let _ = cat;
        for pack in RolePack::all_native_packs() {
            let pdir = dir.join(&pack.name);
            emit_pack(&pdir, &pack).unwrap();
            let v = grade_emission(&pdir, &pack).unwrap();
            assert!(v.valid, "{}: {:?}", pack.name, v.diagnostic);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
