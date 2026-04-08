# SPEC — 06-config: Configuration System

## Responsibility
Provide a unified configuration layer: load from environment variables and TOML config file, merge with defaults, and expose a typed config object to all other components. Also produce documentation for API keys.

## Config Precedence (highest to lowest)
1. Environment variables
2. `~/.patents.toml` (user config file)
3. `.patents.toml` in working directory (project config)
4. Compiled-in defaults

## Environment Variables
```
# Paths
PATENT_CACHE_DIR          # local cache dir (default: .patents/ in cwd)
PATENT_GLOBAL_DB          # global index DB path (default: $XDG_DATA_HOME/patent-cache/index.db)

# API Keys
PATENT_EPO_KEY            # EPO OPS client credentials (format: "client_id:client_secret")
PATENT_LENS_KEY           # Lens.org API key
PATENT_SERPAPI_KEY        # SerpAPI key (web search fallback)
PATENT_BING_KEY           # Bing Web Search API key
PATENT_WIPO_KEY           # WIPO PatentScope API key

# Behavior
PATENT_CONCURRENCY        # max parallel fetches (default: 10)
PATENT_TIMEOUT            # per-request HTTP timeout in seconds (default: 30)
PATENT_FETCH_ALL_SOURCES  # 1/0 — fetch from all sources for completeness (default: 1)
PATENT_DISABLE_MARKER     # 1/0 — disable marker converter (default: 1 in test env)
PATENT_BROWSER_TESTS      # 1/0 — enable Playwright tests (default: 0)
PATENT_LOG_LEVEL          # debug|info|warn|error (default: info)
PATENT_AGENT_CMD          # CLI agent for postprocess_query (default: claude)
```

## TOML Config Schema
`~/.patents.toml` or `.patents.toml`:
```toml
[cache]
local_dir = ".patents"                          # relative to cwd
global_db = "~/.local/share/patent-cache/index.db"

[sources]
# Priority order for fetching; omit a source to disable it
priority = ["USPTO", "EPO_OPS", "WIPO", "Espacenet", "Lens", "JPO", 
            "CNIPA", "KIPRIS", "IP_Australia", "CIPO", "IPONZ", 
            "Google_Patents", "BRPTO", "GCC", "web_search"]
concurrency = 10
fetch_all_sources = true
timeout_seconds = 30

[sources.epo_ops]
client_id = ""
client_secret = ""

[sources.lens]
api_key = ""

[sources.serpapi]
api_key = ""

[sources.bing]
api_key = ""

[converters]
pdf_to_markdown_order = ["pymupdf4llm", "pdftotext", "marker"]
disable = ["marker"]

[agent]
command = "claude"              # CLI for postprocess_query

[logging]
level = "info"
```

## Config Object (typed)
Both Python and Rust expose identical typed config:
```python
@dataclass
class PatentConfig:
    cache_local_dir: Path
    cache_global_db: Path
    source_priority: list[str]
    concurrency: int
    fetch_all_sources: bool
    timeout: float
    converters_order: list[str]
    converters_disabled: list[str]
    epo_client_id: str | None
    epo_client_secret: str | None
    lens_api_key: str | None
    serpapi_key: str | None
    bing_key: str | None
    agent_command: str
    log_level: str
```

## API Key Documentation
Generated file `docs/api-keys.md`:
```markdown
# API Keys for mcp-fetch-patents

## EPO Open Patent Services (OPS)
- Register at: https://developers.epo.org/
- Free tier: 4GB/week quota
- What you get: client_id + client_secret
- Set: PATENT_EPO_KEY="client_id:client_secret" or [sources.epo_ops] in ~/.patents.toml
...
```
One section per keyed source, with registration URL, free tier details, and config instructions.

## XDG Path Resolution
```python
def xdg_data_home() -> Path:
    return Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local" / "share"))

def default_global_db() -> Path:
    return xdg_data_home() / "patent-cache" / "index.db"
```
Identical logic in Rust (`dirs` crate or manual XDG).

## Dependencies
None (no other nodes depend on config being built first, but config is a hard prerequisite for all)

## Test Surface
- Unit: env var overrides TOML config file
- Unit: TOML config file overrides defaults
- Unit: missing API key → None (not an error at config load time)
- Unit: XDG path resolution on Linux
- Cross-impl: Python config object == Rust config object for same inputs
