# API Keys — mcp-fetch-patents

All API keys are optional. The server degrades gracefully: if a key is missing, that source is skipped or uses its no-auth path.

For the installed Rust server, credentials can live in `~/.patents.toml`, normal environment variables, or `~/.patents-mcp.env` autoloaded by the server. A repo-local `.env` only helps when the server is launched from that checkout.

---

## EPO OPS (European Patent Office Open Patent Services)

**Coverage:** EP, WO, and ~100 other offices via EPO exchange data.  
**Free tier:** 4GB/day bandwidth, no registration required for basic access.  
**Registration:** [ops.epo.org](https://ops.epo.org/3.2/rest-services) — create a free account.

```bash
# ~/.patents.toml
[sources.epo_ops]
client_id = "your_client_id"
client_secret = "your_client_secret"

# OR via environment variable (format: "client_id:client_secret")
export PATENT_EPO_KEY="your_client_id:your_client_secret"
```

---

## Lens.org API

**Coverage:** 100M+ patents from 95+ jurisdictions; full text search.  
**Free tier:** Available for non-commercial use.  
**Registration:** [lens.org/lens/user/subscriptions](https://www.lens.org/lens/user/subscriptions)

```bash
export PATENT_LENS_KEY="your_lens_api_key"
```

---

## SerpAPI (Web Search Fallback)

**Coverage:** Last-resort web search when all structured sources fail.  
**Note:** The server falls back to DuckDuckGo's free API if no SerpAPI key is configured. SerpAPI provides higher reliability.  
**Registration:** [serpapi.com](https://serpapi.com)

```bash
export PATENT_SERPAPI_KEY="your_serpapi_key"
```

---

## Google BigQuery (Google Patents Public Data)

**Coverage:** Full-text patents from USPTO, EPO, and other offices.  
**Cost:** Free within Google Cloud free tier limits; BigQuery pricing for heavy usage.  
**Setup:**
1. Create a [Google Cloud project](https://console.cloud.google.com)
2. Enable the BigQuery API
3. Create a service account with `BigQuery Data Viewer` + `BigQuery Job User` roles
4. Download the service account key JSON

```bash
export GOOGLE_APPLICATION_CREDENTIALS="/path/to/service-account-key.json"
# OR
export PATENT_BIGQUERY_PROJECT="your-gcp-project-id"
```

---

## Sources That Need No API Key

| Source | Coverage | Notes |
|--------|----------|-------|
| USPTO PPUBS | US granted patents + applications | Session-based, no registration |
| Espacenet | EP + all EPO member states | Scraped, no auth |
| WIPO PatentScope | WO (PCT) patents | Scraped, no auth |
| IP Australia | AU patents | Scraped, no auth |
| CIPO (Canada) | CA patents | Scraped, no auth |
| DuckDuckGo | Web search fallback | Free, rate-limited |

---

## Configuration File

Create `~/.patents.toml` (or set `PATENT_CONFIG_FILE` to a custom path):

```toml
[cache]
local_dir = "~/.local/share/patent-cache/patents"  # XDG default
global_db = "~/.local/share/patent-cache/index.db"  # XDG default

[sources]
priority = ["USPTO", "EPO_OPS", "BigQuery", "Espacenet", "WIPO_Scrape", "IP_Australia", "CIPO", "web_search"]
fetch_all_sources = false     # If true, fan-out to all sources concurrently
concurrency = 5
timeout = 30.0

[sources.epo_ops]
client_id = ""
client_secret = ""

[converters]
order = ["pymupdf4llm", "pdfplumber", "pdftotext"]
disabled = []

[search]
backend_default = "browser"          # "browser" | "serpapi" | "auto"
browser_headless = true
browser_idle_timeout = 1800          # 30 minutes
browser_max_pages = 3
enrich_top_n = 5
```

All values can be overridden with environment variables. Environment variables take precedence over the config file.

The Rust server autoloads `~/.patents-mcp.env` first, then `.env` in the current working directory, then reads `~/.patents.toml`, and finally applies explicit environment variables.

To verify the Rust server is actually picking up the config you expect, smoke-test it directly over stdio JSON-RPC instead of relying on your editor's MCP reload behavior:

```bash
# Dev server from this checkout
just mcp-smoke-rust

# Installed binary from ~/.cargo/bin
just mcp-smoke-rust-installed
```

`~/.patents-mcp.env` can contain lines like:

```dotenv
PATENT_SERPAPI_KEY=...
PATENT_LENS_KEY=...
PATENT_BIGQUERY_PROJECT=...
GOOGLE_APPLICATION_CREDENTIALS=/path/to/service-account-key.json
```

### Full Environment Variable Reference

| Variable | Default | Description |
|----------|---------|-------------|
| `PATENT_CACHE_DIR` | `~/.local/share/patent-cache/patents` | Cache directory for patent files |
| `PATENT_GLOBAL_DB` | `~/.local/share/patent-cache/index.db` | Global SQLite index |
| `PATENT_EPO_KEY` | — | EPO OPS credentials (`client_id:client_secret`) |
| `PATENT_LENS_KEY` | — | Lens.org API key |
| `PATENT_SERPAPI_KEY` | — | SerpAPI key for web search |
| `PATENT_BING_KEY` | — | Bing Search API key |
| `PATENT_CONCURRENCY` | `5` | Max concurrent patent fetches |
| `PATENT_TIMEOUT` | `30.0` | HTTP timeout in seconds |
| `PATENT_FETCH_ALL_SOURCES` | `false` | Fan-out to all sources concurrently |
| `PATENT_DISABLE_MARKER` | `false` | Disable marker PDF converter |
| `PATENT_BIGQUERY_PROJECT` | — | GCP project for BigQuery |
| `PATENT_ACTIVITY_JOURNAL` | `.patent-activity.jsonl` | Per-repo activity journal (empty = disabled) |
| `PATENT_SEARCH_BACKEND_DEFAULT` | `browser` | Default search backend (`browser`, `serpapi`, `auto`) |
| `PATENT_SEARCH_BROWSER_HEADLESS` | `true` | Run Playwright in headless mode |
| `PATENT_SEARCH_BROWSER_IDLE_TIMEOUT` | `1800` | Browser idle timeout in seconds |
| `PATENT_SEARCH_BROWSER_MAX_PAGES` | `3` | Max Google Patents result pages per query |
| `PATENT_SEARCH_BROWSER_PROFILES_DIR` | XDG default | Browser profile storage directory |
| `PATENT_SEARCH_ENRICH_TOP_N` | `5` | Enrich top N search results with full metadata |
| `GOOGLE_APPLICATION_CREDENTIALS` | — | Path to GCP service account JSON |
