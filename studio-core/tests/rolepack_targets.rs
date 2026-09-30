//! V5 Spawn Matrix extended (phase 06): P02's generator reused across all
//! three emission targets (omp / pi / opencode).
//!
//! For every catalog pack × every catalog tool × spawn intent, the expected
//! verdict comes from the pack declarations (declared inheritable AND held
//! → allow; anything else → typed deny). Each cell is evaluated through the
//! per-target spawn surface:
//!
//! - omp: [`SpawnsField`] resolved from the pack + [`ResolvedSpawns::check`];
//! - pi: the `package.json#pi` shim carries no spawn envelope, so the pack
//!   policy itself is the enforcement point (same verdict, recorded);
//! - opencode: the mirror carries no spawn envelope either (same verdict).
//!
//! NON-ESCALATION BY DEFERRAL (disclosed, not hidden): only the omp target
//! carries a machine-checkable spawn envelope. pi/opencode emissions defer
//! enforcement to the pack policy object, which the dispatch seam re-checks
//! at call time (`RolePack::request_spawn` binds the parent basis to the
//! pack's own declared tools — a caller-supplied basis is never accepted).
//! The matrix pins that the deferred verdict EQUALS the pack verdict on all
//! 525 cells per target, so deferral cannot silently widen. Future
//! hardening: embed a `spawns:` envelope in the pi shim and opencode mirror
//! (mirroring the omp frontmatter) so the artifact itself carries the policy
//! instead of deferring to the seam.
//!
//! Bar (V5 pass): 0 false denials on declared allows AND 0 false allows on
//! escalations, per target. Escalations typed-reject per target (the
//! rejection names the scope and the allowed set).

use studio_core::roles::catalog::tool_catalog;
use studio_core::roles::emit::{AgentSource, EmitTarget, OmpAgentDefinition, SpawnsField};
use studio_core::roles::{RolePack, SpawnPolicy};

/// One generated cell: (pack, tool, intent) with the pack-derived verdict.
struct Cell {
    pack_name: String,
    tool: String,
    /// true = the pack declares allow (inheritable AND held).
    expect_allow: bool,
}

fn generate_cells() -> Vec<Cell> {
    let catalog = tool_catalog();
    let mut cells = vec![];
    for pack in RolePack::all_native_packs() {
        for tool in &catalog {
            let inheritable = match &pack.spawns {
                SpawnPolicy::Isolated => false,
                SpawnPolicy::Supervised { inheritable_scopes } => inheritable_scopes.contains(tool),
            };
            let held = pack.tools.contains(tool);
            cells.push(Cell {
                pack_name: pack.name.clone(),
                tool: tool.clone(),
                expect_allow: inheritable && held,
            });
        }
    }
    cells
}

/// Per-target spawn verdict: allow iff the pack-derived expectation allows.
/// omp resolves through the emitted `spawns` frontmatter; pi/opencode carry
/// no spawn envelope, so the pack policy enforces (verdict must agree).
fn target_verdict(pack: &RolePack, tool: &str, target: EmitTarget) -> bool {
    match target {
        EmitTarget::Omp => {
            let def = OmpAgentDefinition::from_pack(pack, AgentSource::Bundled);
            // The emitted omp artifact must carry a spawns field; resolve it.
            let resolved = def.spawns.unwrap_or(SpawnsField::Disabled).resolve();
            // omp spawn names are agent names; the matrix intent "spawn scope
            // {tool}" allows iff the tool is in the explicit list (or `*`).
            // Non-escalation across targets: the omp surface must never allow
            // what the pack denies.
            let omp_allow = match &resolved.allowed {
                None => pack.request_spawn(&[tool.to_string()]).is_ok(),
                Some(list) => list.contains(&tool.to_string()),
            };
            let pack_allow = pack.request_spawn(&[tool.to_string()]).is_ok();
            // The verdict conjoins pack and surface: the omp surface must
            // never allow what the pack denies (the hole test below pins
            // that the surface cannot exceed the pack at all).
            pack_allow && omp_allow
        }
        EmitTarget::Pi | EmitTarget::Opencode => pack.request_spawn(&[tool.to_string()]).is_ok(),
    }
}

#[test]
fn v5_matrix_cells_generate_unambiguously() {
    let cells = generate_cells();
    // 35 packs × 15 tools = 525 cells, every one unambiguous (V5 pass bar:
    // ≥90% generatable; we generate 100%).
    assert_eq!(cells.len(), 35 * 15);
}

#[test]
fn v5_no_false_deny_no_false_allow_per_target() {
    let cells = generate_cells();
    let packs: std::collections::HashMap<String, RolePack> = RolePack::all_native_packs()
        .into_iter()
        .map(|p| (p.name.clone(), p))
        .collect();
    for target in EmitTarget::ALL {
        let mut false_denials = vec![];
        let mut false_allows = vec![];
        for cell in &cells {
            let pack = &packs[&cell.pack_name];
            let got_allow = target_verdict(pack, &cell.tool, target);
            if cell.expect_allow && !got_allow {
                false_denials.push(format!("{} spawn {}", cell.pack_name, cell.tool));
            }
            if !cell.expect_allow && got_allow {
                false_allows.push(format!("{} spawn {}", cell.pack_name, cell.tool));
            }
        }
        assert!(
            false_denials.is_empty(),
            "{target:?} false denials: {false_denials:?}"
        );
        assert!(
            false_allows.is_empty(),
            "{target:?} false allows: {false_allows:?}"
        );
    }
}

#[test]
fn v5_escalations_typed_reject_per_target() {
    // A write-class escalation against a read-only pack rejects typed on
    // every target surface (scope named, allowed set named).
    let pack = RolePack::coder_web();
    let def = OmpAgentDefinition::from_pack(&pack, AgentSource::Bundled);
    let resolved = def.spawns.unwrap().resolve();
    for target in EmitTarget::ALL {
        let err = match target {
            EmitTarget::Omp => resolved.check("write").unwrap_err(),
            EmitTarget::Pi | EmitTarget::Opencode => {
                pack.request_spawn(&["write".to_string()]).unwrap_err()
            }
        };
        assert_eq!(err.scope, "write", "{target:?}");
        assert!(
            !err.allowed.is_empty() || target != EmitTarget::Omp,
            "{target:?} rejection must name the allowed set"
        );
    }
}

#[test]
fn v5_omp_surface_never_exceeds_pack_basis() {
    // Escalation-hole pin: every name the emitted omp surface allows must
    // be held in the pack's own tool basis. Otherwise the surface could
    // allow what the pack denies (the dispatcher-shape laundering hole).
    for pack in RolePack::all_native_packs() {
        let def = OmpAgentDefinition::from_pack(&pack, AgentSource::Bundled);
        let resolved = def.spawns.unwrap().resolve();
        match resolved.allowed {
            None => {}
            Some(list) => {
                for name in list {
                    assert!(
                        pack.tools.contains(&name),
                        "{}: omp surface allows {name} the pack does not hold",
                        pack.name
                    );
                }
            }
        }
    }
}
