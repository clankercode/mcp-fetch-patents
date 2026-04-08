# PLAN — 05-mcp-protocol: MCP Server + Tool Definitions

*Depends on: all other nodes. This is the final assembly.*
*Tests use end-to-end MCP message → tool call → result flow (all I/O mocked)*

---

## Python Implementation

### T01 — Server starts without crash
- **RED**: `test_server_starts_and_registers_tools` — create `FastMCP` instance; call `mcp.get_tools()`; verify `fetch_patents`, `list_cached_patents`, `get_patent_metadata` all registered
- **GREEN**: implement `patent_mcp/server.py` with FastMCP instance + tool registrations

### T02 — fetch_patents input schema validation
- **RED**: `test_schema_rejects_empty_list` — call `fetch_patents(patent_ids=[])` → validation error (minItems=1)
- **RED**: `test_schema_accepts_valid_batch` — `patent_ids=["US7654321", "EP1234567"]` → no validation error
- **RED**: `test_schema_accepts_all_optional_fields` — all optional fields provided → no error
- **GREEN**: Pydantic model for `FetchPatentsInput`; FastMCP uses it for schema

### T03 — fetch_patents result ordering matches input
- **RED**: `test_result_order_matches_input` — input `["EP1234567", "US7654321"]`; mock orchestrator returns in any order; assert result `[0].patent_id == "EP1234567"` and `[1].patent_id == "US7654321"`
- **GREEN**: sort results by input index after gathering

### T04 — Error in one patent doesn't affect others
- **RED**: `test_batch_error_isolation` — `["US7654321", "INVALID999", "EP1234567"]`; middle one errors; other two succeed; `results[1].status == "error"`, others succeed
- **GREEN**: per-patent error handling in batch loop

### T05 — Cache hit returns "cached" status
- **RED**: `test_cache_hit_status` — mock cache returns a `CacheResult`; `result.status == "cached"`, `result.fetch_duration_ms` is small
- **GREEN**: set status="cached" when result from cache

### T06 — postprocess_query no-op in v1
- **RED**: `test_postprocess_query_accepted` — `fetch_patents(patent_ids=["US7654321"], postprocess_query="summarize claims")` → no exception
- **RED**: `test_postprocess_query_stored_in_metadata` — `result["metadata"]["postprocess_query"] == "summarize claims"` and `result["metadata"]["postprocess_query_note"] == "postprocess_query not yet implemented in v1; stored for future use"`
- **RED**: `test_postprocess_query_logged_to_stderr` — warning captured on stderr (not stdout)
- **GREEN**: accept parameter; store value + note in per-result metadata; log warning to stderr; do not execute

### T07 — Token budget truncation
- **RED**: `test_long_abstract_truncated` — mock orchestrator returns patent with 2000-char abstract; `estimate_tokens(result_json) > MAX_RESPONSE_TOKENS`; verify abstract truncated to 500 chars in response
- **RED**: `test_file_paths_never_truncated` — even when truncation occurs, file paths are preserved
- **GREEN**: implement `truncate_if_needed()` called before returning MCP result

### T08 — list_cached_patents tool
- **RED**: `test_list_cached_returns_all` — mock `cache.list_all()` returns 2 entries; tool returns array of 2
- **GREEN**: implement `list_cached_patents()` tool

### T09 — get_patent_metadata tool (no network)
- **RED**: `test_get_patent_metadata_cache_only` — patent in global DB; `get_patent_metadata(patent_ids=["US7654321"])` → returns metadata without network call
- **RED**: `test_get_patent_metadata_not_found` — patent not in DB; result is null for that ID
- **GREEN**: implement `get_patent_metadata()` tool

### T10 — summary block in response
- **RED**: `test_summary_counts_correct` — 3 patents: 2 success, 1 error; `summary.success==2`, `summary.errors==1`, `summary.total==3`
- **GREEN**: compute summary after all results collected

### T11 — Stderr/stdout discipline
- **RED**: `test_no_stdout_output_outside_mcp` — capture both stdout and stderr during server operation; verify stdout contains ONLY valid JSON-RPC messages; any plain text goes to stderr
- **GREEN**: configure all logging to stderr; `logging.basicConfig(stream=sys.stderr)`

### T12 — End-to-end MCP message flow (integration, mocked)
- **RED**: `test_e2e_mcp_fetch_patents` — send MCP JSON-RPC `tools/call` message to server via pipe; receive response; verify structure matches expected schema
- **GREEN**: should pass when all above pass; wire up FastMCP stdio transport

### T13 — force_refresh bypasses cache
- **RED**: `test_force_refresh_ignores_cache` — patent is cached; `fetch_patents(patent_ids=["US7654321"], force_refresh=True)`; mock orchestrator called (not cache)
- **GREEN**: check `force_refresh` flag before cache lookup

---

## Rust Implementation

### T14 — Rust: MCP JSON-RPC stdio transport
- **RED**: `test_rust_server_stdin_stdout` — send MCP `initialize` + `tools/list` messages via stdin; verify stdout contains valid JSON-RPC responses
- **GREEN**: implement minimal MCP JSON-RPC protocol handler in Rust (or use `mcp-server` crate if available; otherwise manual implementation)
- **NOTES**: MCP protocol is straightforward JSON-RPC 2.0 over stdin; implement bare minimum: `initialize`, `tools/list`, `tools/call`

### T15 — Rust: fetch_patents tool delegates to Rust orchestrator
- **RED**: `test_rust_fetch_patents_mcp_call` — full MCP `tools/call` for `fetch_patents`; verify result JSON shape
- **GREEN**: wire Rust `FetcherOrchestrator` into MCP handler

### T16 — Parity: Python MCP response == Rust MCP response
- `test_mcp_response_parity` — same patent ID, same mock server → identical MCP response JSON (normalized)

---

## CLI entry points

### T17 — `python -m patent_mcp.server` starts
- **RED**: `test_server_cli_entry_point` — subprocess call to `python -m patent_mcp.server --help`; exits 0
- **GREEN**: `__main__.py` + argparse with `--cache-dir`, `--log-level`

### T18 — `patent-mcp-server` binary (Rust)
- **RED**: `test_rust_binary_help` — `cargo run -- --help`; exits 0
- **GREEN**: `clap` CLI with same flags as Python

---

## Acceptance Criteria
- All unit tests pass; end-to-end test mocks all I/O
- `postprocess_query` accepted without error in v1
- stdout is pure JSON-RPC (verified by test)
- Token budget prevents oversized responses

## Dependencies
All other nodes.
