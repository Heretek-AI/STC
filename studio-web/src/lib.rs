//! studio-web: axum+tower cockpit shell over the real `studio.db` projection.
//!
//! Server owns the projection; the browser reads only (GET `/api/*`, SSE
//! history+live). Fresh/stale/lost staleness taxonomy; missing daemon fails
//! closed legibly; zero mock data.

pub mod api;
pub mod catalog;
pub mod daemon;
pub mod hub;
pub mod server;
pub mod staleness;
