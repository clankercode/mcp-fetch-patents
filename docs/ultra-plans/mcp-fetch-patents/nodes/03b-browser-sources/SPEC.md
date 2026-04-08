# SPEC — 03b-browser-sources: Browser-Based Patent Scrapers

## Responsibility
Fetch patent data from websites that require JavaScript rendering or bot detection bypass, using Playwright (Python). Called as a subprocess from the Rust implementation.

## Sources Covered

### 1. Google Patents
- **URL**: `https://patents.google.com/patent/{canonical_id}`
- **Why browser**: Google Patents renders key content via JS and uses bot detection
- **Data available**: Full text, PDF download link, figures, metadata, citations, family members
- **Strategy**: Navigate to patent page, extract JSON-LD metadata, download PDF, extract figure URLs

### 2. Supplemental Scrapers (JS-required sites)
Sites initially attempted via plain HTTP (in 03a) but delegated here if JS is required:
- J-PlatPat (Japanese patent office)
- CNIPA (Chinese patents)  
- Any country source that requires JS rendering

## Playwright Architecture
```
Python process:
  playwright_scraper.py
    - async main(canonical_id, output_dir, source_name) -> SourceResult
    - chromium browser, headless mode
    - stealth: realistic user-agent, viewport, accept cookies
    - network interception: capture PDF downloads, image downloads
    - timeout: 60s per page load
```

**From Rust**: Call as subprocess:
```
python -m patent_mcp.scrapers.playwright_runner {args_as_json}
```
Rust reads stdout JSON result.

## Google Patents Extraction Logic
1. Navigate to `https://patents.google.com/patent/{id}`
2. Wait for `#wrapper > .patent-text` or equivalent selector
3. Extract: title, abstract, claims, description, inventors, assignee, dates from JSON-LD + DOM
4. Find PDF download link: `a[href*=".pdf"]` or `[data-direct-link]`
5. Download PDF via Playwright page context (handles auth cookies)
6. Extract figure URLs from `img.patent-image` elements
7. Download each figure image
8. Return `SourceResult` as JSON on stdout

## Bot Detection Avoidance
- Use `playwright-stealth` Python package
- Realistic viewport: 1280x800
- Normal mouse movement / delays
- Rotate user-agents periodically
- Do NOT implement CAPTCHA solving or other aggressive bypasses
- If blocked: log as failure, move to next source

## Subprocess Interface (for Rust)
```
# Stdin: JSON request
{
  "canonical_id": "US7654321",
  "output_dir": "/abs/path/.patents/US7654321/",
  "source": "google_patents",
  "config": { ... relevant config ... }
}

# Stdout: JSON result (same SourceResult schema as 03-source-fetchers)
# Stderr: debug/log output
# Exit code: 0 = success (even partial), 1 = fatal error
```

## Test Strategy
- Tests mock Playwright's `page.goto()` and `page.content()` at the network level
- Use `playwright` mock context / route interception to serve fixture HTML
- Test fixture: saved HTML pages for known patents (not fetched live during tests)
- Browser tests are skipped in CI unless `PATENT_BROWSER_TESTS=1` env var set
- `marker` and Playwright both excluded from <1s test suite by default

## Dependencies
- `playwright` Python package (chromium)
- `playwright-stealth` Python package
- `01-id-canon`, `02-cache-db`, `06-config`

## Feasibility Notes
- Playwright is well-established for this use case
- Google Patents bot detection is real but manageable with stealth settings
- Subprocess bridge from Rust is a reasonable pattern; alternative is pure Rust headless (chromiumoxide / fantoccini) but adds complexity
