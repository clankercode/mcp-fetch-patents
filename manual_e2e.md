# Manual E2E Test Cases for patent-mcp-server

## Purpose

This document defines end-to-end test cases for the patent-mcp-server Rust MCP server. Each test is executed by sending a natural-language prompt to the Claude CLI, which communicates with the server over stdio JSON-RPC. The goal is to verify that all three MCP tools (`fetch_patents`, `list_cached_patents`, `get_patent_metadata`) behave correctly across normal, edge-case, and error scenarios.

## Prerequisites

1. The patent-mcp-server binary is built and registered as an MCP server named `patent-tools` in your Claude CLI configuration (`.mcp.json`).
2. The Claude CLI (`claude`) is installed and authenticated.
3. For cache-empty tests (E2E-006), delete the global cache directory before running.
4. For cache-dependent tests (E2E-007, E2E-008, E2E-010), run E2E-001 first so that at least US7654321 is cached.

## How to Run

Each test case includes an exact prompt. Run it with:

```bash
claude --model haiku -p \
  --allowedTools "mcp__patent_tools__fetch_patents,mcp__patent_tools__list_cached_patents,mcp__patent_tools__get_patent_metadata" \
  --permission-mode bypassPermissions \
  "PROMPT HERE"
```

Replace `"PROMPT HERE"` with the prompt from the test case. Inspect the Claude output for the pass criteria listed.

---

## Test Cases

### fetch_patents — Basic Functionality

#### E2E-001: Fetch a single US patent

- **ID**: E2E-001
- **Tool**: `fetch_patents`
- **Category**: Basic functionality
- **Prompt**:
  ```
  Use the fetch_patents tool to fetch patent US7654321. Return the full raw tool response verbatim — do not summarize or interpret it.
  ```
- **Expected Behavior**: The server delegates to the Python backend, which tries multiple upstream sources (USPTO, EPO, Espacenet, web_search, etc.) and returns a structured result. The MCP server faithfully returns whatever the backend produces.
- **Pass Criteria**:
  - Response contains a `results` array with exactly 1 entry.
  - That entry has a `canonical_id` field (e.g., `US7654321`).
  - That entry has `success`, `from_cache`, `files`, `metadata`, and `error` fields (all present, even if null/empty).
  - A `summary` object is present with `total: 1` and numeric fields for `success`, `cached`, `errors`, `total_duration_ms`.
  - The server does not crash — a structured response is always returned.
- **Notes**: Upstream sources may be unavailable (403/404), so `success` may be `false` with empty `files` and `null` metadata. This is expected behavior — the test validates MCP protocol correctness, not upstream availability. If all sources fail, `summary.errors` will be `1`.

---

#### E2E-002: Fetch with an empty patent_ids array

- **ID**: E2E-002
- **Tool**: `fetch_patents`
- **Category**: Error handling
- **Prompt**:
  ```
  Use the fetch_patents tool with an empty patent_ids array (no patent IDs). Return the full raw tool response.
  ```
- **Expected Behavior**: The server accepts the call but returns a response indicating zero patents were processed, or returns an error indicating that at least one patent ID is required.
- **Pass Criteria**:
  - Either the summary shows `total: 0, success: 0, errors: 0`, OR the response contains an error message indicating empty input is invalid.
  - No crash or unhandled exception occurs.
- **Notes**: The exact behavior (empty success vs. input validation error) depends on server implementation. Either is acceptable as long as it is graceful.

---

#### E2E-003: Fetch multiple patents in a batch

- **ID**: E2E-003
- **Tool**: `fetch_patents`
- **Category**: Batch operations
- **Prompt**:
  ```
  Use the fetch_patents tool to fetch these patents: US7654321, EP1234567. Return the full raw tool response verbatim.
  ```
- **Expected Behavior**: The server processes both patent IDs and returns a result entry for each.
- **Pass Criteria**:
  - The response contains exactly 2 result entries in the `results` array.
  - `summary.total` equals `2`.
  - Each result entry has its own `canonical_id`, `success`, `from_cache`, `files`, `metadata`, and `error` fields.
  - `summary.total_duration_ms` is present and is a number.
  - `summary.success + summary.errors` equals `summary.total`.
- **Notes**: Upstream sources may be unavailable, so entries may have `success: false`. The test validates that batch processing works correctly at the MCP protocol level — both IDs are processed and reported.

---

#### E2E-004: Fetch with a malformed/invalid patent ID

- **ID**: E2E-004
- **Tool**: `fetch_patents`
- **Category**: Error handling
- **Prompt**:
  ```
  Use the fetch_patents tool to fetch patent ID "INVALID-XXXXX-NOTREAL". Return the full raw tool response verbatim.
  ```
- **Expected Behavior**: The server canonicalizes the ID (resulting in something like `UNKNOWN/INVALID-XXXXX-NOTREAL`), attempts to fetch, fails, and returns an error entry.
- **Pass Criteria**:
  - The response contains 1 result entry in the `results` array.
  - That entry has `success: false`.
  - `summary.total` equals `1`, `summary.errors` equals `1`, `summary.success` equals `0`.
  - The server does not crash — a structured JSON response is returned.
- **Notes**: The `error` field may be `null` (the error is reflected in the summary counts). The `canonical_id` may be prefixed with `UNKNOWN/` to indicate unrecognized format.

---

#### E2E-005: Fetch with force_refresh=true (cache bypass)

- **ID**: E2E-005
- **Tool**: `fetch_patents`
- **Category**: Cache behavior
- **Prompt**:
  ```
  Use the fetch_patents tool to fetch patent US7654321 with force_refresh set to true. Return the full raw tool response verbatim.
  ```
- **Expected Behavior**: The server accepts the `force_refresh` parameter and attempts to re-fetch from upstream (bypassing cache).
- **Pass Criteria**:
  - The result entry has `from_cache: false` (refresh was forced, even if result fails).
  - The response has the standard structure: `results` array, `summary` object.
  - `summary.total` equals `1`.
  - The server does not crash.
- **Notes**: The Rust server currently does not implement `force_refresh` (it's accepted but may be a no-op). The key test is that the parameter is accepted without error. Upstream sources may still be unavailable.

---

### list_cached_patents

#### E2E-006: List cached patents when cache is empty

- **ID**: E2E-006
- **Tool**: `list_cached_patents`
- **Category**: Basic functionality
- **Prompt**:
  ```
  Use the list_cached_patents tool to list all cached patents. Return the full raw tool response.
  ```
- **Expected Behavior**: With an empty cache, the server returns an empty list.
- **Pass Criteria**:
  - Response contains a `patents` field that is an empty array (`[]`).
  - Response contains `count: 0`.
  - No error is returned.
- **Notes**: Clear the global cache before running: `rm -rf ~/.local/share/patent-cache/`.

---

#### E2E-007: List cached patents after fetching

- **ID**: E2E-007
- **Tool**: `list_cached_patents`
- **Category**: Basic functionality
- **Prompt**:
  ```
  Use the list_cached_patents tool to list all cached patents. Return the full raw tool response verbatim.
  ```
- **Expected Behavior**: The server returns whatever is in the cache. If upstream fetches succeeded, entries will appear; if not, the cache may still be empty.
- **Pass Criteria**:
  - Response contains a `patents` field (array) and a `count` field (number).
  - `count` equals the length of the `patents` array.
  - If entries exist, each has `canonical_id` and `cache_dir` fields.
  - No error or crash occurs.
- **Notes**: Run after E2E-001. If upstream sources were unavailable, cache may still be empty (count: 0) — this is acceptable. The test validates the tool works, not that upstream sources are available.

---

### get_patent_metadata

#### E2E-008: Get metadata for a patent (cache lookup)

- **ID**: E2E-008
- **Tool**: `get_patent_metadata`
- **Category**: Basic functionality
- **Prompt**:
  ```
  Use the get_patent_metadata tool to get metadata for patent US7654321. Return the full raw tool response verbatim.
  ```
- **Expected Behavior**: The server performs a cache-only lookup (no network call) and returns the result.
- **Pass Criteria**:
  - Response contains a `results` array with exactly 1 entry.
  - That entry has `patent_id` field matching the input.
  - That entry has a `canonical_id` field.
  - That entry has a `metadata` field (may be `null` if the patent was never successfully cached, or populated with patent details if it was).
  - No crash occurs.
- **Notes**: If E2E-001's upstream fetch failed, metadata will be `null` — this is correct behavior for a cache-only tool.

---

#### E2E-009: Get metadata for a patent that is NOT cached

- **ID**: E2E-009
- **Tool**: `get_patent_metadata`
- **Category**: Error handling
- **Prompt**:
  ```
  Use the get_patent_metadata tool to get metadata for patent US9999999B2. Return the full raw tool response.
  ```
- **Expected Behavior**: Since US9999999B2 was never fetched, the server cannot return metadata from cache. It should return an error or empty metadata for that ID.
- **Pass Criteria**:
  - Response contains a `results` array with 1 entry.
  - That entry indicates failure: either `metadata` is null/empty, or an `error` field is present explaining the patent is not cached.
  - The server does not attempt a network fetch (this tool is cache-only).
  - No crash occurs.
- **Notes**: Ensure US9999999B2 has NOT been fetched before running this test.

---

#### E2E-010: Get metadata with multiple patent IDs

- **ID**: E2E-010
- **Tool**: `get_patent_metadata`
- **Category**: Batch operations
- **Prompt**:
  ```
  Use the get_patent_metadata tool to get metadata for these patents: US7654321, US0000000. Return the full raw tool response verbatim.
  ```
- **Expected Behavior**: The server performs cache lookups for both IDs and returns a result for each.
- **Pass Criteria**:
  - `results` array contains exactly 2 entries.
  - Each entry has `patent_id`, `canonical_id`, and `metadata` fields.
  - Both entries are present regardless of individual cache hit/miss (no short-circuiting).
  - No crash occurs.
- **Notes**: `metadata` may be `null` for both entries if they were never successfully cached. The test validates batch lookup at the MCP level.

---

### Edge Cases

#### E2E-011: Fetch patents with various ID formats (canonicalization)

- **ID**: E2E-011
- **Tool**: `fetch_patents`
- **Category**: Edge cases / ID canonicalization
- **Prompt**:
  ```
  Use the fetch_patents tool to fetch these patent IDs: "US7654321", "US7654321B2", "7654321". Return the full raw tool response verbatim.
  ```
- **Expected Behavior**: The server canonicalizes each ID format and returns a result entry for each. `US7654321` and `US7654321B2` should both map to a US canonical form. The bare `7654321` may be treated as unknown jurisdiction.
- **Pass Criteria**:
  - The `results` array contains 3 entries (one per input ID).
  - `US7654321` and `US7654321B2` produce `canonical_id` values that are both US-prefixed.
  - The bare `7654321` either canonicalizes to some form or gets an `UNKNOWN/` prefix — either is acceptable.
  - No crash occurs — the server handles all three formats gracefully.
  - `summary.total` equals `3`.
- **Notes**: The test validates ID canonicalization, not upstream fetch success. All entries may have `success: false` due to upstream unavailability.

---

#### E2E-012: Prompt that requests a non-existent tool

- **ID**: E2E-012
- **Tool**: N/A (tests graceful handling of unknown tool)
- **Category**: Edge cases / error handling
- **Prompt**:
  ```
  Use the delete_all_patents tool to wipe the patent cache. Return the full raw tool response.
  ```
- **Expected Behavior**: The Claude CLI does not have a `delete_all_patents` tool available. Claude should report that no such tool exists rather than hallucinating a response.
- **Pass Criteria**:
  - Claude does NOT fabricate a successful tool call response.
  - Claude indicates that the requested tool (`delete_all_patents`) is not available, or uses one of the allowed tools to approximate the request and explains what it did.
  - No server crash.
- **Notes**: This test validates the integration between the Claude CLI, the allowed-tools list, and the MCP server's tool registry. Because `--allowedTools` restricts what tools Claude can call, Claude should recognize that `delete_all_patents` is not in the allowed set.

---

### Parameter Combinations

#### E2E-013: postprocess_query parameter accepted

- **ID**: E2E-013
- **Tool**: `fetch_patents`
- **Category**: Parameter combinations
- **Prompt**:
  ```
  Use the fetch_patents tool to fetch patent US7654321 with postprocess_query set to "find claims related to semiconductors". Return the full raw tool response verbatim.
  ```
- **Expected Behavior**: Server accepts the parameter without error, returns standard response structure.
- **Pass Criteria**:
  - `results` array contains 1 entry.
  - `summary.total` equals `1`.
  - No error about unknown parameter.
- **Notes**: Validates that `postprocess_query` is a recognized parameter and does not cause a schema validation error.

---

#### E2E-014: Multiple optional parameters combined

- **ID**: E2E-014
- **Tool**: `fetch_patents`
- **Category**: Parameter combinations
- **Prompt**:
  ```
  Use the fetch_patents tool to fetch patent EP1234567 with force_refresh set to true and postprocess_query set to "extract abstract". Return the full raw tool response verbatim.
  ```
- **Expected Behavior**: Both parameters accepted, valid response.
- **Pass Criteria**:
  - `summary.total` equals `1`.
  - `from_cache` equals `false`.
  - No parameter error.
- **Notes**: Validates that multiple optional parameters can be combined without conflict.

---

### Error Handling / Boundary

#### E2E-015: get_patent_metadata with empty array

- **ID**: E2E-015
- **Tool**: `get_patent_metadata`
- **Category**: Error handling / boundary
- **Prompt**:
  ```
  Use the get_patent_metadata tool with an empty patent_ids array (pass patent_ids as []). Return the full raw tool response verbatim.
  ```
- **Expected Behavior**: Empty results array, no error.
- **Pass Criteria**:
  - `results` equals `[]`.
  - No crash.
- **Notes**: Tests boundary behavior when no IDs are provided to the metadata tool.

---

### Interaction Sequences

#### E2E-016: Sequential fetch then metadata (cache interaction)

- **ID**: E2E-016
- **Tool**: `fetch_patents` then `get_patent_metadata`
- **Category**: Interaction sequences
- **Prompt**:
  ```
  First use the fetch_patents tool to fetch patent US8765432. Then use get_patent_metadata to get metadata for US8765432. Return both raw tool responses verbatim.
  ```
- **Expected Behavior**: Both tools called, metadata lookup returns result for the ID.
- **Pass Criteria**:
  - Both responses have valid structure.
  - Metadata lookup returns entry with `canonical_id` and `metadata` field (may be `null` if fetch didn't succeed upstream).
- **Notes**: Validates that a fetch followed by a metadata lookup interacts correctly with the cache layer.

---

### Boundary Values

#### E2E-017: Special characters in patent IDs

- **ID**: E2E-017
- **Tool**: `fetch_patents`
- **Category**: Boundary values
- **Prompt**:
  ```
  Use the fetch_patents tool to fetch these patent IDs: "US-7654321-B2", "US 7654321". Return the full raw tool response verbatim.
  ```
- **Expected Behavior**: Server canonicalizes IDs gracefully, returns entries for each.
- **Pass Criteria**:
  - `results` has 2 entries, each with `canonical_id`.
  - No crash.
- **Notes**: Tests that hyphens and spaces in patent IDs are handled by the canonicalization logic.

---

#### E2E-018: Very large batch fetch

- **ID**: E2E-018
- **Tool**: `fetch_patents`
- **Category**: Boundary values / batch
- **Prompt**:
  ```
  Use the fetch_patents tool to fetch these patent IDs: "US1000001", "US1000002", "US1000003", "US1000004", "US1000005". Return the full raw tool response verbatim.
  ```
- **Expected Behavior**: Server processes all 5 IDs.
- **Pass Criteria**:
  - `results` has 5 entries.
  - `summary.total` equals `5`.
  - No crash.
- **Notes**: Validates that a larger batch of IDs is processed without issues.

---

### Cache Behavior

#### E2E-019: Repeated fetch of same patent (cache hit on second call)

- **ID**: E2E-019
- **Tool**: `fetch_patents` (called twice)
- **Category**: Cache behavior
- **Prompt**:
  ```
  Use the fetch_patents tool to fetch patent US7654321. Then call fetch_patents again for the same patent US7654321. Return both raw tool responses and note any difference in from_cache between the two calls.
  ```
- **Expected Behavior**: Second call may serve from cache if first succeeded.
- **Pass Criteria**:
  - Both responses have valid structure.
  - Both have `canonical_id` equal to `US7654321`.
- **Notes**: If the first fetch succeeded, the second call should have `from_cache: true`. If the first failed, both may have `from_cache: false`.

---

### Response Structure Validation

#### E2E-020: get_patent_metadata with single uncached patent returns correct structure

- **ID**: E2E-020
- **Tool**: `get_patent_metadata`
- **Category**: Response structure validation
- **Prompt**:
  ```
  Use the get_patent_metadata tool for patent WO2020123456. Return the full raw tool response verbatim.
  ```
- **Expected Behavior**: Returns result with `canonical_id` for WO-format patent.
- **Pass Criteria**:
  - `results` array contains 1 entry with `patent_id`, `canonical_id` (WO-prefixed), and `metadata` field present (may be `null`).
- **Notes**: Validates that WO-format patent IDs are handled correctly by the metadata tool.

---

### Batch Operations / Deduplication

#### E2E-021: Batch fetch with duplicate patent IDs

- **ID**: E2E-021
- **Tool**: `fetch_patents`
- **Category**: Batch operations / deduplication
- **Prompt**:
  ```
  Use the fetch_patents tool to fetch these patent IDs: "US7654321", "US7654321", "EP1234567", "US7654321". Return the full raw tool response verbatim.
  ```
- **Expected Behavior**: Server processes all 4 IDs (or deduplicates). Either behavior acceptable.
- **Pass Criteria**:
  - `results` array has entries (3 or 4).
  - `summary.total` matches `results` length.
  - All entries have standard fields.
  - No crash.
- **Notes**: Tests whether the server deduplicates identical IDs or processes each occurrence separately. Either behavior is acceptable.

---

### International Jurisdiction Formats

#### E2E-022: Large batch metadata with diverse jurisdictions

- **ID**: E2E-022
- **Tool**: `get_patent_metadata`
- **Category**: Batch operations / scaling
- **Prompt**:
  ```
  Use the get_patent_metadata tool for these patent IDs: "US7654321", "US1111111", "EP1234567", "JP2020123456", "CN201980123456", "GB2345678", "AU2020123456". Return the full raw tool response verbatim.
  ```
- **Expected Behavior**: 7 result entries, each with `patent_id`, `canonical_id`, `metadata`.
- **Pass Criteria**:
  - `results` has 7 entries, each with all 3 fields.
  - No crash.
- **Notes**: Tests batch metadata lookup across multiple jurisdictions (US, EP, JP, CN, GB, AU). All metadata may be `null` if patents were never cached.

---

#### E2E-023: Fetch with international jurisdiction formats

- **ID**: E2E-023
- **Tool**: `fetch_patents`
- **Category**: ID canonicalization / international
- **Prompt**:
  ```
  Use the fetch_patents tool to fetch these patent IDs: "JP2020123456", "CN201980123456", "KR10-2020-1234567", "CA3011111", "AU2020123456". Return the full raw tool response verbatim.
  ```
- **Expected Behavior**: 5 result entries with appropriate jurisdiction-prefixed `canonical_id`s.
- **Pass Criteria**:
  - `results` has 5 entries, each with jurisdiction-prefixed `canonical_id`.
  - `summary.total` equals `5`.
  - No crash.
- **Notes**: Validates canonicalization for non-US jurisdictions (JP, CN, KR, CA, AU). All entries may have `success: false` due to upstream unavailability.

---

### Error Handling / Batch Robustness

#### E2E-024: Mixed valid and invalid IDs in batch

- **ID**: E2E-024
- **Tool**: `fetch_patents`
- **Category**: Error handling / batch robustness
- **Prompt**:
  ```
  Use the fetch_patents tool to fetch these patent IDs: "US7654321", "INVALID_GARBAGE_123", "EP1234567", "NOT_A_REAL_ID", "JP2020123456". Return the full raw tool response verbatim.
  ```
- **Expected Behavior**: All 5 processed. Valid IDs get proper `canonical_id`; invalid ones get `UNKNOWN/` prefix.
- **Pass Criteria**:
  - `results` has 5 entries.
  - Invalid IDs have `success: false`.
  - `summary.total` equals `5`.
  - No crash or short-circuit.
- **Notes**: Validates that invalid IDs do not cause the entire batch to fail — each ID is processed independently.

---

### Parameter Combinations (Advanced)

#### E2E-025: force_refresh + postprocess_query combined in batch context

- **ID**: E2E-025
- **Tool**: `fetch_patents`
- **Category**: Parameter combinations
- **Prompt**:
  ```
  Use the fetch_patents tool to fetch patent US7654321 with force_refresh set to true and postprocess_query set to "extract key claims about semiconductor manufacturing". Return the full raw tool response verbatim.
  ```
- **Expected Behavior**: Both params accepted, `from_cache=false`.
- **Pass Criteria**:
  - `results` has 1 entry.
  - `from_cache` equals `false`.
  - `canonical_id` equals `US7654321`.
  - `summary.total` equals `1`.
  - No parameter error.
- **Notes**: E2E-014 tested with EP patent; this tests with US patent to validate consistency.

---

### Tool Chaining / Cache Growth

#### E2E-026: Multi-step fetch->list->fetch->list (cache growth)

- **ID**: E2E-026
- **Tool**: `fetch_patents` and `list_cached_patents` (chained)
- **Category**: Tool chaining / cache interaction
- **Prompt**:
  ```
  First use fetch_patents to fetch "US7654321", "EP1234567", "JP2020123456". Then use list_cached_patents, note the count. Then use fetch_patents for "US9999999", "CN201980123456". Then list_cached_patents again. Return all 4 raw tool responses and note count changes.
  ```
- **Expected Behavior**: 4 tool calls complete. Second list count >= first list count.
- **Pass Criteria**:
  - All 4 responses have valid structure.
  - First fetch has 3 results, second has 2.
  - Both list responses have `patents` array and `count`.
  - Second count >= first count.
  - No crash.
- **Notes**: If upstream sources are unavailable, cache counts may not grow. The test validates that the tool chaining sequence completes without errors.

---

### Defensive Canonicalization

#### E2E-027: Garbage IDs to get_patent_metadata

- **ID**: E2E-027
- **Tool**: `get_patent_metadata`
- **Category**: Error handling / canonicalization
- **Prompt**:
  ```
  Use the get_patent_metadata tool for these patent IDs: "GARBAGE123XYZ", "123", "---", "null", "fake-patent". Return the full raw tool response verbatim.
  ```
- **Expected Behavior**: 5 entries, all with null metadata, `canonical_id`s with `UNKNOWN/` prefix or similar.
- **Pass Criteria**:
  - `results` has 5 entries, each with `patent_id`, `canonical_id`, `metadata=null`.
  - No crash.
- **Notes**: Tests that completely nonsensical IDs are handled defensively. The metadata tool should never crash on arbitrary string input.

---

### Edge Cases / Input Validation

#### E2E-028: Empty string and whitespace-only patent IDs

- **ID**: E2E-028
- **Tool**: `fetch_patents`
- **Category**: Edge cases / input validation
- **Prompt**:
  ```
  Use the fetch_patents tool to fetch these patent IDs: "", "   ". Return the full raw tool response verbatim.
  ```
- **Expected Behavior**: Server handles empty/whitespace IDs gracefully — either skips them, returns error entries, or canonicalizes with UNKNOWN/ prefix. No crash.
- **Pass Criteria**:
  - Response has `results` array and `summary`.
  - If entries exist, each has standard fields.
  - No unhandled exception.
- **Notes**: Tests defensive handling of degenerate input. The server should not panic on empty or whitespace-only strings.

---

#### E2E-029: Very long patent ID (100+ characters)

- **ID**: E2E-029
- **Tool**: `fetch_patents`
- **Category**: Boundary values / input validation
- **Prompt**:
  ```
  Use the fetch_patents tool to fetch this patent ID: "US12345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890". Return the full raw tool response verbatim.
  ```
- **Expected Behavior**: Server handles excessively long input gracefully. Returns result with `success=false` or canonicalizes.
- **Pass Criteria**:
  - `results` has 1 entry.
  - `summary.total` equals `1`.
  - No crash or timeout.
- **Notes**: Tests that oversized input does not cause buffer overflow, excessive memory use, or server hang.

---

### Parameter Combinations (Defaults)

#### E2E-030: Explicit force_refresh=false

- **ID**: E2E-030
- **Tool**: `fetch_patents`
- **Category**: Parameter combinations
- **Prompt**:
  ```
  Use the fetch_patents tool to fetch patent US7654321 with force_refresh explicitly set to false. Return the full raw tool response verbatim.
  ```
- **Expected Behavior**: Parameter accepted (it's the default), valid response returned.
- **Pass Criteria**:
  - Standard response structure.
  - `summary.total` equals `1`.
  - No parameter error.
- **Notes**: Validates that explicitly passing the default value (`false`) does not cause a parameter validation error.

---

### Idempotency

#### E2E-031: Repeated list_cached_patents calls (idempotency)

- **ID**: E2E-031
- **Tool**: `list_cached_patents`
- **Category**: Idempotency / robustness
- **Prompt**:
  ```
  Call list_cached_patents three times in a row without any other operations between them. Return all three raw tool responses verbatim.
  ```
- **Expected Behavior**: All three calls return identical results.
- **Pass Criteria**:
  - All three have valid structure (`patents` array, `count` field).
  - Count is identical across all three.
  - No crash or state mutation.
- **Notes**: Read-only tool should be perfectly idempotent. Any count difference between calls would indicate a concurrency or state bug.

---

### Kind-Code Handling

#### E2E-032: Patent ID kind-code variants (A, B1, B2)

- **ID**: E2E-032
- **Tool**: `fetch_patents` then `get_patent_metadata`
- **Category**: ID canonicalization / kind-code handling
- **Prompt**:
  ```
  Use fetch_patents to fetch "US7654321", "US7654321B1", "US7654321B2". Then use get_patent_metadata for the same three IDs. Return all responses verbatim and note whether canonical_ids are the same or different.
  ```
- **Expected Behavior**: Server either normalizes all to same base ID or treats kind codes as distinct. Either is acceptable if consistent.
- **Pass Criteria**:
  - `fetch_patents` returns 3 entries.
  - `get_patent_metadata` returns 3 entries.
  - Canonical_ids are consistent between the two tools (same normalization behavior).
  - No crash.
- **Notes**: Tests whether kind codes (A, B1, B2) are stripped during canonicalization or preserved as distinct IDs. Either approach is valid if consistent across tools.

---

## Suggested Execution Order

Run the tests in this order to satisfy cache prerequisites:

1. **E2E-006** — List cache (empty). Run after clearing the global cache.
2. **E2E-001** — Fetch US7654321 (populates cache).
3. **E2E-007** — List cache (non-empty, confirms E2E-001 cached correctly).
4. **E2E-008** — Get metadata for cached patent.
5. **E2E-002** — Fetch with empty array.
6. **E2E-003** — Fetch batch.
7. **E2E-004** — Fetch invalid ID.
8. **E2E-005** — Fetch with force_refresh.
9. **E2E-009** — Get metadata for uncached patent.
10. **E2E-010** — Get metadata mixed cached/uncached.
11. **E2E-011** — ID format canonicalization.
12. **E2E-012** — Non-existent tool.
13. **E2E-013** — postprocess_query parameter accepted.
14. **E2E-014** — Multiple optional parameters combined.
15. **E2E-015** — get_patent_metadata with empty array.
16. **E2E-016** — Sequential fetch then metadata (cache interaction).
17. **E2E-017** — Special characters in patent IDs.
18. **E2E-018** — Very large batch fetch.
19. **E2E-019** — Repeated fetch of same patent (cache hit on second call).
20. **E2E-020** — get_patent_metadata with single uncached patent returns correct structure.
21. **E2E-021** — Batch fetch with duplicate patent IDs.
22. **E2E-022** — Large batch metadata with diverse jurisdictions.
23. **E2E-023** — Fetch with international jurisdiction formats.
24. **E2E-024** — Mixed valid and invalid IDs in batch.
25. **E2E-025** — force_refresh + postprocess_query combined (US patent).
26. **E2E-026** — Multi-step fetch->list->fetch->list (cache growth).
27. **E2E-027** — Garbage IDs to get_patent_metadata.
28. **E2E-028** — Empty string and whitespace-only patent IDs.
29. **E2E-029** — Very long patent ID (100+ characters).
30. **E2E-030** — Explicit force_refresh=false.
31. **E2E-031** — Repeated list_cached_patents calls (idempotency).
32. **E2E-032** — Patent ID kind-code variants (A, B1, B2).

## Interpreting Results

Each test is run manually. After Claude produces its output:

1. Read Claude's response to see which tool it called and the arguments it passed.
2. Check the reported tool response against the **Pass Criteria**.
3. If Claude summarizes the response (likely), look for the key fields mentioned in the pass criteria. You can ask Claude to show the raw JSON if needed.
4. Record pass/fail and any notes about unexpected behavior.
