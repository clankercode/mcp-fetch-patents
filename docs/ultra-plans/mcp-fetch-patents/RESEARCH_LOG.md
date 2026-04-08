# RESEARCH LOG — mcp-fetch-patents

*Research completed 2026-04-07.*

---

## Prior Art Survey

### riemannzeta/patent_mcp_server (Python, most mature)
- Python 3.10+, `mcp[cli]` (FastMCP embedded), httpx async, HTTP/2 for PPUBS
- Sources: PPUBS (no auth, US full-text), USPTO ODP (ID.me API key required), PTAB, Litigation
- **PatentsView SHUT DOWN March 20, 2026** — graceful deprecation messages added
- Office Action APIs decommissioned early 2026
- No persistent disk cache (session token cache only, TTL-based)
- 52 tools, graceful degradation for dead APIs
- Token budget management (`MAX_RESPONSE_TOKENS`, `estimate_tokens`)
- Pattern: one `httpx.AsyncClient` per source, tenacity retry 3x, exponential backoff

### openpharma-org/patents-mcp (Python)
- Fork/derivative of riemannzeta
- Adds **BigQuery `patents-public-data`** — 90M+ publications, 17+ countries, US full-text
- BigQuery: 1TB/month free, SQL interface, covers 17+ national offices
- Setup friction: requires Google Cloud service account

### KunihiroS/google-patents-mcp (TypeScript)
- Single-file, SerpAPI only, no caching, no ID normalization, no retry logic
- No test suite; not production quality

### SoftwareStartups/patentscope (TypeScript/Bun)
- Cleanest architecture: `PatentScopeServer` class, separated `SerpApiClient` + `PatentService`
- Dual CLI+MCP mode (serve/search/get subcommands) — good UX pattern
- `resolvePatentId()` handles URL/bare ID/formatted ID normalization
- Bun pre-built binaries, no runtime dependency
- `get_patent` + `search_patents` tools (two tools vs. single)

---

## API Landscape (2025-2026)

| Source | Auth | Free | Coverage | Full Text | Notes |
|---|---|---|---|---|---|
| PPUBS (`ppubs.uspto.gov`) | None | Unlimited | US grants + apps | Yes | Session-based; HTTP/2 helps |
| USPTO ODP (`api.uspto.gov`) | ID.me API key | Free | US prosecution, PTAB, litigation | Metadata only | Significant onboarding friction |
| PatentsView | API key | Free | US granted | No | **SHUT DOWN 2026-03-20** — do not implement |
| EPO OPS v3.2 | OAuth2 (free reg) | 4GB/week | 160M docs, 100+ offices | EP full text | `developers.epo.org`; XML; best for EP+WO |
| Google Patents BigQuery | GCP service account | 1TB/month | 90M+, 17+ countries | US full text | Best programmatic multi-national |
| SerpAPI (Google Patents) | API key | 250/month | 100+ offices | Abstract only | Paid beyond 250; description via scrape |
| WIPO PatentScope | Subscription | None (600 CHF/yr) | 100M+, PCT-focused | PCT full text | **Effectively paywalled** — use as scraping target only |
| Lens.org API | Bearer token | 14-day trial only | 140M+, worldwide | Partial | Manual approval; not for prototyping |
| Espacenet (web) | None | Unlimited | 160M+ | EP via links | Scraping; uses OPS backend |

**Key updates from research:**
- PatentsView is dead — remove from plans
- WIPO is paywalled — implement as web scraping target only
- Lens.org is impractical for free use — implement as web scraping target
- BigQuery is the best multi-national source (add as Tier 1 source, optional due to GCP setup)
- PPUBS needs session caching (TTL-based, not per-request)

---

## PDF-to-Markdown Tools

| Tool | GPU | Speed | Accuracy | Tables | Notes |
|---|---|---|---|---|---|
| pymupdf4llm | No | 0.14s | Good for native PDFs | Good (bordered) | Best default for patents (post-1990 native PDF) |
| marker | Optional | Fast | High (general) | Good | Good fallback for scanned/older patents |
| pdfplumber | No | 0.10s | Good for text | Excellent (coordinate-based) | Best for claim tables specifically |
| Nougat | Yes | Slow | Best for arXiv | Moderate | **NOT suitable for patents** (overfits to arXiv) |

**Recommendation:** Default order: `pymupdf4llm → pdfplumber → marker`. Remove Nougat from consideration.
pdfplumber is valuable addition specifically for claim tables.

---

## MCP Framework

- **Recommendation: `mcp[cli]`** (official Python SDK with FastMCP embedded)
- `@mcp.tool()` decorators, Pydantic for input validation
- 70% of all MCP servers use FastMCP pattern
- Key pattern: `atexit`-registered async cleanup, one `httpx.AsyncClient` per source

---

## Cross-Cutting Patterns (from prior art)

1. **Graceful degradation**: Register tools for dead/unavailable APIs but return helpful messages
2. **Token budget management**: Estimate token count; truncate before returning to LLM
3. **Session token caching**: PPUBS session establishment is expensive; cache with TTL
4. **Stderr/stdout discipline**: All logging → stderr; MCP JSON-RPC → stdout ONLY
5. **Dual CLI+MCP mode**: patentscope approach — good for debugging/testing
6. **No disk cache in any existing project** — this is our key differentiator
7. **HTTP/2 for PPUBS** via `h2` Python package (undocumented but effective)

---

## Plan Updates Needed

- [ ] **03a-http-sources**: Remove PatentsView; downgrade WIPO to scraping-only; add BigQuery as Tier 1 (optional)
- [ ] **04-format-conversion**: Add pdfplumber to default chain; note Nougat unsuitability
- [ ] **02-cache-db**: Add PPUBS session token cache (separate from patent disk cache)
- [ ] **05-mcp-protocol**: Add token budget truncation utility; add `estimate_tokens` function
- [ ] **ROOT.md**: Note key differentiator is persistent disk cache (no prior art has this)
