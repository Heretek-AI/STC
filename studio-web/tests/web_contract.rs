//! Web contract tests (V7 red-on-stale-DTO + write-surface + gap semantics).
//!
//! - `status_response_matches_dto_shape`: live `/api/status` keys ==
//!   `EXPECTED_STATUS_KEYS` — adding a DTO field without updating the const
//!   (and the fixtures + TS mirror) fails red.
//! - `catalog_fixtures_cover_empty_loading_error`: all SEVEN catalog entries
//!   parse their empty/loading fixtures into the documented DTO shapes.
//! - `ts_export_matches_committed`: `cockpit/src/api-types.ts` ==
//!   `export_ts()` — regeneration required on DTO change.
//! - `write_surface_is_decide_only`: every `/api` route is GET-only → 405,
//!   EXCEPT `POST /api/ask/:id/decide` (A1 one-tap decision, P05). There are
//!   no other write endpoints.
//! - `history_gap_is_lost_not_silent` + `ask_queue_decide_is_live`: gap +
//!   A1 semantics over HTTP.
//! - Per-view shape tests (red-on-stale key sets): war-room buckets order +
//!   notifications, agent-stream normalization + unknown-version refusal,
//!   audit kanban + head fence, mcp detection + explain, providers
//!   probes + budgets + auth, runs slots + deferred note.
//! - `decide_round_trip_is_single_shot`: 200 → 409 → 404/400 typed.

use studio_core::approvals::{save_request, ApprovalRequest, ApprovalStatus};
use studio_core::state::StateStore;
use studio_web::catalog::{check_catalog_shape, CATALOG};
use studio_web::hub::Hub;
use studio_web::server::{
    build_router, status_keys, AppState, ProbeCache, EXPECTED_AGENT_STREAM_KEYS, EXPECTED_ASK_KEYS,
    EXPECTED_AUDIT_KEYS, EXPECTED_MCP_KEYS, EXPECTED_PROVIDERS_KEYS, EXPECTED_RUNS_KEYS,
    EXPECTED_STATUS_KEYS, EXPECTED_WAR_ROOM_KEYS,
};
use tower::ServiceExt;

fn seeded_db(tasks: &[(&str, &str, &str)]) -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("studio.db");
    let p = path.to_str().unwrap().to_owned();
    let store = StateStore::open(&p).unwrap();
    for (id, kind, status) in tasks {
        store.upsert_task(id, kind, status).unwrap();
    }
    (dir, p)
}

fn test_state(db_path: &str) -> AppState {
    let hub = Hub::new_with_cap(db_path.to_owned(), 2000, 512);
    assert!(hub.poll_once());
    AppState {
        hub,
        db_path: db_path.to_owned(),
        static_dir: std::path::PathBuf::from("/nonexistent-dist"),
        probe_cache: ProbeCache::default(),
        version: env!("CARGO_PKG_VERSION"),
    }
}

fn polled_hub(db_path: &str, cap: usize) -> Hub {
    let hub = Hub::new_with_cap(db_path.to_owned(), 2000, cap);
    assert!(hub.poll_once());
    hub
}

async fn body_json(
    router: axum::Router,
    req: axum::http::Request<axum::body::Body>,
) -> (axum::http::StatusCode, serde_json::Value) {
    let resp = router.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024 * 1024)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    (status, v)
}

fn get(uri: &str) -> axum::http::Request<axum::body::Body> {
    axum::http::Request::builder()
        .method("GET")
        .uri(uri)
        .body(axum::body::Body::empty())
        .unwrap()
}

#[tokio::test]
async fn status_response_matches_dto_shape() {
    let (_dir, p) = seeded_db(&[("t1", "coder", "building"), ("t2", "reviewer", "done")]);
    let router = build_router(test_state(&p));
    let (status, v) = body_json(router, get("/api/status")).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    // Red-on-stale-DTO: exact key set, sorted, against the const.
    let mut expected: Vec<String> = EXPECTED_STATUS_KEYS.iter().map(|s| s.to_string()).collect();
    expected.sort();
    assert_eq!(status_keys(&v), expected);
    // DB-only source: tasks came from the seeded DB, counts agree.
    assert_eq!(v["tasks"].as_array().unwrap().len(), 2);
    assert_eq!(v["counts"]["building"], 1);
    assert_eq!(v["counts"]["ready"], 1);
    assert_eq!(v["source"], "studio.db");
    assert!(v["staleness"]["label"]
        .as_str()
        .unwrap()
        .starts_with("fresh"));
}

/// Empty-collection keys per catalog entry (name → keys that must be `[]`
/// in the empty fixture).
fn empty_keys(name: &str) -> &'static [&'static str] {
    match name {
        "status" => &["tasks"],
        "war-room" => &["buckets", "notifications"],
        "agent-stream" => &["events"],
        "audit" => &["columns", "feedback"],
        "mcp" => &["agents"],
        "providers" => &["budgets"],
        "runs" => &["runs"],
        _ => &[],
    }
}

fn parse_empty_typed(name: &str, text: &str) {
    use studio_web::api::{
        AgentStreamDto, AuditDto, McpRegistryDto, ProvidersDto, RunsDto, StatusDto, WarRoomDto,
    };
    match name {
        "status" => {
            serde_json::from_str::<StatusDto>(text).unwrap();
        }
        "war-room" => {
            serde_json::from_str::<WarRoomDto>(text).unwrap();
        }
        "agent-stream" => {
            serde_json::from_str::<AgentStreamDto>(text).unwrap();
        }
        "audit" => {
            serde_json::from_str::<AuditDto>(text).unwrap();
        }
        "mcp" => {
            serde_json::from_str::<McpRegistryDto>(text).unwrap();
        }
        "providers" => {
            serde_json::from_str::<ProvidersDto>(text).unwrap();
        }
        "runs" => {
            serde_json::from_str::<RunsDto>(text).unwrap();
        }
        _ => panic!("unknown catalog entry {name}"),
    }
}

#[tokio::test]
async fn catalog_fixtures_cover_empty_loading_error() {
    check_catalog_shape().unwrap();
    assert_eq!(CATALOG.len(), 7);
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    for entry in CATALOG {
        // empty: parses as its DTO with empty collections.
        let empty_text = std::fs::read_to_string(dir.join(entry.fixtures[0])).unwrap();
        parse_empty_typed(entry.name, &empty_text);
        let v: serde_json::Value = serde_json::from_str(&empty_text).unwrap();
        for key in empty_keys(entry.name) {
            assert_eq!(
                v[key],
                serde_json::Value::Array(Vec::new()),
                "{} {key}",
                entry.name
            );
        }
        // loading: parses as its DTO (skeleton, zero age).
        let loading_text = std::fs::read_to_string(dir.join(entry.fixtures[1])).unwrap();
        parse_empty_typed(entry.name, &loading_text);
        let lv: serde_json::Value = serde_json::from_str(&loading_text).unwrap();
        assert_eq!(lv["db_age_ms"], 0, "{}", entry.name);
        // error: the legible 503 shape — must NOT parse as data.
        let err: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join(entry.fixtures[2])).unwrap())
                .unwrap();
        assert_eq!(err["error"], "projection_unavailable", "{}", entry.name);
        assert!(err["detail"].as_str().unwrap().contains("studio init"));
    }
    // The error shape is data-hostile for every DTO (spot-check two).
    let dir2 = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let err_text = std::fs::read_to_string(dir2.join("warroom_error.json")).unwrap();
    assert!(serde_json::from_str::<studio_web::api::WarRoomDto>(&err_text).is_err());
    let err_text = std::fs::read_to_string(dir2.join("runs_error.json")).unwrap();
    assert!(serde_json::from_str::<studio_web::api::RunsDto>(&err_text).is_err());
}

#[tokio::test]
async fn ts_export_matches_committed() {
    let committed =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../cockpit/src/api-types.ts");
    let expected = studio_web::api::export_ts();
    let actual = std::fs::read_to_string(&committed).expect(
        "cockpit/src/api-types.ts missing — run `cargo run -p studio-web --bin studio-web-gen-ts`",
    );
    assert_eq!(
        actual, expected,
        "api-types.ts is stale: regenerate with `cargo run -p studio-web --bin studio-web-gen-ts`"
    );
}

#[tokio::test]
async fn write_surface_is_decide_only() {
    // P05: the ONLY write endpoint is POST /api/ask/:id/decide. Every other
    // /api route is GET-only → 405 on write methods.
    let (_dir, p) = seeded_db(&[("t1", "coder", "building")]);
    let routes = [
        "/api/status",
        "/api/ask",
        "/api/war-room",
        "/api/agent-stream",
        "/api/audit",
        "/api/mcp",
        "/api/providers",
        "/api/runs",
        "/api/events",
        "/api/events/stream",
        "/api/health",
    ];
    for route in routes {
        for method in ["POST", "PUT", "DELETE", "PATCH"] {
            let router = build_router(test_state(&p));
            let req = axum::http::Request::builder()
                .method(method)
                .uri(route)
                .body(axum::body::Body::empty())
                .unwrap();
            let resp = router.oneshot(req).await.unwrap();
            assert_eq!(
                resp.status(),
                axum::http::StatusCode::METHOD_NOT_ALLOWED,
                "{method} {route} must be 405"
            );
        }
    }
    // ...and the decide path is POST-only (GET/PUT/DELETE/PATCH → 405).
    for method in ["GET", "PUT", "DELETE", "PATCH"] {
        let router = build_router(test_state(&p));
        let req = axum::http::Request::builder()
            .method(method)
            .uri("/api/ask/apr-x/decide")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            axum::http::StatusCode::METHOD_NOT_ALLOWED,
            "{method} /api/ask/:id/decide must be 405"
        );
    }
}

#[tokio::test]
async fn history_gap_is_lost_not_silent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("studio.db");
    let p = path.to_str().unwrap().to_owned();
    let store = StateStore::open(&p).unwrap();
    // NOTE: `change_log` triggers cover `tasks` only — burst task rows.
    for i in 0..20 {
        store
            .upsert_task(&format!("t{i}"), "coder", "building")
            .unwrap();
    }
    // Tiny ring: the first events are evicted by the burst.
    let hub = Hub::new_with_cap(p.clone(), 2000, 4);
    assert!(hub.poll_once());
    let state = AppState {
        hub,
        db_path: p,
        static_dir: std::path::PathBuf::from("/nonexistent-dist"),
        probe_cache: ProbeCache::default(),
        version: env!("CARGO_PKG_VERSION"),
    };
    let (status, v) = body_json(build_router(state.clone()), get("/api/events?since=0")).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(v["gap"], "Lost");
    assert_eq!(v["events"].as_array().unwrap().len(), 0);
    // Re-sync from the floor pages contiguously.
    let floor = v["floor_seq"].as_i64().unwrap();
    let (status, v2) = body_json(
        build_router(state),
        get(&format!("/api/events?since={floor}")),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(v2["gap"], "None");
    assert!(!v2["events"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn ask_queue_decide_is_live_for_pending() {
    let (_dir, p) = seeded_db(&[("t1", "coder", "building")]);
    let store = StateStore::open(&p).unwrap();
    for (id, status) in [
        ("apr-pending", ApprovalStatus::Pending),
        ("apr-done", ApprovalStatus::Denied),
    ] {
        let req = ApprovalRequest {
            id: id.into(),
            task_id: "t1".into(),
            tool: "write".into(),
            status,
            reason: None,
            created_ms: 1,
            decided_ms: None,
        };
        save_request(&store, &req).unwrap();
    }
    let (status, v) = body_json(build_router(test_state(&p)), get("/api/ask")).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    // Red-on-stale key set for the ask feed.
    let mut expected: Vec<String> = EXPECTED_ASK_KEYS.iter().map(|s| s.to_string()).collect();
    expected.sort();
    assert_eq!(status_keys(&v), expected);
    let items = v["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    // P05 contract: pending rows expose the live path; decided stay disabled.
    let pend = items.iter().find(|i| i["id"] == "apr-pending").unwrap();
    assert_eq!(pend["decision_enabled"], true);
    let done = items.iter().find(|i| i["id"] == "apr-done").unwrap();
    assert_eq!(done["decision_enabled"], false);
    assert!(v["decision_contract"].as_str().unwrap().contains("one-tap"));
}

fn post_json(uri: &str, body: serde_json::Value) -> axum::http::Request<axum::body::Body> {
    axum::http::Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(axum::body::Body::from(body.to_string()))
        .unwrap()
}

#[tokio::test]
async fn decide_round_trip_is_single_shot() {
    let (_dir, p) = seeded_db(&[("t1", "coder", "building")]);
    let store = StateStore::open(&p).unwrap();
    let req = ApprovalRequest {
        id: "apr-1".into(),
        task_id: "t1".into(),
        tool: "write".into(),
        status: ApprovalStatus::Pending,
        reason: None,
        created_ms: 1,
        decided_ms: None,
    };
    save_request(&store, &req).unwrap();
    // 200: approve lands in the durable ledger.
    let (status, v) = body_json(
        build_router(test_state(&p)),
        post_json(
            "/api/ask/apr-1/decide",
            serde_json::json!({"approved": true, "reason": "scoped write ok"}),
        ),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(v["id"], "apr-1");
    assert_eq!(v["status"], "approved");
    assert!(v["decided_ms"].as_u64().is_some());
    // The ledger row moved: GET shows approved + disabled.
    let (_, feed) = body_json(build_router(test_state(&p)), get("/api/ask")).await;
    let row = feed["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == "apr-1")
        .unwrap();
    assert_eq!(row["status"], "approved");
    assert_eq!(row["decision_enabled"], false);
    // 409: second decision is a typed refusal, first outcome kept.
    let (status, v) = body_json(
        build_router(test_state(&p)),
        post_json(
            "/api/ask/apr-1/decide",
            serde_json::json!({"approved": false, "reason": "reconsider"}),
        ),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::CONFLICT);
    assert_eq!(v["error"], "already_decided");
    // 404: unknown request.
    let (status, v) = body_json(
        build_router(test_state(&p)),
        post_json(
            "/api/ask/apr-nope/decide",
            serde_json::json!({"approved": true, "reason": "x"}),
        ),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::NOT_FOUND);
    assert_eq!(v["error"], "unknown_request");
    // 400: empty reason.
    let (status, v) = body_json(
        build_router(test_state(&p)),
        post_json(
            "/api/ask/apr-1/decide",
            serde_json::json!({"approved": true, "reason": "  "}),
        ),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    assert_eq!(v["error"], "bad_reason");
}

fn check_keys(v: &serde_json::Value, expected: &[&str]) {
    let mut want: Vec<String> = expected.iter().map(|s| s.to_string()).collect();
    want.sort();
    assert_eq!(status_keys(v), want);
}

#[tokio::test]
async fn war_room_buckets_order_and_notifications() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("studio.db");
    let p = path.to_str().unwrap().to_owned();
    let store = StateStore::open(&p).unwrap();
    store.upsert_task("t-need", "coder", "building").unwrap();
    store.upsert_task("t-fail", "coder", "failed").unwrap();
    store.upsert_task("t-run", "coder", "building").unwrap();
    store
        .upsert_task("t-attn", "reviewer", "in-review")
        .unwrap();
    store.upsert_task("t-done", "coder", "done").unwrap();
    let req = ApprovalRequest {
        id: "apr-n".into(),
        task_id: "t-need".into(),
        tool: "write".into(),
        status: ApprovalStatus::Pending,
        reason: None,
        created_ms: 1,
        decided_ms: None,
    };
    save_request(&store, &req).unwrap();
    let (status, v) = body_json(build_router(test_state(&p)), get("/api/war-room")).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    check_keys(&v, EXPECTED_WAR_ROOM_KEYS);
    // Deterministic attention-first order: needs_input(0) > failed(1) >
    // running(2) > attention(3) > done(4).
    let names: Vec<&str> = v["buckets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["needs_input", "failed", "running", "attention", "done"]
    );
    let need = &v["buckets"].as_array().unwrap()[0];
    assert_eq!(need["tasks"][0]["id"], "t-need");
    // Permission notification for the pending approval.
    assert_eq!(v["notifications"].as_array().unwrap().len(), 1);
    assert_eq!(v["notifications"][0]["kind"], "permission");
    assert_eq!(v["notifications"][0]["task_id"], "t-need");
    assert!(v["staleness"]["label"]
        .as_str()
        .unwrap()
        .starts_with("fresh"));
    assert!(v["staleness"]["label"].as_str().unwrap().contains("seq 5"));
}

#[tokio::test]
async fn agent_stream_normalizes_and_refuses_unknown_versions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("studio.db");
    let p = path.to_str().unwrap().to_owned();
    let store = StateStore::open(&p).unwrap();
    store.upsert_task("t1", "coder", "building").unwrap();
    // Agent-origin row: the P05 trigger fans `events` inserts into the ring.
    store
        .append_event("message", r#"{"text":"hello"}"#)
        .unwrap();
    // Unknown provider version, smuggled via a hook payload row.
    drop(store);
    let store2 = StateStore::open(&p).unwrap();
    store2
        .append_event("hook", r#"{"provider":"rogue-hook","protocol_version":99}"#)
        .unwrap();
    drop(store2);
    let (status, v) = body_json(build_router(test_state(&p)), get("/api/agent-stream")).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    check_keys(&v, EXPECTED_AGENT_STREAM_KEYS);
    assert_eq!(v["provider_protocol_version"], 1);
    // studio-cdc provider is always declared compatible.
    let cdc = v["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["name"] == "studio-cdc")
        .unwrap();
    assert_eq!(cdc["compatible"], true);
    // The rogue version is refused (surfaced, never executed)...
    let refused: Vec<&serde_json::Value> = v["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["refused"] == true)
        .collect();
    assert_eq!(refused.len(), 1);
    assert_eq!(refused[0]["provider"], "rogue-hook");
    assert_eq!(refused[0]["kind"], "unknown");
    // ...and the rogue provider is listed incompatible.
    let rogue = v["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["name"] == "rogue-hook")
        .unwrap();
    assert_eq!(rogue["compatible"], false);
    // Task CDC rows normalize to the status kind.
    assert!(v["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["kind"] == "status"));
}

#[tokio::test]
async fn health_reports_missing_daemon_legibly() {
    let (_dir, p) = seeded_db(&[]);
    // Point RUN_DIR at an empty temp dir: no PID files → Missing.
    // (Only this test's /api/health call depends on the env; all other
    // tests assert daemon-agnostic fields, so a parallel interleaving can
    // only flip their `daemon_reachable` bit, which none of them pin.)
    let run = tempfile::tempdir().unwrap();
    // probe_in is env-free; exercise the HTTP shape with the real env by
    // scoping: server reads STUDIO_RUN_DIR, so set it around the call.
    let prev = std::env::var("STUDIO_RUN_DIR").ok();
    unsafe {
        std::env::set_var("STUDIO_RUN_DIR", run.path());
    }
    let (status, v) = body_json(build_router(test_state(&p)), get("/api/health")).await;
    unsafe {
        if let Some(pv) = prev {
            std::env::set_var("STUDIO_RUN_DIR", pv);
        } else {
            std::env::remove_var("STUDIO_RUN_DIR");
        }
    }
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(v["db_ok"], true);
    assert_eq!(v["daemon"]["reachable"], false);
    assert!(v["daemon"]["detail"]
        .as_str()
        .unwrap()
        .contains("studio daemon"));
}

#[tokio::test]
async fn missing_db_fails_closed_legibly() {
    let state = AppState {
        hub: Hub::new_with_cap("/nonexistent-dir-xyz/studio.db".into(), 2000, 512),
        db_path: "/nonexistent-dir-xyz/studio.db".into(),
        static_dir: std::path::PathBuf::from("/nonexistent-dist"),
        probe_cache: ProbeCache::default(),
        version: env!("CARGO_PKG_VERSION"),
    };
    assert!(!state.hub.poll_once());
    let (status, v) = body_json(build_router(state), get("/api/status")).await;
    assert_eq!(status, axum::http::StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(v["error"], "projection_unavailable");
}

#[tokio::test]
async fn unknown_api_route_is_typed_json_never_spa() {
    // F1: /api/* typos must stay typed JSON even when the SPA dist exists
    // (the nested /api scope owns its 404; ServeDir can no longer shadow it).
    let (_dir, p) = seeded_db(&[("t1", "coder", "building")]);
    let dist = tempfile::tempdir().unwrap();
    std::fs::write(dist.path().join("index.html"), "<h1>spa</h1>").unwrap();
    let state = AppState {
        hub: polled_hub(&p, 512),
        db_path: p,
        static_dir: dist.path().to_owned(),
        probe_cache: ProbeCache::default(),
        version: env!("CARGO_PKG_VERSION"),
    };
    let router = build_router(state);
    let resp = router.oneshot(get("/api/nope-xyz")).await.unwrap();
    assert_eq!(resp.status(), axum::http::StatusCode::NOT_FOUND);
    let ctype = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    assert!(ctype.contains("application/json"), "ctype={ctype}");
    let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["error"], "unknown_api_route");
    // ...while the SPA fallback still serves the shell for app routes.
    let state2 = AppState {
        hub: polled_hub(&seeded_db(&[("t1", "coder", "building")]).1, 512),
        db_path: "unused".into(),
        static_dir: dist.path().to_owned(),
        probe_cache: ProbeCache::default(),
        version: env!("CARGO_PKG_VERSION"),
    };
    let resp = build_router(state2).oneshot(get("/")).await.unwrap();
    assert_eq!(resp.status(), axum::http::StatusCode::OK);
    // And without a dist, /api/* typos are still the JSON 404.
    let (_dir3, p3) = seeded_db(&[("t1", "coder", "building")]);
    let (status, v) = body_json(build_router(test_state(&p3)), get("/api/nope-xyz")).await;
    assert_eq!(status, axum::http::StatusCode::NOT_FOUND);
    assert_eq!(v["error"], "unknown_api_route");
}

#[tokio::test]
async fn negative_cursor_is_not_spurious_lost_over_http() {
    // F2 over HTTP: since<0 clamps to 0; floor is 0 here, so None.
    let (_dir, p) = seeded_db(&[("t1", "coder", "building")]);
    let (status, v) = body_json(build_router(test_state(&p)), get("/api/events?since=-5")).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(v["gap"], "None");
    assert_eq!(v["events"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn status_since_below_floor_is_lost() {
    // F4 over HTTP: StalenessState::Lost is reachable via ?since=.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("studio.db");
    let p = path.to_str().unwrap().to_owned();
    let store = StateStore::open(&p).unwrap();
    for i in 0..10 {
        store
            .upsert_task(&format!("t{i}"), "coder", "building")
            .unwrap();
    }
    let state = AppState {
        hub: polled_hub(&p, 4),
        db_path: p,
        static_dir: std::path::PathBuf::from("/nonexistent-dist"),
        probe_cache: ProbeCache::default(),
        version: env!("CARGO_PKG_VERSION"),
    };
    let (status, v) = body_json(build_router(state.clone()), get("/api/status?since=0")).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(v["staleness"]["state"], "Lost");
    assert!(
        v["staleness"]["label"]
            .as_str()
            .unwrap()
            .starts_with("lost · "),
        "{}",
        v["staleness"]["label"]
    );
    let (status, v) = body_json(build_router(state), get("/api/status")).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(v["staleness"]["state"], "Fresh");
}

#[tokio::test]
async fn audit_kanban_carries_evidence_and_head_fence() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("studio.db");
    let p = path.to_str().unwrap().to_owned();
    let store = StateStore::open(&p).unwrap();
    store.upsert_task("t-b", "coder", "building").unwrap();
    store.upsert_task("t-r", "reviewer", "in-review").unwrap();
    store
        .append_receipt("rc-1", "t-r", "review", "diff ok; freeze fh-abc-001")
        .unwrap();
    // Two fences on one target: the later row is the current head.
    insert_freeze(&p, "fh-abc-001", "refs/heads/x", "quick", 1);
    insert_freeze(&p, "fh-abc-002", "refs/heads/x", "full", 2);
    let (status, v) = body_json(build_router(test_state(&p)), get("/api/audit")).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    check_keys(&v, EXPECTED_AUDIT_KEYS);
    // Kanban: review card carries its receipt evidence.
    let review = v["columns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "in-review")
        .unwrap();
    let card = review["cards"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "t-r")
        .unwrap();
    assert!(card["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e == "rc-1"));
    // Fence: old head is not current, new head is; evidence links task.
    let fb: Vec<&serde_json::Value> = v["feedback"].as_array().unwrap().iter().collect();
    assert_eq!(fb.len(), 2);
    let old = fb.iter().find(|f| f["head_sha"] == "fh-abc-001").unwrap();
    let head = fb.iter().find(|f| f["head_sha"] == "fh-abc-002").unwrap();
    assert_eq!(old["current_head"], false);
    assert_eq!(head["current_head"], true);
    assert_eq!(old["task_id"], "t-r");
    assert_eq!(head["task_id"], "");
}

fn insert_freeze(db_path: &str, hash: &str, target: &str, tier: &str, rev: i64) {
    // Test-only direct insert: production writes go through verify/freeze.
    let conn = rusqlite::Connection::open(db_path).unwrap();
    conn.execute(
        "INSERT INTO frozen_candidates(freeze_hash,lineage_hash,revision,target_ref,targets_json,contents_json,tier,created_ms) VALUES(?,?,?,?,?,?,?,?)",
        rusqlite::params![hash, "lin", rev, target, "[]", "{}", tier, 0],
    )
    .unwrap();
}

#[tokio::test]
async fn mcp_registry_detects_and_explains() {
    let (_dir, p) = seeded_db(&[("t1", "coder", "building")]);
    let (status, v) = body_json(build_router(test_state(&p)), get("/api/mcp")).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    check_keys(&v, EXPECTED_MCP_KEYS);
    let agents = v["agents"].as_array().unwrap();
    // Bundled manifest entries + exactly one remote status record.
    assert!(agents
        .iter()
        .any(|a| a["name"] == "opencode" && a["manifest_source"] == "Bundled"));
    let remote: Vec<&serde_json::Value> = agents
        .iter()
        .filter(|a| a["manifest_source"] == "Remote")
        .collect();
    assert_eq!(remote.len(), 1);
    for a in agents {
        // Every record explains itself and carries a valid flag.
        assert!(!a["explain"].as_str().unwrap().is_empty(), "{}", a["name"]);
        assert!(["working", "blocked", "idle"].contains(&a["state"].as_str().unwrap()));
        if a["manifest_source"] == "Remote" {
            // Remote is listed-never-fetched: always undetected/idle.
            assert_eq!(a["detected"], false);
            assert_eq!(a["state"], "idle");
        } else {
            assert_eq!(a["detected"], a["state"] != "idle", "{}", a["name"]);
        }
    }
}

#[tokio::test]
async fn providers_serve_probe_cache_budgets_and_auth() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("studio.db");
    let p = path.to_str().unwrap().to_owned();
    let store = StateStore::open(&p).unwrap();
    store.record_tokens("sess-a", 100, false).unwrap();
    store.record_tokens("sess-a", 40, true).unwrap();
    store.record_tokens("sess-b", 7, false).unwrap();
    // Probe cache: first read re-probes (fresh=false), the immediate
    // second read serves the TTL cache (fresh=true, same probe instant).
    let state = test_state(&p);
    let (status, v) = body_json(build_router(state.clone()), get("/api/providers")).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    check_keys(&v, EXPECTED_PROVIDERS_KEYS);
    assert_eq!(v["probes"].as_array().unwrap().len(), 2);
    for pr in v["probes"].as_array().unwrap() {
        assert_eq!(pr["fresh"], false);
        assert_eq!(pr["ttl_ms"], 30_000);
        assert!(!pr["detail"].as_str().unwrap().is_empty());
    }
    let first_at: Vec<i64> = v["probes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|pr| pr["last_probe_ms"].as_i64().unwrap())
        .collect();
    let (status2, v2) = body_json(build_router(state), get("/api/providers")).await;
    assert_eq!(status2, axum::http::StatusCode::OK);
    for (pr, at) in v2["probes"].as_array().unwrap().iter().zip(first_at) {
        assert_eq!(pr["fresh"], true);
        assert_eq!(pr["last_probe_ms"].as_i64().unwrap(), at);
    }
    // Budgets: billed totals with cache hits tracked separately.
    let a = v["budgets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["session"] == "sess-a")
        .unwrap();
    assert_eq!(a["total"], 100);
    assert_eq!(a["cache_hits"], 1);
    assert_eq!(v["lifetime_total"], 107);
    // Auth probes: real presence checks + explicit unknown fallbacks.
    assert!(v["auth"]
        .as_array()
        .unwrap()
        .iter()
        .any(|x| x["method"] == "credential-file"));
    let unknowns: Vec<&serde_json::Value> = v["auth"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|x| x["status"] == "unknown")
        .collect();
    assert_eq!(unknowns.len(), 2);
    assert!(v["staleness"]["label"]
        .as_str()
        .unwrap()
        .starts_with("fresh"));
}

#[tokio::test]
async fn runs_compare_slots_and_deferred_note() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("studio.db");
    let p = path.to_str().unwrap().to_owned();
    let store = StateStore::open(&p).unwrap();
    store.upsert_task("t1", "coder", "building").unwrap();
    store
        .begin_worktree("/tmp/run-alpha", "repo", "run-alpha")
        .unwrap();
    store
        .begin_worktree("/tmp/run-beta", "repo", "run-beta")
        .unwrap();
    store
        .append_receipt("rc-1", "run-alpha-t0", "diff", "evidence for run-alpha")
        .unwrap();
    let (status, v) = body_json(build_router(test_state(&p)), get("/api/runs")).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    check_keys(&v, EXPECTED_RUNS_KEYS);
    let runs = v["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 2);
    let alpha = runs.iter().find(|r| r["slug"] == "run-alpha").unwrap();
    assert_eq!(alpha["receipts"], 1);
    let beta = runs.iter().find(|r| r["slug"] == "run-beta").unwrap();
    assert_eq!(beta["receipts"], 0);
    // Deferred scope is stated, not faked.
    assert!(v["deferred"].as_str().unwrap().contains("deferred"));
}
