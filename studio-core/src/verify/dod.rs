//! Definition-of-Done receipts: declared write-intent vs actual delta.
//! Tap-outs (prose "done" without delta, "next steps" plans) => `blocked` +
//! rewind. The receipt is evaluated ONLY against the frozen snapshot (the
//! caller proves [`crate::verify::FrozenCandidate::verify_tree`] first).
//!
//! Port of v1 `studio-core/src/verify/dod.rs` (same commit as the phase
//! `c1e49e97…` tree, FILE_HASH_ONLY). Logic kept; `TapOut::Blocked` reason
//! strings unchanged so gate history stays comparable.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DodCheck {
    /// Declared write-intent paths (a task without one is not dispatchable).
    pub declared_paths: Vec<String>,
    /// Actual delta paths vs the frozen snapshot.
    pub actual_paths: Vec<String>,
    /// Task claimed done with zero file delta?
    pub claims_done: bool,
    /// Free-text completion message (scanned for tap-out patterns).
    pub message: String,
    /// Evidence: green tests + syntax pass must both be true to land.
    pub tests_green: bool,
    pub syntax_ok: bool,
    /// Diagnostics budget: audits + lints show zero NEW findings.
    pub diagnostics_budget_ok: bool,
    /// Scope completion: files_done / files_planned (2/7 = 0.29 => refuse).
    pub files_done: usize,
    pub files_planned: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TapOut {
    /// Genuine done with evidence.
    Done,
    /// Blocked receipt with reason; worker rewinds.
    Blocked(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DodReceipt {
    pub verdict: TapOut,
    pub scope_ratio: f64,
    pub reasons: Vec<String>,
}

impl DodReceipt {
    pub fn done(&self) -> bool {
        self.verdict == TapOut::Done
    }
}

/// Deterministic tap-out patterns: prose completion without code delta.
fn is_tap_out_prose(msg: &str) -> bool {
    let m = msg.to_lowercase();
    [
        "here's what's next",
        "here is what's next",
        "next steps",
        "will do next",
        "plan going forward",
        "as a next step",
    ]
    .iter()
    .any(|p| m.contains(p))
}

/// Placeholder scan: TODO/FIXME/XXX/placeholder/lorem in delivered
/// message/delta names.
fn has_placeholders(msg: &str) -> bool {
    let m = msg.to_lowercase();
    ["todo", "fixme", "xxx", "placeholder", "lorem ipsum", "tbd"]
        .iter()
        .any(|p| m.contains(p))
}

/// Tree-sitter syntax gate: real parse for Rust; balanced-delimiter heuristic
/// fallback for other languages (unknown grammars never fail the write).
pub fn syntax_check(content: &str, lang: &str) -> bool {
    match lang {
        "rs" => {
            let mut parser = tree_sitter::Parser::new();
            let language = tree_sitter_rust::LANGUAGE;
            if parser.set_language(&language.into()).is_err() {
                return syntax_heuristic_ok(content);
            }
            match parser.parse(content, None) {
                Some(tree) => !tree.root_node().has_error(),
                None => false,
            }
        }
        _ => syntax_heuristic_ok(content),
    }
}

/// Balanced-delimiter syntax heuristic (fallback for non-Rust languages).
pub fn syntax_heuristic_ok(content: &str) -> bool {
    let mut stack: Vec<char> = vec![];
    let mut in_str: Option<char> = None;
    let mut prev = '\0';
    for c in content.chars() {
        if let Some(q) = in_str {
            if c == q && prev != '\\' {
                in_str = None;
            }
        } else if c == '"' || c == '\'' {
            in_str = Some(c);
        } else if "([{".contains(c) {
            stack.push(c);
        } else if ")]}".contains(c) {
            let open = stack.pop();
            let expect = match c {
                ')' => '(',
                ']' => '[',
                _ => '{',
            };
            if open != Some(expect) {
                return false;
            }
        }
        prev = c;
    }
    stack.is_empty() && in_str.is_none()
}

pub fn evaluate_done(check: &DodCheck) -> DodReceipt {
    let mut reasons = vec![];
    let ratio = if check.files_planned == 0 {
        1.0
    } else {
        check.files_done as f64 / check.files_planned as f64
    };

    // 1. Tap-out prose => blocked + rewind (adversarial refute-done).
    if is_tap_out_prose(&check.message) {
        reasons.push("tap-out prose detected (next-steps without delta)".to_string());
    }
    // 2. Claims done with zero delta => blocked.
    if check.claims_done && check.actual_paths.is_empty() {
        reasons.push("claims done with zero file delta".to_string());
    }
    // 3. Declared intent vs actual delta: every actual path must be declared
    //    (no scope creep without declaration), and at least one declared path
    //    must appear in delta.
    let declared: HashSet<&str> = check.declared_paths.iter().map(|s| s.as_str()).collect();
    let undeclared: Vec<&String> = check
        .actual_paths
        .iter()
        .filter(|p| !declared.contains(p.as_str()))
        .collect();
    if !undeclared.is_empty() {
        reasons.push(format!("undeclared paths in delta: {undeclared:?}"));
    }
    if !check.declared_paths.is_empty()
        && !check
            .actual_paths
            .iter()
            .any(|p| declared.contains(p.as_str()))
    {
        reasons.push("no declared path present in delta".to_string());
    }
    // 4. Scope-completion-ratio gate.
    if ratio < 0.8 {
        reasons.push(format!("scope ratio {ratio:.2} < 0.80"));
    }
    // 5. Placeholders.
    if has_placeholders(&check.message) {
        reasons.push("placeholder markers present".to_string());
    }
    // 6. Evidence gates: green tests + syntax.
    if !check.tests_green {
        reasons.push("tests not green".to_string());
    }
    if !check.syntax_ok {
        reasons.push("syntax check failed".to_string());
    }
    if !check.diagnostics_budget_ok {
        reasons.push("diagnostics budget exceeded (new findings)".to_string());
    }

    let verdict = if reasons.is_empty() {
        TapOut::Done
    } else {
        TapOut::Blocked(reasons.join("; "))
    };
    DodReceipt {
        verdict,
        scope_ratio: ratio,
        reasons,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn ok_check() -> DodCheck {
        DodCheck {
            declared_paths: vec!["a.rs".into()],
            actual_paths: vec!["a.rs".into()],
            claims_done: true,
            message: "implemented feature".into(),
            tests_green: true,
            syntax_ok: true,
            diagnostics_budget_ok: true,
            files_done: 5,
            files_planned: 5,
        }
    }

    #[test]
    fn accepts_evidence_backed_done() {
        let r = evaluate_done(&ok_check());
        assert_eq!(r.verdict, TapOut::Done);
        assert!(r.done());
    }

    #[test]
    fn rejects_tap_out_without_delta() {
        let c = DodCheck {
            declared_paths: vec!["a.rs".into(), "b.rs".into()],
            actual_paths: vec![],
            claims_done: true,
            message: "Here's what's next: finish b.rs".into(),
            tests_green: true,
            syntax_ok: true,
            diagnostics_budget_ok: true,
            files_done: 2,
            files_planned: 7,
        };
        let r = evaluate_done(&c);
        assert!(matches!(r.verdict, TapOut::Blocked(_)));
        assert!(r.scope_ratio < 0.3);
    }

    #[test]
    fn rejects_undeclared_scope_creep() {
        let mut c = ok_check();
        c.actual_paths = vec!["a.rs".into(), "evil.rs".into()];
        assert!(matches!(evaluate_done(&c).verdict, TapOut::Blocked(_)));
    }

    #[test]
    fn rejects_red_tests() {
        let mut c = ok_check();
        c.tests_green = false;
        assert!(matches!(evaluate_done(&c).verdict, TapOut::Blocked(_)));
    }

    #[test]
    fn rejects_low_scope_ratio() {
        let mut c = ok_check();
        c.files_done = 2;
        c.files_planned = 7;
        let r = evaluate_done(&c);
        assert!(matches!(r.verdict, TapOut::Blocked(_)));
        assert!(r.reasons.iter().any(|x| x.contains("scope ratio")));
    }

    #[test]
    fn syntax_heuristic_catches_unbalanced() {
        assert!(syntax_heuristic_ok("fn f() { println!(\"hi\"); }"));
        assert!(!syntax_heuristic_ok("fn f() { (] }"));
    }

    #[test]
    fn tree_sitter_gates_rust() {
        assert!(syntax_check("fn f() -> i32 { 1 }", "rs"));
        assert!(!syntax_check("fn f( { let x = ;", "rs"));
        assert!(syntax_check("def f():\n    pass", "py"));
    }
}
