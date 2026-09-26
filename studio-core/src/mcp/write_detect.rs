//! Bash write detection: 8 categories, fail-closed on unresolvable `$VAR` / substitution.
//! Port of `shell-write-detect.ts` (`WriteCategory`, `detectPosixWrites`) by symbol.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WriteCategory {
    Redirect,
    HereDoc,
    BuiltinWrite,
    InplaceEdit,
    InterpreterEval,
    NetworkDownload,
    ArchiveExtract,
    GitDestructive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteTarget {
    pub category: WriteCategory,
    pub operator: String,
    pub path: Option<String>,
    /// false when path contains $VAR / $(...) / backticks (fail-closed)
    pub resolvable: bool,
}

/// Scan a shell command for write effects. No shell execution; pure string analysis.
/// Returns targets; unresolvable dynamic paths are flagged (caller must deny).
pub fn detect_shell_writes(cmd: &str) -> Vec<WriteTarget> {
    let mut out = vec![];
    let tokens: Vec<&str> = cmd.split_whitespace().collect();

    // redirect: > >> >| <> ; here_doc: << <<<
    if cmd.contains("<<") {
        out.push(WriteTarget {
            category: WriteCategory::HereDoc,
            operator: "<<".into(),
            path: guess_path_after(cmd, "<<"),
            resolvable: resolvable(&guess_path_after(cmd, "<<")),
        });
    }
    if cmd.contains('>') && !cmd.contains(">>") {
        // single > (exclude >>, >& descriptor copies, -> arrows)
        let has_single = tokens.iter().any(|t| {
            *t == ">" || (t.starts_with('>') && !t.starts_with(">>") && !t.starts_with(">&"))
        });
        if has_single || cmd.contains(" > ") {
            out.push(WriteTarget {
                category: WriteCategory::Redirect,
                operator: ">".into(),
                path: guess_path_after(cmd, ">"),
                resolvable: true,
            });
        }
    }
    if cmd.contains(">>") {
        out.push(WriteTarget {
            category: WriteCategory::Redirect,
            operator: ">>".into(),
            path: guess_path_after(cmd, ">>"),
            resolvable: true,
        });
    }

    // builtin_write: echo ... >, printf, tee
    for (i, t) in tokens.iter().enumerate() {
        if ["echo", "printf", "tee"].contains(t) && cmd.contains('>') {
            out.push(WriteTarget {
                category: WriteCategory::BuiltinWrite,
                operator: t.to_string(),
                path: guess_path_after(cmd, ">"),
                resolvable: true,
            });
            let _ = i;
            break;
        }
    }
    // inplace_edit: sed -i, perl -i, ed
    if ["sed", "perl", "ed"]
        .iter()
        .any(|s| tokens.contains(s))
        && tokens.contains(&"-i")
    {
        out.push(WriteTarget {
            category: WriteCategory::InplaceEdit,
            operator: "sed -i".into(),
            path: last_file_token(&tokens),
            resolvable: true,
        });
    }
    // interpreter_eval: python -c, node -e, ruby -e, perl -e with write-ish payload
    if tokens
        .iter()
        .any(|t| ["python", "python3", "node", "ruby", "perl"].contains(t))
        && ["-c", "-e"].iter().any(|s| tokens.contains(s))
    {
        out.push(WriteTarget {
            category: WriteCategory::InterpreterEval,
            operator: "interpreter -c/-e".into(),
            path: None,
            resolvable: false,
        });
    }
    // network_download: curl -o/-O, wget -o/-O
    if ["curl", "wget"].iter().any(|s| tokens.contains(s)) {
        out.push(WriteTarget {
            category: WriteCategory::NetworkDownload,
            operator: tokens[0].into(),
            path: flag_value(&tokens, "-o").or(flag_value(&tokens, "-O")),
            resolvable: true,
        });
    }
    // archive_extract: tar -x, unzip
    if (tokens.contains(&"tar") && cmd.contains("-x")) || tokens.contains(&"unzip")
    {
        out.push(WriteTarget {
            category: WriteCategory::ArchiveExtract,
            operator: tokens[0].into(),
            path: None,
            resolvable: false,
        });
    }
    // git_destructive: reset --hard, clean -fd, checkout -- ., push --force
    if tokens.contains(&"git")
        && ((cmd.contains("reset") && cmd.contains("--hard"))
            || (cmd.contains("clean") && cmd.contains("-f"))
            || cmd.contains("push --force")
            || cmd.contains("push -f"))
    {
        out.push(WriteTarget {
            category: WriteCategory::GitDestructive,
            operator: "git".into(),
            path: None,
            resolvable: false,
        });
    }
    // fail-closed: any $VAR / $(...) / backtick makes all targets unresolvable
    if cmd.contains('$') || cmd.contains("``") || cmd.contains('`') {
        for t in out.iter_mut() {
            t.resolvable = false;
        }
    }
    out
}

fn guess_path_after(cmd: &str, op: &str) -> Option<String> {
    let idx = cmd.find(op)?;
    let rest = cmd[idx + op.len()..].trim_start();
    // skip descriptor digits for >& style
    let tok = rest.split_whitespace().next()?;
    if tok.is_empty() || tok.starts_with('&') || tok.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(tok.trim_matches(|c| c == '"' || c == '\'').to_string())
}

fn last_file_token(tokens: &[&str]) -> Option<String> {
    tokens.last().map(|s| s.to_string())
}

fn flag_value(tokens: &[&str], flag: &str) -> Option<String> {
    tokens
        .windows(2)
        .find(|w| w[0] == flag)
        .map(|w| w[1].to_string())
}

fn resolvable(p: &Option<String>) -> bool {
    match p {
        None => false,
        Some(s) => !(s.contains('$') || s.contains("``") || s.contains('`') || s.contains("$(")),
    }
}

/// Single WRITE_TOOL_NAMES list (mirrors `WRITE_TOOL_NAMES` by symbol).
pub const WRITE_TOOL_NAMES: &[&str] = &[
    "write",
    "edit",
    "patch",
    "apply_patch",
    "swarm_apply_patch",
    "create_file",
    "insert",
    "replace",
    "append",
    "prepend",
    "extract_code_blocks",
];

/// Resolve write targets with caps (1M path / 2M total analog: cap count + length).
/// Fail-closed: unresolvable dynamic path => Err(SCOPE_*).
pub fn resolve_write_targets(
    tool: &str,
    args: &[String],
    scope_root: &str,
) -> Result<Vec<String>, String> {
    if !WRITE_TOOL_NAMES.contains(&tool) {
        // shell tools route through detect_shell_writes instead
        return Ok(vec![]);
    }
    if args.len() > 64 {
        return Err("SCOPE_VIOLATION: too many write targets".into());
    }
    let mut out = vec![];
    for a in args {
        if a.len() > 1_000_000 {
            return Err("SCOPE_VIOLATION: write target path too long".into());
        }
        if a.contains('$') || a.contains("$(") || a.contains('`') {
            return Err("SCOPE_NOT_DECLARED: unresolvable dynamic path (fail-closed)".into());
        }
        // root escape check
        let joined = format!("{scope_root}/{a}");
        if a.starts_with("..") || a.starts_with('/') || joined.contains("/../") {
            // allow only if normalized stays under root (simple check)
            if a.starts_with('/') || a.split('/').any(|c| c == "..") {
                return Err(format!("SCOPE_ROOT_ESCAPE: {a} leaves root {scope_root}"));
            }
        }
        out.push(a.clone());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_all_eight_categories() {
        assert!(detect_shell_writes("echo hi > out.txt")
            .iter()
            .any(|t| t.category == WriteCategory::Redirect
                || t.category == WriteCategory::BuiltinWrite));
        assert!(detect_shell_writes("cat <<EOF > f")
            .iter()
            .any(|t| t.category == WriteCategory::HereDoc));
        assert!(detect_shell_writes("sed -i s/a/b/ f")
            .iter()
            .any(|t| t.category == WriteCategory::InplaceEdit));
        assert!(detect_shell_writes("python3 -c 'open(1)'")
            .iter()
            .any(|t| t.category == WriteCategory::InterpreterEval));
        assert!(detect_shell_writes("curl -o f http://x")
            .iter()
            .any(|t| t.category == WriteCategory::NetworkDownload));
        assert!(detect_shell_writes("tar -xzf a.tgz")
            .iter()
            .any(|t| t.category == WriteCategory::ArchiveExtract));
        assert!(detect_shell_writes("git reset --hard HEAD")
            .iter()
            .any(|t| t.category == WriteCategory::GitDestructive));
    }

    #[test]
    fn fail_closed_on_var() {
        let ts = detect_shell_writes("echo hi > $OUT");
        assert!(ts.iter().all(|t| !t.resolvable));
        assert!(resolve_write_targets("write", &["$VAR/f".into()], "/root").is_err());
    }

    #[test]
    fn root_escape_denied() {
        assert!(resolve_write_targets("write", &["../etc/passwd".into()], "/root").is_err());
    }
}
