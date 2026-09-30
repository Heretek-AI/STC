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
}
