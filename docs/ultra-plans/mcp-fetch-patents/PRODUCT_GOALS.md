# PRODUCT GOALS — mcp-fetch-patents

*Finalized after user interview — 2026-04-07*

## Problem Statement
AI agents frequently need to access patents by ID. Fetching them is slow, unreliable, and the same patent gets re-fetched repeatedly across agent sessions. There is no standard local cache. Agents have no easy way to get structured, markdown-friendly patent content.

## Primary Goal
A single-responsibility MCP server that fetches patents by ID, caches them locally and globally, and returns file paths + metadata. Agents never wait for a patent twice.

## Implementation Strategy
- **Python first**: Quick-iteration reference implementation using Python + fastmcp
- **Rust second**: Production implementation mirroring Python's behavior exactly
- **Cross-validation**: Both implementations must produce identical output for all test fixtures and fuzzing inputs via a shared deterministic HTTP mock server test harness

## Confirmed Success Criteria

### Core (v1)
- [ ] Given a patent ID (any supported jurisdiction), return local file paths to all downloaded formats
- [ ] Second fetch returns instantly (cache hit, no network)
- [ ] Supports batching: MCP tool description explicitly encourages requesting 100+ patents at once
- [ ] Jurisdictions: US, EP, WO, and all major economic zones (JP, CN, KR, AU, CA, NZ, BR, Gulf states, etc.)
- [ ] Downloads: PDF, full-text (TXT), all figures/diagrams (PNG/JPEG)
- [ ] Converts PDF to Markdown (pymupdf4llm → pdftotext → marker, configurable order)
- [ ] OCRs figures via tesseract, stores captions; markdown embeds `![fig](images/fig.png)` references
- [ ] Falls back through N+1 sources if primary is unavailable
- [ ] Falls back to web search as last resort; returns URLs in metadata
- [ ] Global XDG index (`~/.local/share/patent-cache/index.db`, SQLite) tracks every patent across all caches
- [ ] Per-repo local cache in `.patents/` (XDG-style)
- [ ] Test suite runs in <1s (all I/O mocked)
- [ ] MCP transport: stdin/stdout only (HTTP in v2)
- [ ] Config: env vars + `~/.patents.toml` (env overrides file)
- [ ] API key docs for each source: where to get them, how to configure
- [ ] Playwright-based scraping for sources that require browser (can test without API keys)

### Deferred to v2
- [ ] Legal/application status timeline fetching (DB schema stubbed in v1)
- [ ] HTTP MCP transport
- [ ] PyPI + crates.io packaging
- [ ] `postprocess_query` execution (parameter accepted + logged in v1, calls `claude` CLI in v2)

## Non-Goals
- Does NOT return patent content in MCP response body (file paths + metadata only)
- Does NOT implement patent search/query by keyword (ID-based only)
- Does NOT process or summarize patents in v1
- Does NOT require paid APIs (free/scraping path must always exist)

## Key Design Principles
1. **Cache-first**: Check global index → local cache → network. Never fetch twice.
2. **Completeness over speed**: Download every available format, don't stop at first success
3. **Redundancy**: Multiple sources per jurisdiction; failure of one source is not failure of the fetch
4. **Transparency**: Return rich metadata (source used, formats found, cache location, any errors per source)
5. **Agent-friendly**: MCP tool description says "prefer batching; agents should request many patents at once"
6. **Dual-impl parity**: Python output = Rust output for all inputs, enforced by test harness
7. **Configurable by design**: Converter order, source priority, API keys all configurable

## Architecture Summary
```
MCP Client (agent)
      │ stdin/stdout (MCP protocol)
      ▼
MCP Server (Python / Rust)
      │
      ├─► Cache Check (global XDG DB + local .patents/)
      │         └─ HIT → return file paths immediately
      │
      ├─► Fetcher Chain (try each source in priority order)
      │         ├─ HTTP Sources (USPTO, EPO OPS, WIPO, Lens.org, ...)
      │         ├─ Browser Sources (Google Patents, Playwright-scraped)
      │         └─ Web Search Fallback (return URLs only)
      │
      ├─► Format Conversion
      │         ├─ PDF → Markdown (pymupdf4llm → pdftotext → marker)
      │         ├─ Image extraction
      │         └─ OCR captions (tesseract)
      │
      └─► Cache Store + Index Update → return metadata + file paths
```

## Sources (all to be integrated, redundancy key)
- Google Patents (Playwright scraper)
- USPTO Full-Text + PatentsView + PAIR
- EPO OPS API (free tier, broad coverage via exchange data)
- WIPO PatentScope
- Espacenet
- Lens.org
- Country-specific: JPO (Japan), CNIPA (China), KIPRIS (Korea), IP Australia, CIPO (Canada), IPONZ (NZ), BRPTO (Brazil), Gulf GCC patent office, etc.
- Web search (SerpAPI / DuckDuckGo) as last resort
