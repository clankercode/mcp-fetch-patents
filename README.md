# mcp-fetch-patents

A single-responsibility MCP server that fetches patents by ID, caches them locally and globally, and returns file paths + metadata. Agents never wait for the same patent twice.

## Features

- **Fetch by ID** — any patent ID format: US, EP, WO, JP, CN, KR, AU, CA, NZ, BR, IN, and more
- **Cache-first** — SQLite cache (per-repo `.patents/` + global XDG index); cache hit is instant
- **Batch fetching** — pass 100+ patent IDs in a single MCP tool call
- **Multiple sources** — USPTO PPUBS, EPO OPS, Espacenet scraping, WIPO, CIPO, IP Australia, web search fallback
- **Format conversion** — PDF → Markdown (pymupdf4llm → pdfplumber → pdftotext); OCR figures via tesseract
- **Dual implementation** — Python reference + Rust production mirror; parity validated by cross-impl tests
- **MCP transport** — stdin/stdout only (v1); JSON-RPC 2.0

## Quick Start

### Python (reference implementation)

```bash
pip install patent-mcp-server
# Add to Claude / Cursor / Cline MCP config:
# command: python3 -m patent_mcp
```

### Rust (production)

```bash
cargo install patent-mcp-server
# or build from source:
cargo build --release --manifest-path src/rust/Cargo.toml
```

### MCP Configuration (Claude Desktop / claude.json)

```json
{
  "mcpServers": {
    "patents": {
      "command": "python3",
      "args": ["-m", "patent_mcp"],
      "env": {
        "PATENT_EPO_KEY": "your_client_id:your_client_secret"
      }
    }
  }
}
```

## Tools

### `fetch_patents`

Fetch one or more patents by ID. Returns file paths and metadata.

```json
{
  "patent_ids": ["US7654321", "EP1234567B1", "WO2024123456"],
  "force_refresh": false
}
```

Returns:
```json
{
  "results": [
    {
      "canonical_id": "US7654321",
      "success": true,
      "from_cache": false,
      "files": {
        "pdf": "/path/.patents/US7654321/US7654321.pdf",
        "md": "/path/.patents/US7654321/US7654321.md"
      },
      "metadata": {
        "title": "...",
        "inventors": ["..."],
        "filing_date": "..."
      }
    }
  ],
  "summary": {"total": 1, "success": 1, "cached": 0, "errors": 0}
}
```

### `list_cached_patents`

List all patents in the local `.patents/` cache.

### `get_patent_metadata`

Return cached metadata for patents (no network call).

## Configuration

Config is loaded from `~/.patents.toml` (overridden by env vars):

| Env var | Default | Description |
|---|---|---|
| `PATENT_CACHE_DIR` | `.patents/` | Local cache directory |
| `PATENT_CONCURRENCY` | `5` | Max concurrent fetches |
| `PATENT_TIMEOUT_SECS` | `30.0` | HTTP timeout |
| `PATENT_EPO_KEY` | — | `client_id:client_secret` for EPO OPS |
| `PATENT_LENS_KEY` | — | Lens.org API key |
| `PATENT_SERPAPI_KEY` | — | SerpAPI key (web search fallback) |
| `PATENT_FETCH_ALL_SOURCES` | `false` | Try all sources even after success |

See [docs/api-keys.md](docs/api-keys.md) for how to obtain API keys for each source.

## Supported Patent ID Formats

| Format | Example | Jurisdiction |
|---|---|---|
| US granted | `US7654321` | United States |
| US application | `US20230001234A1` | United States |
| EP | `EP1234567B1` | European Patent Office |
| WO (WIPO) | `WO2024/123456` | International |
| JP | `JP2023-123456` | Japan |
| CN | `CN202310001234A` | China |
| KR | `KR10-1234567` | South Korea |
| AU | `AU2023123456` | Australia |
| CA | `CA3123456` | Canada |
| NZ | `NZ123456` | New Zealand |
| Google Patents URL | `https://patents.google.com/patent/US7654321/en` | Any |

## Development

### Running tests

```bash
# Python unit tests (fast, all I/O mocked)
pytest tests/python/ -m "not browser and not integration and not slow"

# All Python tests including fuzz + slow
pytest tests/python/

# Rust unit tests
cargo test --manifest-path src/rust/Cargo.toml

# Cross-implementation parity tests (requires Rust binary built)
pytest tests/cross_impl/
```

### Project structure

```
src/
  python/patent_mcp/    # Python reference implementation
    id_canon.py         # Patent ID canonicalization
    cache.py            # SQLite cache (local + global XDG)
    config.py           # Configuration loading
    fetchers/           # Patent source fetchers
      http.py           # PPUBS, EPO OPS, Espacenet, WIPO, ...
      web_search.py     # Web search fallback
      orchestrator.py   # Source priority orchestration
    converters/         # PDF → Markdown pipeline
    server.py           # MCP server (FastMCP)
  rust/                 # Rust production mirror
    src/
      id_canon/         # Patent ID canonicalization
      cache/            # SQLite cache
      config/           # Configuration
      fetchers/         # Orchestrator (delegates to Python on miss)
      converters/       # Delegates to Python subprocess
      server/           # MCP JSON-RPC server
tests/
  python/               # Python unit + integration tests
  rust/                 # Rust integration tests
  cross_impl/           # Python == Rust parity tests
  fixtures/             # Shared test data
docs/
  api-keys.md           # API key setup guide
  ultra-plans/          # Implementation plan tree
```

## License

MIT
