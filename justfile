# Patent MCP Server — dev scripts
# Run `just` to see all available recipes.

# Default: list recipes
default:
    @just --list

# ── Install ──────────────────────────────────────────────────────────────────

# Default install (Rust binary)
install: install-rs

# Install Python package with dev dependencies (editable, via pip)
install-py:
    pip install -e ".[dev]"

# Install Python package with dev dependencies (editable, via uv)
install-uv:
    uv pip install -e ".[dev]"

# Install Rust binary from the local crate, replacing any existing install
install-rs:
    CC=gcc cargo install --path src/rust --force

# Install Python with all optional extras (via pip)
install-all:
    pip install -e ".[dev,pymupdf4llm,browser,bigquery]"

# Install Playwright browser drivers (after install-all)
playwright-install:
    playwright install chromium

# ── Python tests ─────────────────────────────────────────────────────────────

# Run fast Python unit tests (skip slow/browser/integration)
test:
    pytest tests/python -m 'not browser and not integration and not slow' --tb=short -q

# Run fast tests with verbose output
test-v:
    pytest tests/python -m 'not browser and not integration and not slow' -v

# Run ALL Python tests including slow ones
test-all:
    pytest tests/python -q --tb=short

# Run only slow tests
test-slow:
    pytest tests/python -m slow -q --tb=short

# Run cross-implementation parity tests
test-cross:
    pytest tests/cross_impl/ -v --tb=short

# Run a specific test file or pattern (e.g. just test-filter test_search)
test-filter PATTERN:
    pytest tests/python -k "{{PATTERN}}" -v --tb=short

# ── Rust tests ────────────────────────────────────────────────────────────────

# Run Rust tests
test-rust:
    CC=gcc cargo test --manifest-path src/rust/Cargo.toml

# Build Rust binary (debug)
build-rust:
    CC=gcc cargo build --manifest-path src/rust/Cargo.toml

# Build Rust binary (release)
build-rust-release:
    CC=gcc cargo build --release --manifest-path src/rust/Cargo.toml

# Check Rust without building binary
check-rust:
    CC=gcc cargo check --manifest-path src/rust/Cargo.toml

# Run clippy lints on Rust code
lint-rust:
    CC=gcc cargo clippy --manifest-path src/rust/Cargo.toml -- -D warnings

# Format Rust code
fmt-rust:
    cargo fmt --manifest-path src/rust/Cargo.toml

# ── Combined ──────────────────────────────────────────────────────────────────

# Run all lints (Rust + Python)
lint: lint-rust
    ruff check src/python/

# Run all tests (Python fast + Rust)
ci:
    @just test
    @just test-rust

# Run everything including slow tests
ci-full:
    @just test-all
    @just test-rust
    @just test-cross

# ── MCP server ────────────────────────────────────────────────────────────────

# Start the Python MCP patent-fetch server (stdio mode for MCP clients)
serve:
    python -m patent_mcp

# Start the Rust MCP server (19 tools: fetch + search + status)
serve-rust:
    CC=gcc cargo run --manifest-path src/rust/Cargo.toml --bin patent-mcp-server

# Start the patent-search MCP server (used by OpenCode agent)
serve-search:
    python -m patent_mcp.search

# Smoke-test the Rust MCP server directly over stdio JSON-RPC
mcp-smoke-rust PATENT_ID='US10000000B2':
    printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"fetch_patents","arguments":{"patent_ids":["{{PATENT_ID}}"],"force_refresh":true}}}' | CC=gcc cargo run --quiet --manifest-path src/rust/Cargo.toml --bin patent-mcp-server

# Smoke-test the installed Rust MCP binary directly over stdio JSON-RPC
mcp-smoke-rust-installed PATENT_ID='US10000000B2':
    printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"fetch_patents","arguments":{"patent_ids":["{{PATENT_ID}}"],"force_refresh":true}}}' | /home/xertrov/.cargo/bin/patent-mcp-server

# Run manual E2E test suite (31 tests against all tools)
test-e2e:
    python3 run_manual_e2e.py

# ── Search ────────────────────────────────────────────────────────────────────

# Start the patent-search MCP server with browser backend available
serve-search-browser:
    pip install -e ".[browser]" 2>/dev/null; python -m patent_mcp.search

# Launch a headed browser for Google login (profile persistence)
search-login PROFILE='default':
    python -c "from patent_mcp.search.profile_manager import ProfileManager; from patent_mcp.search.browser_manager import BrowserManager; pm = ProfileManager(); bm = BrowserManager(pm, '{{PROFILE}}', headless=False); page = bm.get_page(); page.goto('https://patents.google.com/'); input('Press Enter to close browser...'); bm.close()"

# ── Utilities ─────────────────────────────────────────────────────────────────

# Show pytest test count breakdown by marker
test-count:
    @echo "=== Fast tests ==="
    @pytest tests/python -m 'not browser and not integration and not slow' --collect-only -q 2>/dev/null | tail -1
    @echo "=== Slow tests ==="
    @pytest tests/python -m slow --collect-only -q 2>/dev/null | tail -1
    @echo "=== All Python ==="
    @pytest tests/python --collect-only -q 2>/dev/null | tail -1

# Clean Python build artifacts
clean:
    find . -type d -name __pycache__ -exec rm -rf {} + 2>/dev/null || true
    find . -type d -name "*.egg-info" -exec rm -rf {} + 2>/dev/null || true
    find . -type d -name .pytest_cache -exec rm -rf {} + 2>/dev/null || true
    find . -name "*.pyc" -delete 2>/dev/null || true

# Clean Rust build artifacts
clean-rust:
    cargo clean --manifest-path src/rust/Cargo.toml

# Clean everything
clean-all:
    @just clean
    @just clean-rust
