# mcp-fetch-patents

**The last patent data tool your agent will ever need.**

Give any AI agent instant access to the entire global patent corpus. One MCP tool call. Throw any patent ID at it — US, EP, WO, JP, CN, KR, AU, CA, NZ, BR, IN — in any format, with or without kind codes, even raw Google Patents URLs. Get back PDF, Markdown, and structured metadata in seconds. Ask for it again? Instant, from cache.

Your agent shouldn't have to know that US patents live on USPTO PPUBS, European ones require EPO OPS OAuth2, and PCT applications need to be scraped from WIPO PatentScope. It shouldn't care that Espacenet has different HTML than CIPO, or that Google Patents needs a headless browser. This server ate all of that complexity so your agent never has to think about it.

```
"Fetch US7654321, EP1234567B1, and WO2024/123456"
  → 3 PDFs, 3 Markdown files, 3 metadata objects — cached forever
```

9 patent sources. 4 PDF-to-Markdown converters with OCR. Two-layer SQLite cache. Automatic retry with exponential backoff. Web search fallback when everything else fails. Zero configuration required — works out of the box with 6 of 9 sources.

---

## Why this exists

Patent data is the most fragmented public dataset on the planet. Every national patent office has its own API, auth scheme, document format, and rate limits. An agent doing patent analysis shouldn't need to know any of that — it should just say "get me this patent" and get it.

**9 patent sources, 1 tool call:**

| Source | Coverage | Auth required? |
|---|---|---|
| USPTO PPUBS | US granted + applications | No (session-based) |
| EPO OPS | EP, WO, 100+ offices via exchange data | Yes (OAuth2) |
| Espacenet | EP + EPO member states | No (scraped) |
| WIPO PatentScope | WO / PCT international | No (scraped) |
| IP Australia | AU patents | No (REST API) |
| CIPO | Canadian patents | No (scraped) |
| Google Patents | All jurisdictions | No (Playwright) |
| Google BigQuery | Bulk patent data | Yes (GCP credentials) |
| Web search fallback | Anything missed | No (DuckDuckGo) / Optional (SerpAPI) |

Sources are tried in priority order. First success wins (unless you set `PATENT_FETCH_ALL_SOURCES=true` to aggregate from all). If every structured source fails, the web search fallback finds the PDF anyway. We really don't like returning empty-handed.

## Quick start

### Install

```bash
# Rust (recommended)
cargo install patent-mcp-server

# or build from source:
cargo build --release --manifest-path src/rust/Cargo.toml

# Python (reference implementation, also works standalone)
pip install patent-mcp-server
```

### Configure your MCP client

Add to your Claude Desktop, Claude Code, Cursor, or Cline config:

```json
{
  "mcpServers": {
    "patents": {
      "command": "patent-mcp-server"
    }
  }
}
```

That's the whole setup. No API keys needed — 6 of 9 sources work without auth (USPTO, Espacenet, WIPO, IP Australia, CIPO, DuckDuckGo). Add keys later to unlock EPO OPS, BigQuery, and SerpAPI. See [docs/api-keys.md](docs/api-keys.md).

### Use it

```
"Fetch patents US7654321 and EP1234567B1, then summarize the key claims."
```

The agent calls `fetch_patents`, gets back file paths and metadata, reads the Markdown, and does its thing. You don't configure sources. You don't pick formats. You don't manage cache. It just works.

## Tools

### `fetch_patents`

Fetch one or more patents by ID. Accepts any format — bare numbers, jurisdiction-prefixed, with kind codes, even Google Patents URLs.

**Parameters:**
| Name | Type | Required | Description |
|---|---|---|---|
| `patent_ids` | `string[]` | Yes | Patent IDs in any format |
| `force_refresh` | `bool` | No | Bypass cache, re-fetch from sources |
| `formats` | `string[]` | No | Requested formats (default: `["pdf", "txt", "md"]`) |
| `postprocess_query` | `string` | No | Reserved for v2 post-processing |

**Response:**
```json
{
  "results": [
    {
      "canonical_id": "US7654321",
      "success": true,
      "from_cache": false,
      "files": {
        "pdf": ".patents/US7654321/US7654321.pdf",
        "md": ".patents/US7654321/US7654321.md"
      },
      "metadata": {
        "title": "Method and apparatus for ...",
        "inventors": ["Jane Doe", "John Smith"],
        "assignee": "Acme Corp",
        "filing_date": "2005-03-15",
        "publication_date": "2010-02-02",
        "jurisdiction": "US",
        "doc_type": "patent"
      },
      "sources": [
        {"source": "USPTO", "success": true, "elapsed_ms": 1842}
      ]
    }
  ],
  "summary": {
    "total": 1,
    "success": 1,
    "cached": 0,
    "errors": 0,
    "total_duration_ms": 2105
  }
}
```

### `list_cached_patents`

List everything in the local `.patents/` cache. No parameters, returns `{patents: [...], count: N}`.

### `get_patent_metadata`

Cache-only metadata lookup — no network calls, instant response.

**Parameters:** `patent_ids: string[]`
**Returns:** `{results: [{patent_id, canonical_id, metadata}]}`

## Patent ID formats

Don't worry about formatting. The canonicalizer has seen it all:

| Input | Canonical form | Jurisdiction |
|---|---|---|
| `US7654321` | `US7654321` | US |
| `US7654321B2` | `US7654321` | US (with kind code) |
| `US20230001234A1` | `US20230001234` | US application |
| `7654321` | `US7654321` | US (inferred) |
| `EP1234567B1` | `EP1234567` | EP |
| `WO2024/123456` | `WO2024/123456` | International (PCT) |
| `JP2023-123456` | `JP2023123456` | Japan |
| `CN202310001234A` | `CN202310001234` | China |
| `KR10-1234567` | `KR10-1234567` | South Korea |
| `AU2023123456` | `AU2023123456` | Australia |
| `CA3123456` | `CA3123456` | Canada |
| `https://patents.google.com/patent/US7654321/en` | `US7654321` | (extracted) |

Also handles NZ, BR, and IN formats. 22+ patterns total. If it looks like a patent number, we'll figure it out.

## How it works

```
Agent                    MCP Server (Rust)
  │                           │
  ├─ fetch_patents ──────────►│
  │                           ├─ cache lookup (SQLite)
  │                           │   HIT? return immediately
  │                           │   MISS?
  │                           │     ├─ try sources in priority order
  │                           │     │   USPTO → EPO → Espacenet → WIPO → ...
  │                           │     ├─ download PDF (async, concurrent)
  │                           │     ├─ convert PDF → Markdown
  │                           │     │   (pymupdf4llm → pdfplumber → pdftotext → marker)
  │                           │     ├─ extract metadata
  │                           │     └─ store in cache
  │◄──────────────────────────┤
  │   files + metadata        │
```

**Cache:** Two-layer SQLite — local `.patents/index.db` per project + global `~/.local/share/patent-cache/index.db` shared across all projects. Files live in `.patents/{CANONICAL_ID}/`. First fetch takes seconds. Every subsequent fetch is instant — even across different repos.

**PDF → Markdown:** Four converter backends tried in order (pymupdf4llm → pdfplumber → pdftotext → marker). If one fails, the next picks up. Tables extracted and merged. OCR via tesseract for scanned patent figures. The output is clean enough for an LLM to read directly.

**Dual implementation, cross-verified:** Rust is the production server — standalone, async, with native HTTP fetchers and retry logic. Python is the reference implementation. 32 cross-implementation parity tests verify both produce identical results for ID canonicalization, source ordering, converter output, and web search queries. When we say they match, we mean it — it's tested.

## Configuration

All config via `~/.patents.toml` or environment variables (env vars take precedence):

| Env var | Default | Description |
|---|---|---|
| `PATENT_CACHE_DIR` | `.patents/` | Local cache directory |
| `PATENT_CONCURRENCY` | `5` | Max concurrent fetches |
| `PATENT_TIMEOUT` | `30.0` | HTTP timeout (seconds) |
| `PATENT_EPO_KEY` | — | EPO OPS `client_id:client_secret` |
| `PATENT_LENS_KEY` | — | Lens.org API key |
| `PATENT_SERPAPI_KEY` | — | SerpAPI key (web search fallback) |
| `PATENT_BIGQUERY_PROJECT` | — | GCP project for BigQuery source |
| `PATENT_FETCH_ALL_SOURCES` | `false` | Try all sources even after first success |

## Development

### Running tests

```bash
# Rust tests (74 tests, <0.1s)
cargo test --manifest-path src/rust/Cargo.toml

# Fast Python unit tests (<1s, all I/O mocked)
pytest tests/python/ -m "not browser and not integration and not slow"

# Full Python suite (includes fuzz tests via Hypothesis, slow tests)
pytest tests/python/

# Cross-implementation parity — verifies Python == Rust (32 tests)
pytest tests/cross_impl/
```

### Project structure

```
src/
  python/patent_mcp/       # Python reference implementation
    id_canon.py            #   Patent ID canonicalization (22+ formats)
    cache.py               #   Dual-layer SQLite cache
    config.py              #   TOML + env var config loading
    fetchers/
      http.py              #   All HTTP/API source implementations
      web_search.py        #   DuckDuckGo / SerpAPI fallback
      orchestrator.py      #   Priority-ordered source orchestration
    converters/            #   PDF → Markdown (4 backends + OCR)
    server.py              #   MCP server (FastMCP, stdin/stdout)
  rust/                    # Rust production server (standalone)
    src/
      id_canon/            #   Canonicalization (must match Python)
      cache/               #   Two-layer SQLite cache (rusqlite)
      config/              #   TOML + env var config loading
      converters/          #   PDF → Markdown (4 backends + OCR)
      fetchers/
        http/              #   9 native async HTTP sources with retry
        web_search/        #   DuckDuckGo + SerpAPI fallback
        browser.rs         #   Playwright-based Google Patents scraper
        mod.rs             #   Priority-ordered source orchestrator
      server/              #   JSON-RPC 2.0 over stdin/stdout
tests/
  python/                  # 16 test files: unit, integration, fuzz
  cross_impl/              # Python == Rust parity tests
  fixtures/                # Shared test data
docs/
  api-keys.md              # API key setup for each source
```

## What makes this different

Most patent tools give you an API wrapper for one database. This gives your agent the entire global patent system behind a single function call. It handles the authentication, the format differences, the fallbacks, the caching, and the PDF-to-text conversion. Your agent asks for a patent and gets back something it can read. That's it. That's the product.

## License

MIT
