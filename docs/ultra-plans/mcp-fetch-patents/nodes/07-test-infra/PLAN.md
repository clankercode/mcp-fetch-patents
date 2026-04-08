# PLAN — 07-test-infra: Cross-Implementation Test Infrastructure

*Build this early — it unblocks all subsequent testing.*
*Must be complete before 02-cache-db and 03x-sources are tested.*

---

## Phase 1: Fixture Files

### T01 — Fixture directory structure
- **Task**: Create the `tests/fixtures/` directory tree with subdirs: `us/`, `ep/`, `wo/`, `jp/`, `invalid/`, `partial/`
- **Acceptance**: directories exist; no test code yet

### T02 — US patent fixture (US7654321)
- **Task**: Create `tests/fixtures/us/US7654321/` containing:
  - `metadata.json` — expected canonical metadata (title, inventors, dates, etc.) — use a real public domain patent's metadata
  - `sources/uspto_response.json` — mocked PPUBS session response + document retrieval response
  - `sources/google_html.html` — mocked Google Patents HTML (minimal, contains patent data in JSON-LD)
  - `sources/epo_ops_xml.xml` — mocked EPO OPS bibliographic XML for a US patent
  - `patent_stub.pdf` — tiny valid 1-page PDF (generate programmatically with reportlab or fpdf2; NOT a real patent)
  - `patent_stub.txt` — expected text content of the stub PDF
- **Note**: All fixture files contain synthetic data — no real patent content needed for unit tests

### T03 — EP patent fixture (EP1234567B1)
- **Task**: Create `tests/fixtures/ep/EP1234567B1/` with equivalent files
- **Key difference**: EPO OPS XML response is the primary fixture here

### T04 — WO fixture (WO2024123456)
- **Task**: Create `tests/fixtures/wo/WO2024123456/` — WIPO web HTML scrape fixture

### T05 — JP fixture (JP2023123456)
- **Task**: `tests/fixtures/jp/JP2023123456/` — J-PlatPat HTML scrape fixture (minimal)

### T06 — Invalid ID fixture
- **Task**: `tests/fixtures/invalid/INVALID999/expected_error.json` — `{"status": "error", "error": "Could not parse patent ID"}`

### T07 — Partial success fixture
- **Task**: `tests/fixtures/partial/US9999999/` — metadata.json with `sources_used` showing some sources succeeded, some failed

### T07b — Playwright mock HTML fixtures
- **Task**: Create `tests/fixtures/browser/google_patents/`:
  - `US7654321.html` — minimal Google Patents page with: JSON-LD `<script type="application/ld+json">` containing patent metadata; 2 `<img class="patent-image">` elements; `<a href="...pdf">` download link; no actual patent content needed
  - `EP1234567B1.html` — same structure for EP
  - `WO2024123456.html` — same structure for WO
- **Task**: Add `PATENT_PLAYWRIGHT_MOCK_DIR` env var to test config pointing to these fixtures
- **Test**: `test_google_patents_html_fixture_valid` — load fixture, verify JSON-LD is parseable and contains title field

### T07c — Browser source mock directory structure
- **Task**: Document the `PATENT_PLAYWRIGHT_MOCK_DIR` convention in a `tests/fixtures/browser/README.md`:
  - File naming: `{canonical_id}.html`
  - Required JSON-LD fields: title, inventor, assignee, datePublished
  - Image elements: `<img class="patent-image" src="...">` (URLs don't need to resolve in mock mode)

---

## Phase 2: Mock HTTP Server

### T08 — routes.json mapping
- **Task**: Create `tests/mock_server/routes.json` mapping source API paths to fixture files:
  ```json
  {
    "GET /ppubs-api/v1/patent/US7654321": "fixtures/us/US7654321/sources/uspto_response.json",
    "GET /3.2/rest-services/published-data/publication/biblio/EP1234567": "fixtures/ep/EP1234567B1/sources/epo_ops_xml.xml",
    ...
  }
  ```
- Include: USPTO PPUBS session endpoint, document endpoint, PDF endpoint; EPO OPS endpoints; stub 404 for unknown IDs

### T09 — Mock server implementation
- **Task**: Implement `tests/mock_server/server.py`:
  - `aiohttp` or `starlette` server (choose starlette — already likely a dep via fastmcp)
  - Read routes from `routes.json` on startup
  - Match `{METHOD} {path}` → serve file contents with appropriate Content-Type
  - Return 404 JSON `{"error": "not_found"}` for unmapped routes
  - Log all requests to stderr (for test debugging)
  - Entry point: `python -m tests.mock_server.server --port 18080`
- **Test**: `test_mock_server_serves_fixture` — request a known route, verify response bytes match fixture file

### T10 — pytest conftest.py
- **Task**: Implement `tests/python/conftest.py`:
  - `mock_server` session-scoped fixture: starts mock server subprocess on a random free port; yields port; kills on teardown
  - `test_config` fixture: returns `PatentConfig` with all `source_base_urls` overriding to `http://localhost:{port}`, temp cache dir, disabled marker
  - `tmp_cache_dir` fixture: temp `.patents/` dir cleaned up after each test
- **Test**: `test_conftest_mock_server_starts` — verify server responds to health check

### T11 — Free port finder
- **Task**: utility `tests/python/utils.py::find_free_port()` — bind socket to port 0, get assigned port
- **Test**: `test_find_free_port_returns_int` — basic smoke test

---

## Phase 3: Parity + Rust Setup

### T12 — Rust: cargo test integration with Python mock server
- **Task**: `tests/rust/setup.rs` — `setup_mock_server()` starts Python mock server subprocess before integration tests; tears down after
- **Test**: Rust integration test that requests a known fixture URL from mock server

### T13 — Parity assertion utility
- **Task**: Implement `tests/cross_impl/parity.py`:
  - `normalize_result(d: dict) -> dict` — strip timestamps, sort keys, normalize paths to relative
  - `assert_parity(python_result, rust_result)` — compare normalized dicts; print diff on failure
- **Test**: `test_assert_parity_equal` — identical dicts pass; `test_assert_parity_diff` — different dicts raise `AssertionError` with readable diff

### T14 — DB parity log parser
- **Task**: Implement `tests/cross_impl/db_parity.py`:
  - Parse `DB_INSERT/DB_UPDATE/DB_DELETE` log lines from stderr
  - `compare_db_logs(py_log: str, rust_log: str) -> list[str]` — return list of differences
- **Test**: `test_db_logs_match` / `test_db_logs_differ_detected`

### T15 — Stub PDF generator
- **Task**: `tests/fixtures/generate_stub_pdf.py` — uses `reportlab` or `fpdf2` to generate a 1-page PDF with known text content; output `tests/fixtures/stub.pdf`
- Store the generated PDF in fixtures; regenerate via `python -m tests.fixtures.generate_stub_pdf` if needed
- **Test**: `test_stub_pdf_is_valid_pdf` — open with pymupdf, verify 1 page

---

## Phase 4: Parity Test Execution (Phase C — after Phase A + B complete)

### T16 — End-to-end parity: node 01 (canonicalization)
- **Task**: `tests/cross_impl/test_id_canon_parity.py`:
  - For each fixture patent ID: call Python `canonicalize(id)` → JSON; run `./patent-mcp-server canonicalize {id}` → JSON
  - `assert_parity(python_json, rust_json)`
- **Acceptance**: all fixture IDs produce identical canonical output

### T17 — End-to-end parity: node 02 (cache + DB)
- **Task**: `tests/cross_impl/test_cache_parity.py`:
  - Initialize both Python and Rust caches pointing to separate temp dirs
  - Run same sequence of store operations (using mock server for sources)
  - Export SQLite: `sqlite3 {db} .dump` for both
  - `assert_db_dump_parity(py_dump, rust_dump)` — compare schema + data rows

### T18 — End-to-end parity: node 03 (orchestrator)
- **Task**: `tests/cross_impl/test_orchestrator_parity.py`:
  - For each fixture patent: run Python `fetch()` → `OrchestratorResult` JSON; run Rust subprocess → JSON
  - `assert_parity()` on normalized results (strip duration_ms, timestamps)

### T19 — End-to-end parity: MCP protocol (node 05)
- **Task**: `tests/cross_impl/test_mcp_parity.py`:
  - Send identical MCP `tools/call fetch_patents` message to Python server (subprocess) and Rust server (subprocess)
  - Both servers point to same mock server and same temp cache dirs
  - `assert_parity()` on normalized MCP response JSON

### T20 — Fuzzing setup: Python (hypothesis)
- **Task**: `tests/python/test_fuzz_id_canon.py`:
  - `@given(st.text())` strategy on `canonicalize()` — verify: never raises, always returns `CanonicalPatentId`, `canonical` is a non-empty string
- **Task**: `tests/python/test_fuzz_config.py`:
  - `@given(st.dictionaries(...))` on env var dict — `load_config(env=...)` never raises

### T21 — Fuzzing setup: Rust (cargo-fuzz)
- **Task**: Create `fuzz/fuzz_targets/fuzz_canonicalize.rs` — fuzz target for `canonicalize()`
- **Task**: Add `fuzz/Cargo.toml` with `libfuzzer-sys`
- Run via `cargo +nightly fuzz run fuzz_canonicalize -- -max_total_time=60`

---

## Acceptance Criteria
- Mock server starts in <100ms (not counted in <1s test budget)
- All fixture files present and parseable
- `test_config` fixture provides complete isolation (no real network calls possible)
- Parity assert produces readable diff output on failure
- Rust integration tests can connect to Python mock server

## Dependencies
- Python: `starlette`, `uvicorn`, `reportlab` or `fpdf2`
- All fixture JSON uses the same field names as defined in INTERFACE.md files
