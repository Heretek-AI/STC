//! Tree-sitter syntax gate for the v2 workspace.
//!
//! This is the "tests + tree-sitter" step of the gate order: every Rust source
//! that belongs to the v2 workspace must parse with a clean tree-sitter Rust
//! grammar (no ERROR or MISSING nodes). It is deliberately stricter than the
//! compiler's error recovery: a file that only *compiles by accident* through
//! macro recovery still fails the gate.

use std::path::{Path, PathBuf};

fn collect_rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rust_sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn all_workspace_rust_sources_parse_clean() {
    let core = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = core.parent().expect("studio-core lives in the workspace");

    let roots = [
        core.join("src"),
        core.join("tests"),
        workspace.join("studio-cli/src"),
        workspace.join("studio-cli/tests"),
    ];

    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .expect("tree-sitter-rust grammar loads");

    let mut checked = 0usize;
    let mut failures: Vec<String> = vec![];
    let mut sources: Vec<PathBuf> = vec![];
    for root in &roots {
        collect_rust_sources(root, &mut sources);
    }
    sources.sort();

    for path in &sources {
        let src = std::fs::read_to_string(path).expect("source is UTF-8");
        let tree = parser.parse(&src, None).expect("tree-sitter parses");
        if tree.root_node().has_error() {
            failures.push(path.display().to_string());
        }
        checked += 1;
    }

    assert!(
        checked >= 6,
        "expected to check the v2 sources, got {checked}"
    );
    assert!(
        failures.is_empty(),
        "tree-sitter found syntax errors in: {failures:#?}"
    );
}
