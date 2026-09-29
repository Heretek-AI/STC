//! `studio-web` — serve the cockpit SPA + `/api` projection reads.
//!
//! Env knobs (all optional, all documented):
//! - `STUDIO_HUB_POLL_MS` (default 100): CDC poll cadence.
//! - `STUDIO_HUB_RING_CAP` (default 512): event ring retention.
//! - `STUDIO_STALE_AFTER_MS` (default 2000): time-stale budget.
//! - `STUDIO_RUN_DIR` (default `./.studio-run`): daemon PID/version files.
//! - `STUDIO_MS1_FROZEN_NOW_MS`: frozen clock for deterministic gate runs.

use clap::Parser;
use studio_web::{
    hub::Hub,
    server::{build_router, AppState},
    staleness::stale_after_ms,
};

#[derive(Parser)]
#[command(
    name = "studio-web",
    version,
    about = "STC v2 cockpit shell (read-only projection server)"
)]
struct Args {
    /// Path to the v2 database (read-only open; works with no daemon).
    #[arg(long, default_value = "studio.db")]
    db: String,
    /// Port to listen on.
    #[arg(long, default_value_t = 3769)]
    port: u16,
    /// Directory of the built cockpit SPA (`cockpit/dist`).
    #[arg(long, default_value = "cockpit/dist")]
    static_dir: String,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let hub = Hub::new(args.db.clone(), stale_after_ms());
    // Boot poll: serve fresh data on the first request instead of 503.
    // A missing DB here is NOT fatal — endpoints fail closed legibly.
    if hub.poll_once() {
        eprintln!("[studio-web] boot poll ok: {}", args.db);
    } else {
        eprintln!(
            "[studio-web] boot poll missed (db {:?} unreadable?) — serving legible 503s until it appears; daemon probe on /api/health",
            args.db
        );
    }
    let _poller = hub.spawn_poller();
    let state = AppState {
        hub,
        db_path: args.db,
        static_dir: std::path::PathBuf::from(args.static_dir),
        version: env!("CARGO_PKG_VERSION"),
    };
    let app = build_router(state);
    let addr = format!("127.0.0.1:{}", args.port);
    eprintln!("[studio-web] listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await.expect("bind");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .expect("serve");
}
