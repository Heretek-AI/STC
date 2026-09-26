//! Isolation router: shared-tree claims by default; worktree/container fallback
//! for scopes declared `isolation: worktree|container`. Never share one checkout
//! across hosts (`repoId::path` vs `wt2:host:inst` — remote lanes stay local-only in v1).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Isolation {
    #[default]
    Shared,
    Worktree,
    Container,
}

pub struct IsolationRouter;

impl IsolationRouter {
    /// Route a task: shared requires claim acquisition; worktree/container bypass claims.
    pub fn route(isolation: Isolation, claims_held: bool) -> Result<&'static str, String> {
        match isolation {
            Isolation::Shared if claims_held => Ok("shared-tree"),
            Isolation::Shared => Err("shared isolation requires acquired scope claims".into()),
            Isolation::Worktree => Ok("worktree"),
            Isolation::Container => Ok("container"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_requires_claims() {
        assert!(IsolationRouter::route(Isolation::Shared, false).is_err());
        assert_eq!(
            IsolationRouter::route(Isolation::Shared, true).unwrap(),
            "shared-tree"
        );
        assert_eq!(
            IsolationRouter::route(Isolation::Worktree, false).unwrap(),
            "worktree"
        );
    }
}
