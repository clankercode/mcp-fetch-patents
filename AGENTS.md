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
- `list_cached_patents` — list all patents in local cache
- `get_patent_metadata` — return cached metadata for patents

**Search tools (16):**
- `patent_search_natural` — NL search with planner + enrichment
- `patent_search_structured` — structured field search
- `patent_suggest_queries` — query suggestions from description
- `patent_citation_chain` — forward/backward citation chains
- `patent_classification_search` — CPC/IPC classification search
- `patent_family_search` — family members
- `patent_session_create` / `list` / `load` / `export` — session management
- `patent_session_note` — add timestamped notes to sessions
- `patent_session_annotate` — annotate sessions
- `patent_search_profile_login_start` — launch browser for profile login

## Python Search Server

Separate MCP server from the fetch server:
```bash
python -m patent_mcp.search    # search MCP server
python -m patent_mcp           # fetch MCP server
```

## Environment Variables

Key env vars (also configurable via `.patentrc`):
- `PATENT_SERPAPI_KEY` — SerpAPI key for search
- `PATENT_EPO_KEY` — EPO OPS credentials (`client_id:client_secret`)
- `PATENT_SEARCH_BROWSER_HEADLESS` — browser headless mode (default: true)
- `PATENT_SEARCH_BROWSER_PROFILES_DIR` — browser profile directory
- `PATENT_SEARCH_BACKEND_DEFAULT` — default search backend (auto/browser/serpapi)
