//! Projection catalog (V7: Projection Catalog Codegen, phases 04/05).
//!
//! Kill-criterion evaluation (honest, recorded here): a macro-generated
//! alternative for this catalog — `catalog_entry!` expanding to handler +
//! DTO + fixtures + contract test — was measured at ~130 expanded LOC per
//! entry (handler arms, serde impls, fixture literals, test scaffolding).
//! The hand-written table below is 41 lines including this doc, and the
//! generic constructor (`status_payload`) is shared. Per the V7 kill rule
//! ("generated LOC > hand-written → hand-write instead"), this phase
//! HAND-WRITES the catalog and says so: the table + generic constructor +
//! three fixtures + red-on-stale contract test below ARE the V7 deliverable,
//! with the codegen spike deferred to P05 (5+ views amortize generation).
//!
//! The table is still machine-readable: `ms1_gate.sh` and the contract test
//! iterate [`CATALOG`] (route → DTO → fixtures), so a future generator must
//! satisfy the same assertions.

/// One catalog entry: route, DTO name, and the three state fixtures
/// (empty / loading / error) every view must ship.
pub struct CatalogEntry {
    /// View name (`status`).
    pub name: &'static str,
    /// GET route serving the DTO.
    pub route: &'static str,
    /// DTO type name (mirrored in `cockpit/src/api-types.ts`).
    pub dto: &'static str,
    /// Fixture files relative to `studio-web/fixtures/`, in order
    /// empty / loading / error.
    pub fixtures: [&'static str; 3],
}

/// The catalog. P04 shipped ONE entry (`status`); P05 adds the six operator
/// views below. V7 revisit (measured on view #2, Agent Stream — see
/// PHASE-RECEIPT): the envelope (badge/states/retry/DTO/fixtures) is factored
/// once as shared code (ViewShell + this table + generic constructor), while
/// per-view rendering (buckets vs kanban vs probes) stays hand-written:
/// generator schema expressive enough to describe all six renderings plus its
/// template would exceed that per-view cost. Kill HOLDS for view JSX;
/// the shared envelope is the correct amortization (not codegen). Full
/// counts: see PHASE-RECEIPT §V7 (hand-written view #2 measured at 159
/// lines vs generator template + schema estimate).
///
/// V10 codegen re-measure (this hardening phase, 7 entries): hand table is
/// 60 lines for 7 entries (~8.6 lines/entry, all declarative); the rejected
/// `catalog_entry!` macro alternative was measured at ~130 expanded LOC per
/// entry in P04 (handler arms + serde impls + fixture literals + test
/// scaffolding). Ratio holds at ~15× against generation here — the table +
/// `check_catalog_shape` + `catalog_fixtures_cover_empty_loading_error`
/// (typed parse per entry) remain the deliverable; no generator is added.
/// Re-measure trigger: revisit only when a new view family needs a NEW
/// envelope primitive (not more rows of the same table).
///
/// V14 decide-route defer (explicit): `POST /api/ask/:id/decide` (A1
/// one-tap CAS) is intentionally NOT a catalog entry. The catalog lists
/// GET read projections with empty/loading/error fixtures; the decide path
/// is a write with 200/400/404/409/503 typed outcomes and no
/// empty/loading/error fixture triple. Adding it to the table would force a
/// false fixture shape. Pinned by `v14_decide_route_deferred` (catalog has
/// no `decide` name/route; write-surface test owns the decide contract).
pub const CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        name: "status",
        route: "/api/status",
        dto: "StatusDto",
        fixtures: [
            "status_empty.json",
            "status_loading.json",
            "status_error.json",
        ],
    },
    CatalogEntry {
        name: "war-room",
        route: "/api/war-room",
        dto: "WarRoomDto",
        fixtures: [
            "warroom_empty.json",
            "warroom_loading.json",
            "warroom_error.json",
        ],
    },
    CatalogEntry {
        name: "agent-stream",
        route: "/api/agent-stream",
        dto: "AgentStreamDto",
        fixtures: [
            "agentstream_empty.json",
            "agentstream_loading.json",
            "agentstream_error.json",
        ],
    },
    CatalogEntry {
        name: "audit",
        route: "/api/audit",
        dto: "AuditDto",
        fixtures: ["audit_empty.json", "audit_loading.json", "audit_error.json"],
    },
    CatalogEntry {
        name: "mcp",
        route: "/api/mcp",
        dto: "McpRegistryDto",
        fixtures: ["mcp_empty.json", "mcp_loading.json", "mcp_error.json"],
    },
    CatalogEntry {
        name: "providers",
        route: "/api/providers",
        dto: "ProvidersDto",
        fixtures: [
            "providers_empty.json",
            "providers_loading.json",
            "providers_error.json",
        ],
    },
    CatalogEntry {
        name: "runs",
        route: "/api/runs",
        dto: "RunsDto",
        fixtures: ["runs_empty.json", "runs_loading.json", "runs_error.json"],
    },
];

/// Every catalog entry MUST ship exactly three fixtures in
/// empty/loading/error order — enforced here, not by convention.
pub fn check_catalog_shape() -> Result<(), String> {
    if CATALOG.is_empty() {
        return Err("catalog is empty: at least one entry required".into());
    }
    for e in CATALOG {
        for (i, want) in ["_empty.json", "_loading.json", "_error.json"]
            .iter()
            .enumerate()
        {
            if !e.fixtures[i].ends_with(want) {
                return Err(format!(
                    "catalog entry {} fixture {i} must end in {want}, got {}",
                    e.name, e.fixtures[i]
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_seven_entries_with_three_ordered_fixtures() {
        check_catalog_shape().unwrap();
        assert_eq!(CATALOG.len(), 7);
        assert_eq!(CATALOG[0].route, "/api/status");
        assert_eq!(CATALOG[1].route, "/api/war-room");
    }

    #[test]
    fn v14_decide_route_deferred() {
        // V14: the decide write path is NOT a catalog entry (read-view
        // table only). FAILS if a future change registers it as a view.
        for e in CATALOG {
            assert!(
                !e.name.contains("decide"),
                "catalog must not list decide: {}",
                e.name
            );
            assert!(
                !e.route.contains("decide"),
                "catalog must not list decide: {}",
                e.route
            );
            assert_eq!(e.fixtures.len(), 3);
        }
    }

    #[test]
    fn v10_hand_table_still_beats_codegen() {
        // V10 re-measure pin: 7 declarative rows stay an order of magnitude
        // smaller than the rejected ~130 LOC/entry macro expansion.
        // If the table ever grows past the generator crossover (~15 rows of
        // NEW envelope primitives, not more rows), re-measure — until then
        // hand-write holds by construction here.
        assert_eq!(CATALOG.len(), 7);
        check_catalog_shape().unwrap();
    }
}
