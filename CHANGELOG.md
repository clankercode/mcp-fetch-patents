# Changelog

## [0.1.0] - 2026-04-10

### Added
- Rust MCP server with full feature parity to Python
- 18 MCP tools: fetch patents, natural-language search, structured search, sessions, citation chains, family search, classification search, query suggestions, profile management, quick search, session delete
- SQLite-backed patent cache
- Browser automation via chromiumoxide (CDP protocol)
- SerpAPI, USPTO, EPO OPS search backends
- Deterministic NL query planner
- Heuristic hit ranking with multi-query bonus
- Session persistence with atomic writes
- Browser profile management with file-based locking
- MCP spec compliance (isError, ping, notifications)

### Changed
- Pre-compiled regex statics for planner and ranking performance
- Concurrent citation chain expansion and SerpAPI queries
- Async file I/O for session manager (spawn_blocking)
- Config-driven timeouts for all search backends

### Security
- Session ID path traversal validation (Python and Rust)
- SSRF prevention on EPO URL construction
- Output path validation for session export
