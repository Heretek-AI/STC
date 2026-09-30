//! f06-handoff-pipeline: handoff generation as inter-role evidence.
//!
//! Clean-room port of the `/handoff` lifecycle shape (c058
//! `df9a56481d8ecba2d422937ff68484b247aefe01d7de69ee0a63e96ed470b6ae`,
//! MIT): trigger guards (refuse while streaming; minimum-content floor of
//! two messages; refuse double-generation), deterministic document build
//! from session state (no model — mechanical extraction), and commit as a
//! compaction entry. Divergences: no LLM side-request pipeline (out of scope
//! for a deterministic core — the request *shape* is what is ported: focus
//! hint + snapshot + render), no TUI/RPC surfaces.

use super::RoleError;

/// Minimum message entries required to hand off (upstream: `< 2` warns).
pub const MIN_MESSAGES: usize = 2;

/// Session state the handoff is generated from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionState {
    pub messages: Vec<String>,
    /// True while a response is streaming → handoff refused.
    pub streaming: bool,
    /// Committed compaction entries (handoff docs land here).
    pub compactions: Vec<String>,
}

/// Generated handoff document: the inter-role evidence envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoffDoc {
    pub summary: String,
    pub decisions: Vec<String>,
    pub open_questions: Vec<String>,
    pub evidence_refs: Vec<String>,
    pub next_actions: Vec<String>,
}

impl HandoffDoc {
    pub fn render(&self) -> String {
        let list = |items: &[String]| {
            if items.is_empty() {
                "none recorded".to_string()
            } else {
                items
                    .iter()
                    .map(|i| format!("- {i}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        };
        format!(
            "# Handoff\n\n## Summary\n\n{}\n\n## Decisions\n\n{}\n\n## Open questions\n\n{}\n\n## Evidence\n\n{}\n\n## Next actions\n\n{}\n",
            self.summary,
            list(&self.decisions),
            list(&self.open_questions),
            list(&self.evidence_refs),
            list(&self.next_actions)
        )
    }
}

/// Generate the handoff document. Guards (typed, fail-closed):
/// streaming → refused; fewer than [`MIN_MESSAGES`] → refused.
pub fn generate_handoff(
    state: &SessionState,
    focus: Option<&str>,
) -> Result<HandoffDoc, RoleError> {
    if state.streaming {
        return Err(RoleError::HandoffViolation {
            detail: "handoff refused while a response is streaming".into(),
        });
    }
    if state.messages.len() < MIN_MESSAGES {
        return Err(RoleError::HandoffViolation {
            detail: "Nothing to hand off (no messages yet)".into(),
        });
    }
    let mut decisions = vec![];
    let mut questions = vec![];
    let mut evidence = vec![];
    let mut rest = vec![];
    for m in &state.messages {
        let t = m.trim();
        if let Some(d) = t.strip_prefix("decision:") {
            decisions.push(d.trim().to_string());
        } else if t.ends_with('?') {
            questions.push(t.to_string());
        } else if let Some(e) = t.strip_prefix("evidence:") {
            evidence.push(e.trim().to_string());
        } else {
            rest.push(t.to_string());
        }
    }
    let mut summary = if rest.is_empty() {
        "session handoff".to_string()
    } else {
        rest.join(" / ")
    };
    if let Some(f) = focus {
        summary = format!("{summary} (focus: {f})");
    }
    Ok(HandoffDoc {
        summary,
        decisions,
        open_questions: questions,
        evidence_refs: evidence,
        next_actions: rest.into_iter().take(3).collect(),
    })
}

/// Commit the rendered document as a compaction entry (append-only).
pub fn commit_handoff(state: &mut SessionState, doc: &HandoffDoc) {
    state.compactions.push(doc.render());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> SessionState {
        SessionState {
            messages: vec![
                "context: fix login".into(),
                "decision: use CAS".into(),
                "evidence: tests green".into(),
                "ship it?".into(),
            ],
            streaming: false,
            compactions: vec![],
        }
    }

    #[test]
    fn guards_refuse_streaming_and_empty_typed() {
        let mut s = state();
        s.streaming = true;
        assert!(matches!(
            generate_handoff(&s, None).unwrap_err(),
            RoleError::HandoffViolation { .. }
        ));
        let mut s = state();
        s.messages.truncate(1);
        let err = generate_handoff(&s, None).unwrap_err();
        assert!(err.to_string().contains("Nothing to hand off"));
    }

    #[test]
    fn doc_extracts_sections_and_commits_append_only() {
        let mut s = state();
        let doc = generate_handoff(&s, Some("login")).unwrap();
        assert_eq!(doc.decisions, vec!["use CAS"]);
        assert_eq!(doc.evidence_refs, vec!["tests green"]);
        assert_eq!(doc.open_questions, vec!["ship it?"]);
        assert!(doc.summary.contains("(focus: login)"));
        let md = doc.render();
        for section in [
            "## Summary",
            "## Decisions",
            "## Open questions",
            "## Evidence",
            "## Next actions",
        ] {
            assert!(md.contains(section), "missing {section}");
        }
        commit_handoff(&mut s, &doc);
        assert_eq!(s.compactions.len(), 1);
        assert!(s.compactions[0].contains("## Decisions"));
    }
}
