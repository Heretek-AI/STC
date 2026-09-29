//! V5 Spawn-Policy Decision Matrix (phase 02): 6 roles × 15 tools × 3 intents.
//!
//! 270 cells, each evaluated against the real [`SpawnPolicy::request`] / tool
//! declarations. Expectations come from an INDEPENDENT reference (plain
//! subset logic over the hand-written declarations below — not by calling
//! the implementation), so a wrong implementation shows up as a false
//! denial (expected allow, got deny) or false allow (expected deny, got
//! allow). Bar: 0 false denials on declared allows; this suite pins 0 false
//! denials AND 0 false allows across all 270 cells.
//!
//! Cell semantics for (role R, tool T, intent I):
//! - Read intent: allow iff T == "read" and R declares T.
//! - Write intent: allow iff T == "write" and R declares T.
//! - Spawn intent: the child proposes scope {T}; allow iff T is declared by
//!   R AND R's policy inherits T AND R's parent scope holds T (the
//!   anti-laundering intersection: `dispatcher` inherits `sast` on paper but
//!   does not hold it, so spawning `sast` must deny).

use studio_core::roles::{RolePack, SpawnPolicy};

fn catalog() -> Vec<String> {
    [
        "read",
        "write",
        "edit",
        "patch",
        "runProcess",
        "code_search",
        "tool_open",
        "kv_get",
        "web_search",
        "plan_open",
        "dag_commit",
        "syntax_check",
        "sast",
        "sbom",
        "retrieve_docs",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

fn flagship_roles() -> Vec<RolePack> {
    vec![
        RolePack {
            name: "coder-web".into(),
            version: "1".into(),
            tools: s(&[
                "read",
                "write",
                "edit",
                "runProcess",
                "code_search",
                "tool_open",
                "kv_get",
            ]),
            skill_lockfile_hash: RolePack::lockfile_hash("coder-web-skills-v1"),
            system_prompt: "web coding specialist".into(),
            model_slot: "coder.primary".into(),
            harness_profile: Default::default(),
            spawns: SpawnPolicy::Supervised {
                inheritable_scopes: s(&["read", "code_search"]),
            },
            output_schema: None,
        },
        RolePack {
            name: "reviewer".into(),
            version: "1".into(),
            tools: s(&["read", "code_search", "tool_open", "kv_get"]),
            skill_lockfile_hash: RolePack::lockfile_hash("reviewer-skills-v1"),
            system_prompt: "evidence-only reviewer".into(),
            model_slot: "reviewer".into(),
            harness_profile: Default::default(),
            spawns: SpawnPolicy::Isolated,
            output_schema: None,
        },
        RolePack {
            name: "dispatcher".into(),
            version: "1".into(),
            tools: s(&["read", "plan_open", "dag_commit", "kv_get", "tool_open"]),
            skill_lockfile_hash: RolePack::lockfile_hash("dispatcher-skills-v1"),
            system_prompt: "task dispatcher".into(),
            model_slot: "manager".into(),
            harness_profile: Default::default(),
            // NOTE: `sast` is inheritable on paper but NOT held — the
            // anti-laundering intersection must still deny spawning it.
            spawns: SpawnPolicy::Supervised {
                inheritable_scopes: s(&["read", "plan_open", "sast"]),
            },
            output_schema: None,
        },
        RolePack {
            name: "researcher".into(),
            version: "1".into(),
            tools: s(&[
                "read",
                "web_search",
                "retrieve_docs",
                "code_search",
                "kv_get",
            ]),
            skill_lockfile_hash: RolePack::lockfile_hash("researcher-skills-v1"),
            system_prompt: "literature researcher".into(),
            model_slot: "researcher".into(),
            harness_profile: Default::default(),
            spawns: SpawnPolicy::Isolated,
            output_schema: None,
        },
        RolePack {
            name: "auditor".into(),
            version: "1".into(),
            tools: s(&["read", "sast", "sbom", "tool_open", "kv_get"]),
            skill_lockfile_hash: RolePack::lockfile_hash("auditor-skills-v1"),
            system_prompt: "security auditor".into(),
            model_slot: "reviewer".into(),
            harness_profile: Default::default(),
            spawns: SpawnPolicy::Isolated,
            output_schema: None,
        },
        RolePack {
            name: "releaser".into(),
            version: "1".into(),
            tools: s(&[
                "read",
                "write",
                "runProcess",
                "syntax_check",
                "tool_open",
                "kv_get",
            ]),
            skill_lockfile_hash: RolePack::lockfile_hash("releaser-skills-v1"),
            system_prompt: "release engineer".into(),
            model_slot: "manager".into(),
            harness_profile: Default::default(),
            spawns: SpawnPolicy::Supervised {
                inheritable_scopes: s(&["read"]),
            },
            output_schema: None,
        },
    ]
}

#[derive(Clone, Copy)]
enum Intent {
    /// Child proposes the catalog tool under test, evaluated against the
    /// role's own policy with the parent basis bound to declared tools.
    PackPolicy,
    /// Same proposal against a forced-Isolated baseline: must deny
    /// everywhere (fail-closed baseline; any allow is a critical impl bug).
    IsolatedBaseline,
    /// Same proposal against a FORGED maximally-permissive basis (parent
    /// claims to hold all 15 tools): documents that the primitive trusts its
    /// basis, and cross-checks that the bound pack entry point denies
    /// whatever the forgery would grant above declared authority.
    ForgedBasis,
}

const INTENTS: [Intent; 3] = [
    Intent::PackPolicy,
    Intent::IsolatedBaseline,
    Intent::ForgedBasis,
];

/// Maximally permissive forged parent basis: claims every catalog tool.
fn forged_basis() -> Vec<String> {
    catalog()
}

/// Independent reference: plain subset logic over declarations (does NOT call
/// the implementation under test). QA-R2: every intent exercises
/// `SpawnPolicy::request` — the previous Read/Write intents bypassed it
/// (only 90/270 cells called the implementation).
fn expect_allow(role: &RolePack, tool: &str, intent: Intent) -> bool {
    match intent {
        Intent::PackPolicy => {
            if !role.tools.iter().any(|t| t == tool) {
                return false;
            }
            match &role.spawns {
                SpawnPolicy::Isolated => false,
                SpawnPolicy::Supervised { inheritable_scopes } => {
                    inheritable_scopes.iter().any(|x| x == tool)
                        && role.tools.iter().any(|x| x == tool)
                }
            }
        }
        Intent::IsolatedBaseline => false,
        Intent::ForgedBasis => match &role.spawns {
            SpawnPolicy::Isolated => false,
            SpawnPolicy::Supervised { inheritable_scopes } => {
                inheritable_scopes.iter().any(|x| x == tool)
            }
        },
    }
}

fn actual_allow(role: &RolePack, tool: &str, intent: Intent) -> bool {
    let child = vec![tool.to_string()];
    match intent {
        // Bound path: parent basis is the pack's declared tools.
        Intent::PackPolicy => role.request_spawn(&child).is_ok(),
        Intent::IsolatedBaseline => SpawnPolicy::Isolated.request(&child, &role.tools).is_ok(),
        // Raw primitive with the forged basis (trusts its inputs).
        Intent::ForgedBasis => role.spawns.request(&child, &forged_basis()).is_ok(),
    }
}

#[test]
fn spawn_matrix_270_cells_zero_false_denials_zero_false_allows() {
    let cat = catalog();
    let roles = flagship_roles();
    assert_eq!(roles.len(), 6);
    assert_eq!(cat.len(), 15);
    // Every flagship role is coherent first (parity + secrets), so matrix
    // denials can only come from policy, never from phantom tools.
    for role in &roles {
        role.check_parity(&cat).unwrap();
        role.check_no_raw_secrets().unwrap();
    }

    let mut cells = 0;
    let mut allows = 0;
    let mut false_denials = vec![];
    let mut false_allows = vec![];
    for role in &roles {
        for tool in &cat {
            for intent in INTENTS {
                cells += 1;
                let intent_name = match intent {
                    Intent::PackPolicy => "pack-policy",
                    Intent::IsolatedBaseline => "isolated-baseline",
                    Intent::ForgedBasis => "forged-basis",
                };
                let expected = expect_allow(role, tool, intent);
                let actual = actual_allow(role, tool, intent);
                if expected {
                    allows += 1;
                }
                if expected && !actual {
                    false_denials.push(format!("{} × {tool} × {intent_name}", role.name));
                }
                if !expected && actual {
                    false_allows.push(format!("{} × {tool} × {intent_name}", role.name));
                }
            }
        }
    }
    assert_eq!(cells, 270, "6 roles × 15 tools × 3 intents");
    assert!(
        false_denials.is_empty(),
        "false denials on declared allows: {false_denials:?}"
    );
    assert!(
        false_allows.is_empty(),
        "false allows above authority: {false_allows:?}"
    );
    // The matrix must contain BOTH allows and denies, or it proves nothing.
    assert!(
        allows > 0 && allows < cells,
        "degenerate matrix ({allows}/{cells} allow)"
    );
}

/// The anti-laundering intersection, isolated: inheritable-but-unheld scopes
/// never propagate, even under Supervised.
#[test]
fn unheld_inheritable_scope_never_propagates() {
    let roles = flagship_roles();
    let dispatcher = roles.iter().find(|r| r.name == "dispatcher").unwrap();
    // `sast` is inheritable on paper but the dispatcher does not hold it.
    assert!(!dispatcher.tools.contains(&"sast".to_string()));
    assert!(dispatcher
        .spawns
        .request(&["sast".to_string()], &dispatcher.tools)
        .is_err());
    // Held + inheritable DOES propagate through the bound entry point.
    assert!(dispatcher.request_spawn(&["plan_open".to_string()]).is_ok());
}

/// Forged-basis contrast across the matrix: every cell the forgery would
/// grant ABOVE declared authority must still deny through the bound pack
/// entry point. (QA-R2: the binding is load-bearing, not decorative.)
#[test]
fn forged_basis_never_beats_the_bound_pack() {
    let roles = flagship_roles();
    let cat = catalog();
    let mut contrast_cells = 0;
    for role in &roles {
        for tool in &cat {
            let child = vec![tool.clone()];
            let forged_allows = role.spawns.request(&child, &forged_basis()).is_ok();
            let bound_allows = role.request_spawn(&child).is_ok();
            if forged_allows && !bound_allows {
                // The interesting cells: forgery grants, binding denies.
                contrast_cells += 1;
            }
            // The binding must NEVER grant above what the pack declares:
            // bound allows imply the tool is declared AND inheritable-held.
            if bound_allows {
                assert!(role.tools.contains(tool), "{} × {tool}", role.name);
            }
        }
    }
    // dispatcher×sast is the designed contrast cell (inheritable, unheld).
    assert!(contrast_cells > 0, "no forgery-vs-binding contrast found");
}
