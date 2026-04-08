# PLAN — 03b-browser-sources: Playwright Browser Scrapers

*Depends on: 06-config, 01-id-canon*
*Playwright tests disabled by default (PATENT_BROWSER_TESTS=0)*
*Unit tests mock the Playwright subprocess interface*

---

## Python Implementation

### T01 — Playwright runner subprocess interface (JSON in/out)
- **RED**: `test_runner_accepts_json_stdin` — invoke `patent_mcp.scrapers.playwright_runner` as subprocess with JSON on stdin; verify it starts without crash (use a mock Playwright that returns immediately)
- **RED**: `test_runner_outputs_json_to_stdout` — mock Playwright context; verify stdout is valid JSON `SourceAttempt`-shaped
- **GREEN**: implement `playwright_runner.py` `main()` — read JSON from stdin, dispatch to source handler, write JSON to stdout

### T02 — Google Patents URL construction
- **RED**: `test_google_patents_url_us` — `GooglePatentsSource.build_url(CanonicalPatentId(canonical="US7654321"))` → `"https://patents.google.com/patent/US7654321"`
- **RED**: `test_google_patents_url_ep` — EP → `"https://patents.google.com/patent/EP1234567B1"`
- **GREEN**: implement `build_url()` with jurisdiction-specific formats

### T03 — Google Patents HTML extraction (mocked page)
- **RED**: `test_google_patents_metadata_extraction` — mock `page.content()` to return fixture HTML (tests/fixtures/.../google_html.html); verify title, abstract, inventors extracted from JSON-LD + DOM
- **RED**: `test_google_patents_pdf_link_found` — fixture HTML contains `<a href="...pdf">`; verify link captured
- **GREEN**: implement `GooglePatentsSource._extract_from_page()` — JSON-LD parsing first; DOM fallback

### T04 — Image URL extraction
- **RED**: `test_google_patents_image_urls_extracted` — fixture HTML has 3 `<img class="patent-image">` elements; verify 3 URLs returned
- **GREEN**: extract image URLs; deduplicate

### T05 — Playwright mock context (unit test mode)
- **RED**: `test_playwright_runner_with_mock_context` — provide `PATENT_PLAYWRIGHT_MOCK_DIR=tests/fixtures/...`; runner reads HTML from disk instead of launching browser; returns expected SourceAttempt
- **GREEN**: add `PATENT_PLAYWRIGHT_MOCK_DIR` env var support; if set, use mock context instead of real browser
- **REFACTOR**: this is the primary unit test path; real browser only when `PATENT_BROWSER_TESTS=1`

### T06 — Bot blocking graceful failure
- **RED**: `test_bot_block_returns_failure` — mock page raises `playwright.async_api.Error("Target closed")`; `SourceAttempt(success=False, error="Browser scraping failed: ...")`, no exception
- **GREEN**: broad try/except around browser operations; log failure; return SourceAttempt

### T07 — Rust subprocess bridge
- **RED**: `test_rust_calls_playwright_runner` — from Rust test: spawn `python -m patent_mcp.scrapers.playwright_runner` with mock JSON stdin; read stdout; parse as SourceAttempt
- **GREEN**: implement in `fetchers/browser.rs` — spawn Python subprocess, write JSON to stdin, read JSON from stdout

### T08 — Playwright runner graceful when Python unavailable
- **RED** (Rust): `test_playwright_unavailable` — mock `Command::new("python")` to fail; `BrowserSource.fetch()` → `SourceAttempt(success=False, error="Python not available for browser scraping")`
- **GREEN**: check Python availability on `BrowserSource` init; degrade gracefully

---

## Integration Tests (Playwright, gated behind PATENT_BROWSER_TESTS=1)

### T09 — Real Google Patents fetch (integration only)
- **SKIP in default test run** (`pytest.mark.browser`)
- Launch real Playwright; fetch a known public-domain patent; verify PDF downloaded
- Not counted toward <1s budget

---

## Acceptance Criteria
- Unit tests use `PATENT_PLAYWRIGHT_MOCK_DIR` — no real browser launched
- Playwright import not triggered in Rust (subprocess only)
- `SourceAttempt` produced for all outcomes including browser failures
- All unit tests complete in <100ms (mock context, no subprocess overhead beyond fixture read)

## Dependencies
- `06-config`, `01-id-canon`
- Python: `playwright`, `playwright-stealth`
- Rust: `std::process::Command` (no Rust playwright bindings)
