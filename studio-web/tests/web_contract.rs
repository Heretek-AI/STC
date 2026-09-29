//! Web contract tests (V7 red-on-stale-DTO + read-only + gap semantics).
//!
//! - `status_response_matches_dto_shape`: live `/api/status` keys ==
//!   `EXPECTED_STATUS_KEYS` — adding a DTO field without updating the const
//!   (and the fixtures + TS mirror) fails red.
//! - `catalog_fixtures_cover_empty_loading_error`: the three V7 fixtures
//!   parse into their documented shapes.
//! - `ts_export_matches_committed`: `cockpit/src/api-types.ts` ==
//!   `export_ts()` — regeneration required on DTO change.
//! - `browser_is_read_only`: POST/PUT/DELETE/PATCH on every `/api` route
//!   → 405. There are no write endpoints.
//! - `history_gap_is_lost_not_silent` + `ask_queue_is_read_only_and_disabled`:
//!   gap + A1 semantics over HTTP.

use studio_core::approvals::{save_request, ApprovalRequest, ApprovalStatus};
use studio_core::state::StateStore;
use studio_web::catalog::{check_catalog_shape, CATALOG};
use studio_web::hub::Hub;
use studio_web::server::{build_router, status_keys, AppState, EXPECTED_STATUS_KEYS};
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

#[tokio::test]
async fn catalog_fixtures_cover_empty_loading_error() {
    check_catalog_shape().unwrap();
    assert_eq!(CATALOG.len(), 1);
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    // empty: parses as StatusDto with zero tasks.
    let empty: studio_web::api::StatusDto =
        serde_json::from_str(&std::fs::read_to_string(dir.join(CATALOG[0].fixtures[0])).unwrap())
            .unwrap();
    assert!(empty.tasks.is_empty());
    // loading: parses as StatusDto (skeleton, zero age).
    let loading: studio_web::api::StatusDto =
        serde_json::from_str(&std::fs::read_to_string(dir.join(CATALOG[0].fixtures[1])).unwrap())
            .unwrap();
    assert_eq!(loading.db_age_ms, 0);
    // error: the legible 503 shape — must NOT parse as data.
    let err: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join(CATALOG[0].fixtures[2])).unwrap())
            .unwrap();
    assert_eq!(err["error"], "projection_unavailable");
    assert!(err["detail"].as_str().unwrap().contains("studio init"));
    assert!(serde_json::from_value::<studio_web::api::StatusDto>(err).is_err());
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
async fn browser_is_read_only() {
    let (_dir, p) = seeded_db(&[("t1", "coder", "building")]);
    let routes = [
        "/api/status",
        "/api/ask",
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
async fn ask_queue_is_read_only_and_disabled() {
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
    let (status, v) = body_json(build_router(test_state(&p)), get("/api/ask")).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let items = v["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], "apr-1");
    // P04 contract: visibly disabled, never fake-enabled.
    assert_eq!(items[0]["decision_enabled"], false);
    assert!(v["decision_contract"]
        .as_str()
        .unwrap()
        .contains("read-only"));
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
