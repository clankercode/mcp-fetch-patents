# Patent MCP Server — dev scripts
# Run `just` to see all available recipes.

# Default: list recipes
default:
    @just --list

# ── Install ──────────────────────────────────────────────────────────────────

# Install Python package with dev dependencies (editable, via pip)
install-py:
    pip install -e ".[dev]"

# Install Python package with dev dependencies (editable, via uv)
install-uv:
    uv pip install -e ".[dev]"

# Install Rust binary (via cargo install from local path)
install-rs:
    cargo install --path src/rust

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
    cargo test --manifest-path src/rust/Cargo.toml

# Build Rust binary (debug)
build-rust:
    cargo build --manifest-path src/rust/Cargo.toml

# Build Rust binary (release)
build-rust-release:
    cargo build --release --manifest-path src/rust/Cargo.toml

# Check Rust without building binary
check-rust:
    cargo check --manifest-path src/rust/Cargo.toml

# Run clippy lints on Rust code
lint-rust:
    cargo clippy --manifest-path src/rust/Cargo.toml -- -D warnings

# ── Combined ──────────────────────────────────────────────────────────────────

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

# Start the patent-search MCP server (used by OpenCode agent)
serve-search:
    python -m patent_mcp.search

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
