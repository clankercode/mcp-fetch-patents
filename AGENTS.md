# AGENTS.md — mcp-fetch-patents

## Project Overview

Dual-implementation MCP server for patent fetching, caching, and search.
- **Python** (`src/python/`) — reference implementation
- **Rust** (`src/rust/`) — production implementation, feature parity with Python

Both expose MCP tools over stdio JSON-RPC.

## Build & Test Commands

**Must use `CC=gcc` for all cargo commands** (default `cc` is a wrapper that breaks aws-lc-sys).

```bash
just test-rust              # Run all Rust tests (205 tests)
just build-rust             # Debug build
just build-rust-release     # Release build
just check-rust             # Quick type-check
just lint-rust              # Clippy with -D warnings
just test                   # Python fast tests
just ci                     # Python fast + Rust tests
just serve-rust             # Run Rust MCP server
```

For direct cargo commands:
```bash
CC=gcc cargo test --manifest-path src/rust/Cargo.toml
CC=gcc cargo build --manifest-path src/rust/Cargo.toml
```

## Rust Module Layout

```
src/rust/src/
  lib.rs              — crate root, pub mod declarations
  main.rs             — binary entrypoint
  config/mod.rs       — PatentConfig (env vars, .patentrc)
  id_canon/           — patent ID canonicalization
  cache/              — SQLite-backed patent cache
  fetchers/           — HTTP fetch backends (USPTO, EPO, Google)
    http/             — HTTP fetcher implementations
    browser.rs        — Playwright/chromiumoxide browser fetcher
    web_search/       — web search utilities
    mod.rs
  converters/         — format conversion (XML, JSON, etc.)
  journal.rs          — activity journal
  planner.rs          — NL query planner (deterministic, rule-based)
  ranking.rs          — PatentHit scoring/reranking
  search/             — search MCP tools
    mod.rs            — module root, SearchBackends struct
    session_manager.rs — session persistence (atomic JSON, path traversal guard)
    searchers.rs      — SerpAPI, USPTO, EPO OPS backends (JSON + XML)
    profile_manager.rs — browser profile dirs with file-based locking
    browser_search.rs  — Google Patents via chromiumoxide
  server/mod.rs       — MCP server: tool descriptors + handlers (fetch + search)
```

## Code Conventions

- Error handling: `anyhow::Result` everywhere
- Dynamic metadata: `serde_json::Value`
- No comments unless explicitly requested
- Regex in production code: use `OnceLock<Regex>` statics, never `Regex::new(...).unwrap()` inline
- Tests live in the same file as the code they test (`#[cfg(test)] mod tests`)
- Async runtime: tokio
- HTTP client: reqwest
- Browser automation: chromiumoxide (CDP protocol)
- Follow existing patterns in the file you're editing

## MCP Tools (Rust)

**Fetch tools:**
- `fetch_patents` — fetch and cache patents by ID
- `patent_cache_status` — cache info for patent IDs
- `patent_convert` — format conversion

**Search tools (13):**
- `patent_search_natural` — NL search with planner + enrichment
- `patent_search_structured` — structured field search
- `patent_suggest_queries` — query suggestions from description
- `patent_search_session_create` / `list` / `load` / `export` — session management
- `patent_search_session_annotate` — add notes to sessions
- `patent_search_merge_sessions` — merge multiple sessions
- `patent_get_citation_chains` — forward/backward citation chains
- `patent_get_patent_family` — family members
- `patent_get_classification_tree` — CPC/IPC tree
- `patent_search_profile_login` — launch browser for profile login

## Python Search Server

Separate MCP server from the fetch server:
```bash
python -m patent_mcp.search    # search MCP server
python -m patent_mcp           # fetch MCP server
```

## Environment Variables

Key env vars (also configurable via `.patentrc`):
- `PATENT_SERPAPI_KEY` — SerpAPI key for search
- `PATENT_USPTO_API_KEY` — USPTO API key
- `PATENT_EPO_API_KEY` / `PATENT_EPO_SECRET` — EPO OPS credentials
- `PATENT_BROWSER_HEADLESS` — browser headless mode (default: true)
- `PATENT_BROWSER_PROFILES_DIR` — browser profile directory
- `PATENT_SEARCH_BACKEND` — default search backend (auto/browser/serpapi)
