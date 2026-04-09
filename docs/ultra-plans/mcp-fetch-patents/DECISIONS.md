# DECISIONS — mcp-fetch-patents

ADR-style log of architectural decisions.

---

## ADR-001: Dual Python + Rust Implementation
**Status:** Accepted
**Date:** 2026-04-07
**Context:** User wants both fast-iteration (Python) and production-quality (Rust) implementations.
**Decision:** Implement Python first as reference; then Rust mirroring Python behavior exactly. Cross-validate via shared mock server test harness.
**Consequences:** ~2x implementation effort. Benefit: production-grade binary with no Python runtime dependency; parity validation catches bugs in both; Python enables fast iteration on new sources.

---

## ADR-002: XDG-Compliant Cache Paths
**Status:** Accepted
**Date:** 2026-04-07
**Context:** User confirmed XDG strict.
**Decision:** Local cache = `.patents/` in working dir. Global index = `$XDG_DATA_HOME/patent-cache/index.db` (default `~/.local/share/patent-cache/index.db`).
**Consequences:** Follows Linux standards. Works on macOS too (XDG_DATA_HOME can be set). Windows not a primary target.

---

## ADR-003: Cache-First, Completeness-Driven Fetching
**Status:** Accepted
**Date:** 2026-04-07
**Context:** Primary purpose is caching; secondary purpose is completeness.
**Decision:** Always check global index before any network call. When fetching, try ALL sources (not stop at first success) to maximize format coverage. Cache every artifact found.
**Consequences:** First fetch may be slower (exhausts all sources). Subsequent fetches are instant. Agent gets maximum format options.

---

## ADR-004: Global Index via Self-Registering Caches
**Status:** Accepted
**Date:** 2026-04-07
**Context:** Need to discover "other" patent caches on the system.
**Decision:** Each `.patents/` dir self-registers in global SQLite DB when first created. No filesystem scanning. Cross-machine caches not supported (local only).
**Consequences:** Simple; no scan overhead. If a cache is created by a different tool, it won't be in the registry unless it uses this library. Document registration API for third-party tools.

---

## ADR-005: Playwright via Python Subprocess from Rust
**Status:** Accepted
**Date:** 2026-04-07
**Context:** Rust doesn't have mature Playwright bindings. Python does.
**Decision:** Playwright scraping always runs as a Python subprocess. Rust calls `python -m patent_mcp.scrapers.playwright_runner` and reads JSON from stdout.
**Consequences:** Python runtime required even when using Rust binary. Acceptable: Python is a soft dependency (graceful degradation if absent, just skip browser sources).

---

## ADR-006: Configurable Converter Priority
**Status:** Accepted
**Date:** 2026-04-07
**Context:** User wants multiple PDF→Markdown converters with easy priority changes.
**Decision:** Default order: pymupdf4llm → pdftotext → marker. Configurable via `[converters] pdf_to_markdown_order` in TOML or env. Marker disabled by default in test environment.
**Consequences:** Easy to swap converters. Default is fast + good quality. Marker available for highest quality when needed.

---

## ADR-007: Legal Status Deferred to v2
**Status:** Accepted
**Date:** 2026-04-07
**Context:** User confirmed v2.
**Decision:** DB schema includes legal_status and status_fetched_at fields (nullable). No fetching logic in v1.
**Consequences:** Clean schema from day one. v2 can add fetching without schema migration.

---

## ADR-008: Localhost HTTP Transport Added Alongside Stdio
**Status:** Accepted
**Date:** 2026-04-07
**Context:** Some MCP clients launch stdio subprocesses inside restricted sandboxes, which breaks cache-backed startup and browser-profile writes.
**Decision:** Keep stdio as the default launch mode, and add localhost-only HTTP on the default URL `http://127.0.0.1:38473/mcp` as an explicit `serve-http` mode in Rust and Python.
**Consequences:** Existing stdio integrations keep working. Remote clients can connect to a user-started local server process outside their sandbox. The shared commitment is the default localhost URL, not wire-level Rust/Python HTTP parity. Public network exposure remains out of scope.

---

## ADR-009: postprocess_query — Accept but Ignore in v1
**Status:** Accepted
**Date:** 2026-04-07
**Context:** User confirmed v1 no-op; future hook for `claude` CLI.
**Decision:** Parameter accepted in v1, logged as "not yet implemented", ignored. Default agent command = `claude`.
**Consequences:** Schema is established; v2 implementation is a drop-in addition.

---

## ADR-011: BigQuery as Optional Tier-1 Multi-National Source
**Status:** Accepted
**Date:** 2026-04-07
**Context:** Research found that `patents-public-data.patents` BigQuery dataset covers 17+ countries including US full text, within a 1TB/month free tier. No other single source covers this many jurisdictions for free.
**Decision:** Implement BigQuery as an optional Tier 1 source. Graceful degradation if not configured (skip, log warning). Setup requires GCP service account — document clearly in `docs/api-keys.md`.
**Consequences:** Agents with GCP access get broad multi-national coverage from a single integration. Agents without can still use EPO OPS + country scrapers.

---

## ADR-012: PatentsView — Do Not Implement
**Status:** Accepted
**Date:** 2026-04-07
**Context:** PatentsView API was shut down March 20, 2026 — confirmed by research. Data migrated to USPTO ODP bulk datasets.
**Decision:** Do not implement a PatentsView client. If included in config priority list, return a graceful message pointing to ODP.

---

## ADR-013: WIPO PatentScope — Scraping Only (API Paywalled)
**Status:** Accepted
**Date:** 2026-04-07
**Context:** WIPO PatentScope API costs 600 CHF/year — effectively paywalled for individual/development use.
**Decision:** Implement WIPO PatentScope as a web scraping target only (httpx + Playwright fallback). No API integration.

---

## ADR-014: Lens.org — Scraping Only (API Requires Manual Approval + Paid)
**Status:** Accepted
**Date:** 2026-04-07
**Context:** Lens.org API requires manual approval and is a paid service after 14-day trial. Not suitable for free/automated use.
**Decision:** Implement Lens.org as a web scraping target only for v1.

---

## ADR-010: Broad Jurisdiction Coverage (All Major Economic Zones)
**Status:** Accepted
**Date:** 2026-04-07
**Context:** User wants US, EP, WO, and all major economic zones.
**Decision:** Tier 1 (full regex + tests): US, EP, WO, JP, CN, KR, AU, CA, NZ, BR, IN. Tier 2 (best-effort): DE, FR, GB, IT, ES, NL, SE, CH, and other Europeans + GCC. Tier 3 (ISO prefix passthrough): all others.
**Feasibility note:** Country-specific source scrapers are a long tail. EPO OPS covers ~80 countries via exchange data, which significantly reduces per-country effort. Lens.org covers 160+ countries.
**Consequences:** Broad coverage via EPO OPS + Lens.org. Country-specific scrapers added incrementally.
