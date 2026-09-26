//! L4 opt-in semantic index. Embeddings are pluggable; a reranker is MANDATORY:
//! `search` without one fails closed (unranked vector hits never reach the agent).

#[derive(Debug, Clone)]
pub struct RerankedHit {
    pub doc_id: String,
    pub score: f64,
}

/// Pluggable embedding + rerank backend (opt-in; no default network calls).
pub trait SemanticIndex {
    fn embed(&self, text: &str) -> Vec<f32>;
    /// Rerank candidate doc_ids for the query. Returning None means "no reranker".
    fn rerank(&self, query: &str, candidates: &[String]) -> Option<Vec<RerankedHit>>;
}

/// Search with mandatory reranker gate.
pub fn search(
    index: &dyn SemanticIndex,
    query: &str,
    candidates: &[String],
) -> Result<Vec<RerankedHit>, String> {
    match index.rerank(query, candidates) {
        Some(hits) => Ok(hits),
        None => Err(
            "semantic search refused: L4 requires a reranker (unranked hits never served)".into(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoRerank;
    impl SemanticIndex for NoRerank {
        fn embed(&self, _: &str) -> Vec<f32> {
            vec![]
        }
        fn rerank(&self, _: &str, _: &[String]) -> Option<Vec<RerankedHit>> {
            None
        }
    }

    #[test]
    fn refuses_without_reranker() {
        assert!(search(&NoRerank, "q", &["d".into()]).is_err());
    }
}
