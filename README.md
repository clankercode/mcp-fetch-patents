# mcp-fetch-patents

**Give any AI agent instant access to the global patent corpus.**

One MCP tool call. Any patent ID format. PDF + Markdown + structured metadata back in seconds — or instantly from cache. Your agent never waits for the same patent twice.

```
"Fetch US7654321, EP1234567B1, and WO2024/123456"
  → 3 PDFs, 3 Markdown files, 3 metadata objects, all cached for next time
```

---

## Why this exists

Patent data is scattered across dozens of national databases, each with its own API, format, and quirks. An agent doing patent analysis shouldn't have to figure out that a US patent lives on USPTO PPUBS, a European one needs EPO OPS OAuth2, and a PCT application requires scraping WIPO PatentScope. This server handles all of that behind a single interface.

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

Sources are tried in priority order. First success wins (unless you set `PATENT_FETCH_ALL_SOURCES=true`). If all structured sources fail, web search finds the PDF anyway.

## Quick start

### Install

```bash
# Python (reference implementation)
pip install patent-mcp-server

# Rust (production — thin cache proxy that delegates fetching to Python)
cargo install patent-mcp-server
# or build from source:
cargo build --release --manifest-path src/rust/Cargo.toml
```

### Configure your MCP client

**Claude Desktop / Claude Code / Cursor / Cline:**

```json
{
  "mcpServers": {
    "patents": {
      "command": "patent-mcp-server",
      "env": {
        "PATENT_EPO_KEY": "your_client_id:your_client_secret"
      }
    }
  }
}
```

No API keys? That's fine — USPTO, Espacenet, WIPO, IP Australia, CIPO, and DuckDuckGo all work without auth. Add keys later to unlock EPO OPS, BigQuery, and SerpAPI. See [docs/api-keys.md](docs/api-keys.md) for setup.

### Use it

Ask your agent to fetch patents. That's it.

```
"Fetch patents US7654321 and EP1234567B1, then summarize the key claims."
```

The agent calls `fetch_patents`, gets back file paths and metadata, reads the Markdown, and does its thing.

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

Throw whatever you have at it. The canonicalizer handles all of these:

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

Also supports NZ, BR, and IN formats.

## How it works

```
Agent                MCP Server (Rust)              Python Backend
  │                       │                              │
  ├─ fetch_patents ──────►│                              │
  │                       ├─ cache lookup (SQLite) ──►   │
  │                       │   HIT? return immediately    │
  │                       │   MISS? ─────────────────────►│
  │                       │                              ├─ try sources in priority order
  │                       │                              │   USPTO → EPO → Espacenet → ...
  │                       │                              ├─ download PDF
  │                       │                              ├─ convert PDF → Markdown
  │                       │                              │   (pymupdf4llm → pdfplumber → pdftotext)
  │                       │                              ├─ extract metadata
  │                       │◄─────────────────────────────┤
  │                       ├─ store in cache              │
  │◄──────────────────────┤                              │
  │   files + metadata    │                              │
```

**Cache architecture:** Two-layer SQLite — local `.patents/index.db` per repo + global `~/.local/share/patent-cache/index.db` (XDG). Files live in `.patents/{CANONICAL_ID}/`. Second fetch of the same patent is instant.

**PDF → Markdown pipeline:** Four converter backends tried in order (pymupdf4llm → pdfplumber → pdftotext → marker). Tables extracted and merged. OCR via tesseract for scanned figures.

**Dual implementation:** Python is the reference implementation with all source fetchers, converters, and the full orchestrator. Rust is the production server — a thin, fast cache proxy that delegates to Python on cache miss. Cross-implementation parity tests ensure they stay in sync.

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
# Fast Python unit tests (<1s, all I/O mocked)
pytest tests/python/ -m "not browser and not integration and not slow"

# Full suite (includes fuzz tests via Hypothesis, slow tests)
pytest tests/python/

# Rust tests
cargo test --manifest-path src/rust/Cargo.toml

# Cross-implementation parity (Python == Rust, requires built Rust binary)
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
  rust/                    # Rust production server
    src/
      id_canon/            #   Canonicalization (must match Python)
      cache/               #   SQLite cache (rusqlite)
      fetchers/            #   Cache-first, delegates to Python on miss
      server/              #   JSON-RPC 2.0 over stdin/stdout
tests/
  python/                  # 16 test files: unit, integration, fuzz
  cross_impl/              # Python == Rust parity tests
  fixtures/                # Shared test data
docs/
  api-keys.md              # API key setup for each source
```

## License

MIT
