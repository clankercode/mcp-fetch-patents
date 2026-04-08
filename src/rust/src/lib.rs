//! patent-mcp-server — Rust production implementation.
//!
//! Mirrors patent_mcp Python reference implementation behavior exactly.
//! Cross-validated via shared test fixtures and deterministic HTTP mock harness.

pub mod cache;
pub mod config;
pub mod converters;
pub mod fetchers;
pub mod id_canon;
pub mod server;
