# SCAFFOLDING — mcp-fetch-patents

*Do this FIRST before any node implementation.*
*These tasks have no test-first requirement — they are structure, not behavior.*

---

## S01 — Python package structure
Create directory tree:
```
src/
  python/
    patent_mcp/
      __init__.py          # version = "0.1.0"
      __main__.py          # entry point: python -m patent_mcp → runs server
      config.py            # 06-config
      cache.py             # 02-cache-db (PatentCache)
      db.py                # 02-cache-db (SQLite operations, schema)
      session_cache.py     # 02-cache-db (SessionCache)
      id_canon.py          # 01-id-canon
      server.py            # 05-mcp-protocol
      fetchers/
        __init__.py
        base.py            # BasePatentSource ABC
        orchestrator.py    # 03-source-fetchers
        http.py            # 03a (HTTP sources)
        browser.py         # 03b (Playwright bridge)
        web_search.py      # 03c (fallback)
      converters/
        __init__.py
        pipeline.py        # 04-format-conversion
        pymupdf4llm_conv.py
        pdfplumber_conv.py
        pdftotext_conv.py
        marker_conv.py
      scrapers/
        __init__.py
        playwright_runner.py  # subprocess entry point for Playwright
```

## S02 — pyproject.toml
```toml
[build-system]
requires = ["hatchling"]
build-backend = "hatchling.build"

[project]
name = "patent-mcp-server"
version = "0.1.0"
description = "MCP server for fetching and caching patents"
requires-python = ">=3.11"
dependencies = [
    "mcp[cli]>=1.3.0",
    "httpx[http2]>=0.27",
    "h2>=4.1",
    "tenacity>=8.2",
    "pymupdf4llm>=0.0.17",
    "PyMuPDF>=1.24",
    "pdfplumber>=0.11",
    "beautifulsoup4>=4.12",
    "lxml>=5.0",
    "tomli>=2.0; python_version < '3.11'",
]

[project.optional-dependencies]
browser = ["playwright>=1.44", "playwright-stealth>=1.0"]
marker = ["marker-pdf>=0.3"]
bigquery = ["google-cloud-bigquery>=3.13", "google-auth>=2.23"]
dev = [
    "pytest>=8.0",
    "pytest-asyncio>=0.23",
    "respx>=0.21",
    "hypothesis>=6.100",
    "reportlab>=4.0",
    "starlette>=0.37",
    "uvicorn>=0.29",
]

[project.scripts]
patent-mcp-server = "patent_mcp.__main__:main"

[tool.pytest.ini_options]
asyncio_mode = "auto"
testpaths = ["tests/python"]
markers = [
    "browser: requires PATENT_BROWSER_TESTS=1 (Playwright)",
    "integration: hits real network",
    "slow: takes >1s",
]
```

## S03 — Rust project structure
```
src/
  rust/
    Cargo.toml
    src/
      main.rs              # binary entry point
      lib.rs               # library root
      config/
        mod.rs             # 06-config
      id_canon/
        mod.rs             # 01-id-canon
      cache/
        mod.rs             # 02-cache-db
        session.rs         # SessionCache
      fetchers/
        mod.rs             # 03-source-fetchers orchestrator
        http/
          mod.rs           # 03a
          ppubs.rs
          epo_ops.rs
          country/         # country-specific sources
        browser/
          mod.rs           # 03b
        web_search/
          mod.rs           # 03c
      converters/
        mod.rs             # 04-format-conversion
      server/
        mod.rs             # 05-mcp-protocol
        mcp_protocol.rs    # JSON-RPC over stdin
```

## S04 — Cargo.toml
```toml
[package]
name = "patent-mcp-server"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "patent-mcp-server"
path = "src/main.rs"

[dependencies]
tokio = { version = "1.38", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
reqwest = { version = "0.12", features = ["json", "stream"] }
rusqlite = { version = "0.31", features = ["bundled"] }
toml = "0.8"
dirs = "5.0"
regex = "1.10"
clap = { version = "4.5", features = ["derive"] }
anyhow = "1.0"
thiserror = "1.0"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
chrono = { version = "0.4", features = ["serde"] }

[dev-dependencies]
tempfile = "3.10"
tokio-test = "0.4"
```

## S05 — Root files
- `.gitignore`: `.patents/`, `target/`, `__pycache__/`, `*.pyc`, `.env`, `*.db`, `node_modules/`
- `.patents.toml.example`: copy of the full TOML schema from 06-config SPEC with all values commented out
- `docs/api-keys.md`: one section per keyed source (see 03a SPEC for content outline)
- `README.md`: skeleton with: what it is, quick start (Python + Rust), config, sources table

## S06 — Test directory structure
```
tests/
  python/
    conftest.py            # 07-test-infra T10
    utils.py               # find_free_port, etc.
    test_id_canon.py       # 01
    test_config.py         # 06
    test_cache.py          # 02
    test_converters.py     # 04
    test_http_sources.py   # 03a
    test_browser_sources.py # 03b
    test_web_search.py     # 03c
    test_orchestrator.py   # 03
    test_mcp_server.py     # 05
  rust/                    # cargo test --test integration_tests
    integration_tests.rs
  cross_impl/
    parity.py              # assert_parity utility
    db_parity.py           # SQLite dump comparison
    test_id_canon_parity.py
    test_cache_parity.py
    test_orchestrator_parity.py
    test_mcp_parity.py
  mock_server/
    server.py              # 07-test-infra T09
    routes.json            # 07-test-infra T08
  fixtures/
    us/US7654321/...       # 07-test-infra T02
    ep/EP1234567B1/...
    wo/WO2024123456/...
    jp/JP2023123456/...
    invalid/INVALID999/...
    partial/US9999999/...
    browser/
      google_patents/...   # 07-test-infra T07b
    stub.pdf               # 07-test-infra T15
    generate_stub_pdf.py
```

## S07 — CI configuration (GitHub Actions)
`.github/workflows/test.yml`:
```yaml
jobs:
  python-tests:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: astral-sh/setup-uv@v3
      - run: uv sync --all-extras
      - run: pytest tests/python -m "not browser and not integration" --timeout=5
  rust-tests:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test --manifest-path src/rust/Cargo.toml
  parity-tests:
    needs: [python-tests, rust-tests]
    runs-on: ubuntu-latest
    steps:
      - run: cargo build --manifest-path src/rust/Cargo.toml --release
      - run: pytest tests/cross_impl --timeout=30
```
