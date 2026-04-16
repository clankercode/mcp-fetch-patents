//! patent-mcp-server — Rust production implementation.
//!
//! Mirrors patent_mcp Python reference implementation behavior exactly.
//! Cross-validated via shared test fixtures and deterministic HTTP mock harness.

pub mod cache;
pub mod config;
pub mod converters;
pub mod cooldown;
pub mod fetchers;
pub mod id_canon;
pub mod journal;
pub mod planner;
pub mod prefetch;
pub mod ranking;
pub mod rate_limit;
pub mod search;
pub mod server;

pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

pub fn elapsed_ms(start: std::time::Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}
