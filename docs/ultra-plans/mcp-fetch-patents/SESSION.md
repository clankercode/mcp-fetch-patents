# SESSION — mcp-fetch-patents

## Current Phase
**Plan tree reviewed and fixed** → READY FOR IMPLEMENTATION

## Status
- Bootstrap: DONE
- Interview: DONE (all P0/P1 questions answered)
- Decomposition: DONE (7 top-level nodes, 3 source-fetcher children)
- SPECs written: all nodes
- INTERFACEs written: all nodes
- DECISIONS.md: populated (ADR-001 through ADR-010)
- Research agent: running in background (prior art + API landscape)
- Leaf PLANs: NOT YET WRITTEN

## Last Action
Complete plan review: fixed all 16 issues (4 critical, 6 major, 6 minor).
- Created missing INTERFACE.md for 03a, 03b, 03c
- Added SessionCache to 02 INTERFACE.md
- Expanded BigQuery tasks (T09a-e) in 03a PLAN.md
- Added metadata field to SourceAttempt for web search URLs
- Added Playwright HTML fixtures (T07b-c) to 07 PLAN.md
- Added parity test execution loop (T16-T21) to 07 PLAN.md
- Added pymupdf+pdfplumber merge tasks to 04 PLAN.md
- Fixed postprocess_query spec + test assertions
- Fixed DB parity approach (SQLite dump vs log lines)
- Created SCAFFOLDING.md (project structure + pyproject.toml + Cargo.toml)
- Created PHASE_B_RUST.md (detailed Rust tasks B01-B10, crate choices, risks)

Previous: Wrote SPEC.md + INTERFACE.md for all 10 nodes:
- 01-id-canon, 02-cache-db, 03-source-fetchers
- 03a-http-sources, 03b-browser-sources, 03c-web-search-fallback
- 04-format-conversion, 05-mcp-protocol, 06-config, 07-test-infra
Wrote ROOT.md, DECISIONS.md, PRODUCT_GOALS.md (final), RESEARCH_LOG.md stub

## Next Planned Action
1. Await research agent results → update RESEARCH_LOG.md → revise SPECs if needed
2. Write leaf PLAN.md files for each node (TDD task lists)
3. Scope-prune: review for YAGNI issues
4. Handoff to implementation

## Implementation Order (planned)
```
Phase A — Python Reference:
  1. 06-config (no deps)
  2. 01-id-canon (no deps)
  3. 07-test-infra mock server (enables all other tests)
  4. 02-cache-db
  5. 04-format-conversion
  6. 03a-http-sources + 03c-web-search-fallback
  7. 03b-browser-sources (Playwright)
  8. 03-source-fetchers (orchestrator)
  9. 05-mcp-protocol (final assembly)

Phase B — Rust Mirror:
  Same order; reuse fixtures from Phase A
  
Phase C — Parity Validation + Fuzzing
```

## Key Constraints (reminders)
- Test suite: <1s (mock all I/O; skip marker + Playwright in default test run)
- No patent content in MCP response (file paths + metadata only)
- Dual-impl parity enforced; DB writes logged for cross-impl comparison
- Legal status: DB schema stubbed only (v2 feature)
- HTTP transport: v2 only

## Open Threads
- [x] Research agent results → RESEARCH_LOG.md updated
- [x] 03a-http-sources SPEC updated (BigQuery added, PatentsView removed, WIPO/Lens downgraded to scraping)
- [x] 04-format-conversion SPEC updated (pdfplumber added, Nougat excluded)
- [x] 02-cache-db SPEC updated (PPUBS session token cache added)
- [x] 05-mcp-protocol SPEC updated (token budget + stderr/stdout discipline)
- [x] ADRs 011-014 added (BigQuery, PatentsView dead, WIPO/Lens scraping-only)
- [ ] SCOPE CONCERN: country-specific scrapers are long tail; EPO OPS covers ~100 offices via exchange data + BigQuery covers 17+ — most per-country scrapers are v2
- [x] All leaf PLAN.md files written (TDD task lists for all 10 nodes)
- [ ] **READY FOR IMPLEMENTATION** — begin Phase A (Python) with 06-config
