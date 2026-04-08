# ROOT — mcp-fetch-patents Plan Tree

*Last updated: 2026-04-07*

## Project
Single-responsibility MCP server that fetches patents by ID, caches everything locally + globally, and returns file paths + metadata. Dual-implementation: Python (reference/fast-iteration) + Rust (production). Cross-validated via shared deterministic mock test harness.

## Tree Status

| Node | Name | Status | Dependencies |
|---|---|---|---|
| 01 | Patent ID Canonicalization | PLAN reviewed | — |
| 02 | Cache + Global DB | PLAN reviewed | 01 |
| 03 | Source Fetchers (parent) | PLAN reviewed | 01, 02 |
| 03a | HTTP Sources | PLAN reviewed | 01, 02, 06 |
| 03b | Browser Sources (Playwright) | PLAN reviewed | 01, 02, 06 |
| 03c | Web Search Fallback | PLAN reviewed | 01 |
| 04 | Format Conversion | PLAN reviewed | — |
| 05 | MCP Protocol / Server | PLAN reviewed | 01, 02, 03, 04, 06 |
| 06 | Configuration System | PLAN reviewed | — |
| 07 | Cross-Impl Test Infrastructure | PLAN reviewed | all |

## Node Dependency Graph
```
06-config ──────────────────────────────┐
01-id-canon ──────────────────────────┐  │
                                      ▼  ▼
02-cache-db ◄──────────────────── 03-source-fetchers
     │                             ├── 03a-http-sources
     │                             ├── 03b-browser-sources
     │                             └── 03c-web-search-fallback
     │
04-format-conversion ◄─────────────────────────┐
     │                                          │
     └────────────────── 05-mcp-protocol ────────┘
                               │
                         07-test-infra (validates all)
```

## Plan Documents

| Document | Purpose |
|---|---|
| `PRODUCT_GOALS.md` | Confirmed requirements, success criteria, non-goals |
| `DECISIONS.md` | 14 ADRs covering all architectural choices |
| `RESEARCH_LOG.md` | Prior art survey + API landscape findings |
| `SCAFFOLDING.md` | Project structure setup (do FIRST) |
| `PHASE_B_RUST.md` | Detailed Rust implementation plan (Phase B) |

## Implementation Phases

### Phase 0: Scaffolding (before anything else)
See `SCAFFOLDING.md`: create Python package structure, pyproject.toml, Cargo.toml, test directory, CI config.

### Phase A: Python Reference Implementation
Build all nodes in Python, full test coverage, all sources integrated.
Order: 06-config → 01-id-canon → 07-test-infra → 02-cache-db → 04-format-conversion → 03a+03c → 03b → 03 → 05

### Phase B: Rust Production Implementation
See `PHASE_B_RUST.md` for detailed tasks, crate choices, risks, and Rust-specific patterns.
Mirror Python behavior in Rust. Same interfaces, same behavior.

### Phase C: Parity Validation + Fuzzing
Run cross-impl test harness (07-test-infra T16-T21). Python output == Rust output. Add fuzzing.

## Repo Layout (planned)
```
mcp-fetch-patents/
  src/
    python/            # Python implementation
      patent_mcp/      # main package
        id_canon.py    # 01
        cache.py       # 02
        db.py          # 02 (SQLite global index)
        fetchers/      # 03
          http.py      # 03a
          browser.py   # 03b (Playwright)
          web_search.py # 03c
        converters/    # 04
        server.py      # 05 (MCP server)
        config.py      # 06
    rust/              # Rust implementation
      src/
        id_canon/      # 01
        cache/         # 02
        fetchers/      # 03
        converters/    # 04
        server/        # 05
        config/        # 06
  tests/
    fixtures/          # shared JSON patent fixtures
      us/              # US patent fixtures
      ep/              # EP fixtures
      wo/              # WO fixtures
      ...
    mock_server/       # deterministic HTTP mock server
    python/            # Python unit tests (pytest)
    rust/              # Rust unit tests (cargo test)
    cross_impl/        # parity tests: Python == Rust
  docs/
    ultra-plans/       # this plan tree
    api-keys.md        # where to get keys for each source
  pyproject.toml
  Cargo.toml
  .patents.toml.example
```

## Key Constraints
- Test suite: <1s (all network mocked, marker disabled by default in tests)
- No patent content in MCP response body (file paths + metadata only)
- Cache-first: never fetch twice for the same patent ID
- Dual-impl parity enforced by test harness
