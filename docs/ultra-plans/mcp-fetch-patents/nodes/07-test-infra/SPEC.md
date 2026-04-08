# SPEC — 07-test-infra: Cross-Implementation Test Infrastructure

## Responsibility
Provide shared test fixtures, a deterministic HTTP mock server, and a parity test harness that validates Python and Rust produce identical output for all patent fetch operations.

## Architecture

```
tests/
  fixtures/                   # shared JSON/HTML/binary fixtures
    us/
      US7654321/
        metadata.json         # expected canonical metadata
        patent.pdf            # tiny real or synthetic PDF
        patent.txt
        patent.md
        sources/
          uspto_response.json  # mocked USPTO API response
          google_html.html     # mocked Google Patents HTML
          epo_xml.xml          # mocked EPO OPS XML response
    ep/
      EP1234567B1/
        ...
    wo/
      WO2024123456/
        ...
    invalid/
      INVALID_ID/
        expected_error.json
  mock_server/
    server.py               # deterministic HTTP server (Python)
    routes.json             # URL → fixture file mapping
  python/                   # pytest test suite
    test_id_canon.py
    test_cache.py
    test_fetchers.py
    test_converters.py
    test_mcp_server.py
    test_cross_impl.py      # calls Rust binary, compares
  rust/
    # cargo test files under src/**/*_test.rs
  cross_impl/
    parity_runner.py        # drives both impls, compares output
    db_parity.py            # compares DB state after operations
```

## Deterministic HTTP Mock Server

### Purpose
Serve fixture responses for all patent data sources, eliminating all real network calls.

### Implementation
```python
# tests/mock_server/server.py
# Tiny aiohttp or starlette server
# Routes configured via routes.json:
{
  "GET /3.2/rest-services/published-data/publication/EP1234567": 
      "fixtures/ep/EP1234567B1/sources/epo_xml.xml",
  "GET /ibd-api/v1/patent/US7654321":
      "fixtures/us/US7654321/sources/uspto_response.json",
  ...
}
# Returns Content-Type and exact file contents
# Logs all requests to stderr for debugging
```

### Test Configuration
Both Python and Rust tests point to the mock server:
```
PATENT_USPTO_BASE_URL=http://localhost:18080
PATENT_EPO_BASE_URL=http://localhost:18080
PATENT_GOOGLE_PATENTS_BASE_URL=http://localhost:18080
... (all source base URLs overrideable via env)
```
The mock server starts before tests and shuts down after.

### Python: `conftest.py`
```python
@pytest.fixture(scope="session")
def mock_server():
    server = start_mock_server(port=18080, routes="tests/mock_server/routes.json")
    yield server
    server.stop()
```

### Rust: `build.rs` or test setup
Start mock server subprocess before integration tests:
```rust
// tests/integration_tests.rs
fn setup_mock_server() -> Child {
    Command::new("python").args(["-m", "tests.mock_server.server"]).spawn().unwrap()
}
```

## Parity Test Harness

### Approach
For each fixture patent in `tests/fixtures/`:
1. Run Python fetch: `python -m patent_mcp.cli fetch {patent_id} --cache-dir /tmp/test-py`
2. Run Rust fetch: `./patent-mcp-server fetch {patent_id} --cache-dir /tmp/test-rust`
3. Compare outputs:
   - `metadata.json` must be semantically identical (normalized JSON diff)
   - `sources.json` must list same sources attempted + same success/failure
   - `patent.md` must be identical (or within configurable similarity threshold)
   - `patent.txt` must be identical
   - DB state: same rows inserted in same tables with same values

### DB Parity Check
Both impls write debug DB operation logs (to stderr in test mode):
```
DB_INSERT patents id=US7654321 title=... filing_date=...
DB_INSERT patent_locations patent_id=US7654321 format=pdf ...
```
Log lines compared for equality after normalization (strip timestamps).

### Fuzzing
- Python: `hypothesis` library generates random patent ID strings
- Rust: `cargo-fuzz` target for canonicalization
- Both: random ID strings must not panic/crash; output format must be valid JSON
- Parity: Python(fuzz_input) canonical == Rust(fuzz_input) canonical

## Test Performance Requirements
- All unit tests (Python + Rust combined): <1s
- Mock server startup: <100ms (startup not counted in <1s budget)
- Parity tests run separately (not in <1s unit test suite)
- Playwright tests: disabled by default (`PATENT_BROWSER_TESTS=0`)
- Marker converter: disabled by default (`PATENT_DISABLE_MARKER=1`)

## Fixture Coverage Requirements
At minimum, one fixture per:
- US patent (full: PDF + text + images)
- US patent application
- EP patent
- WO PCT application
- JP patent
- One parse error case (invalid ID)
- One not-found case (valid ID, no sources have it)
- One partial-success case (some sources found it, others didn't)

## Test Tooling
- Python: `pytest`, `pytest-asyncio`, `respx` (HTTP mocking for httpx), `hypothesis`
- Rust: `cargo test`, `wiremock` crate (alternative: point to Python mock server), `cargo-fuzz`
- CI: GitHub Actions matrix running both Python and Rust tests

## Dependencies
All other nodes (07 validates all of them)

## Feasibility Notes
- <1s unit test constraint is achievable: all I/O mocked, no real PDF processing in unit tests
- Parity testing across languages is non-trivial but well-defined given the JSON schemas
- Fuzzing is a nice-to-have for v1; focus on fixture-based parity first
