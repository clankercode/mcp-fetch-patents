# SPEC — 03-source-fetchers: Source Fetcher Orchestrator

## Responsibility
Orchestrate fetching a patent from multiple sources in priority order, collect all available formats, and return a unified result. Never stop at first success — exhaust all sources to maximize format coverage.

## Architecture
The fetcher orchestrator is a **fan-out coordinator**:
1. Try each source in priority order
2. Accumulate all artifacts found (don't stop at first PDF found)
3. Some sources provide formats others don't (e.g., USPTO has full XML, Google Patents has better images)
4. Track which sources failed and why
5. Return aggregated result with all artifacts + per-source success/failure log

## Source Priority Order (default, configurable)
```
Tier 1 (most reliable, structured APIs):
  1. USPTO (for US patents)
  2. EPO OPS API (for EP, WO, and many others via exchange data)
  3. WIPO PatentScope (for WO)

Tier 2 (good but less reliable):
  4. Espacenet
  5. Lens.org
  6. Country-specific official sources (JPO, CNIPA, KIPRIS, etc.)

Tier 3 (scraping, less stable):
  7. Google Patents (Playwright)
  8. Patent-specific scrapers per jurisdiction

Tier 4 (last resort):
  9. Web search fallback (returns URLs only, no download)
```

## Fetcher Result Schema
```json
{
  "canonical_id": "US7654321",
  "success": true,
  "artifacts": {
    "pdf": "/abs/path/.patents/US7654321/patent.pdf",
    "txt": "/abs/path/.patents/US7654321/patent.txt",
    "md": "/abs/path/.patents/US7654321/patent.md",
    "images": ["/abs/path/.patents/US7654321/images/fig001.png"],
    "raw_xml": "/abs/path/.patents/US7654321/raw/patent.xml",
    "raw_html": "/abs/path/.patents/US7654321/raw/google.html"
  },
  "metadata": { ... },
  "sources": [
    {"name": "USPTO", "success": true, "formats": ["pdf", "txt", "xml"], "url": "..."},
    {"name": "EPO_OPS", "success": false, "error": "not_found"},
    {"name": "Google_Patents", "success": true, "formats": ["html", "images"], "url": "..."}
  ]
}
```

## Fetcher Interface (both Python and Rust)
Each source fetcher implements a common interface:
```
fetch(canonical_id, output_dir, config) -> SourceResult {
  success: bool,
  formats: list[str],
  artifacts: dict[str, path],
  metadata: dict | null,
  error: str | null
}
```

## Concurrency
- Sources within the same tier can be fetched in parallel (configurable)
- Tier 1 sources are tried sequentially first (fastest path to PDF)
- Background: continue fetching remaining sources for completeness after Tier 1 succeeds
- Configurable: `fetch_all_sources: bool` (default true for completeness)

## Children Nodes
- `03a-http-sources`: All HTTP/API-based sources
- `03b-browser-sources`: Playwright-based scrapers
- `03c-web-search-fallback`: Web search last resort

## Dependencies
- `01-id-canon`
- `02-cache-db`
- `06-config`

## Test Surface
- Unit: orchestrator returns partial success if only some sources work
- Unit: correct priority ordering respected
- Unit: concurrent fetching doesn't corrupt output directory
- Integration (mocked): all sources return canned fixtures, verify aggregation
