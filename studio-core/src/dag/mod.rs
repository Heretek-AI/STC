//! Typed DAG artifact with strict referential integrity.
//! Port anchor (by symbol): agent-swarm `dependsOn` (arbitrary strings — the gap this fixes).

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowNode {
    pub id: String,
    pub r#type: String,
    pub config: serde_json::Value,
    pub next: Vec<String>,
    pub inputs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDefinition {
    pub nodes: Vec<WorkflowNode>,
    pub entry: String,
}

#[derive(Debug, Error)]
pub enum DagError {
    #[error("unknown dependency: node {node} depends on unknown {dep}")]
    UnknownDependency { node: String, dep: String },
    #[error("no single entry: found {0:?}")]
    BadEntry(Vec<String>),
    #[error("unreachable nodes: {0:?}")]
    Unreachable(Vec<String>),
    #[error("duplicate node id: {0}")]
    Duplicate(String),
}

impl WorkflowDefinition {
    /// Validate referential integrity: every depends_on/inputs/next resolves,
    /// single entry, every node reachable. Rejects freeform strings at init.
    pub fn validate(&self) -> Result<(), DagError> {
        let ids: HashSet<&str> = self.nodes.iter().map(|n| n.id.as_str()).collect();
        if self.nodes.len() != ids.len() {
            let mut seen = HashSet::new();
            for n in &self.nodes {
                if !seen.insert(n.id.as_str()) {
                    return Err(DagError::Duplicate(n.id.clone()));
                }
            }
        }
        if !ids.contains(self.entry.as_str()) {
            return Err(DagError::BadEntry(vec![self.entry.clone()]));
        }
        // entry must have no inputs (single entry check: exactly one node with empty inputs)
        let entries: Vec<String> = self
            .nodes
            .iter()
            .filter(|n| n.inputs.is_empty())
            .map(|n| n.id.clone())
            .collect();
        if entries != vec![self.entry.clone()] {
            return Err(DagError::BadEntry(entries));
        }
        for n in &self.nodes {
            for dep in n.inputs.iter().chain(n.next.iter()) {
                if !ids.contains(dep.as_str()) {
                    return Err(DagError::UnknownDependency {
                        node: n.id.clone(),
                        dep: dep.clone(),
                    });
                }
            }
        }
        // reachability from entry via next edges
        let mut children: HashMap<&str, Vec<&str>> = HashMap::new();
        for n in &self.nodes {
            for nx in &n.next {
                children.entry(n.id.as_str()).or_default().push(nx.as_str());
            }
        }
        let mut seen: HashSet<&str> = HashSet::new();
        let mut stack = vec![self.entry.as_str()];
        while let Some(cur) = stack.pop() {
            if !seen.insert(cur) {
                continue;
            }
            if let Some(ch) = children.get(cur) {
                stack.extend(ch.iter().copied());
            }
        }
        // also follow inputs-reverse: nodes reachable only via inputs still count if linked;
        // require union of next-reachable == all ids (DAG visualizer uses next)
        if seen.len() != ids.len() {
            let missing: Vec<String> = ids
                .iter()
                .filter(|id| !seen.contains(**id))
                .map(|s| s.to_string())
                .collect();
            return Err(DagError::Unreachable(missing));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn node(id: &str, inputs: &[&str], next: &[&str]) -> WorkflowNode {
        WorkflowNode {
            id: id.into(),
            r#type: "coder".into(),
            config: json!({}),
            next: next.iter().map(|s| s.to_string()).collect(),
            inputs: inputs.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn accepts_valid_dag() {
        let wf = WorkflowDefinition {
            entry: "t0".into(),
            nodes: vec![
                node("t0", &[], &["t1", "t2"]),
                node("t1", &["t0"], &["t2"]),
                node("t2", &["t0", "t1"], &[]),
            ],
        };
        wf.validate().unwrap();
    }

    #[test]
    fn rejects_unknown_dependency() {
        let wf = WorkflowDefinition {
            entry: "t0".into(),
            nodes: vec![node("t0", &[], &[]), node("t1", &["nope"], &[])],
        };
        assert!(matches!(
            wf.validate(),
            Err(DagError::UnknownDependency { .. })
        ));
    }
}
