# PLAN — 03c-web-search-fallback: Web Search Last Resort

*Depends on: 06-config, 01-id-canon*
*Small, focused node — only called when all structured sources fail*

---

## Python Implementation

### T01 — Query generation
- **RED**: `test_query_generation_us` — `generate_queries(CanonicalPatentId(canonical="US7654321", ...))` → list containing `'"US7654321" patent PDF'` and `'US7654321 patent full text'`
- **RED**: `test_query_generation_ep` — EP canonical → queries include `site:epo.org`
- **RED**: `test_query_generation_wo` — WO canonical → appropriate queries
- **GREEN**: implement `generate_queries(id: CanonicalPatentId) -> list[str]` with jurisdiction-specific templates

### T02 — URL confidence scoring
- **RED**: `test_confidence_high_when_id_in_url` — URL `https://patents.google.com/patent/US7654321` → `"high"`
- **RED**: `test_confidence_medium_when_known_domain` — URL `https://patents.justia.com/...` without ID → `"medium"`
- **RED**: `test_confidence_low_generic` — arbitrary URL → `"low"`
- **GREEN**: implement `score_url_confidence(url: str, canonical_id: str) -> str`

### T03 — DuckDuckGo Instant Answer API
- **RED**: `test_ddg_api_called` — mock httpx GET to DDG endpoint; `DuckDuckGoSearchBackend.search("US7654321 patent PDF")` → returns list of URL results
- **RED**: `test_ddg_empty_results` — DDG returns empty JSON → empty list, no exception
- **GREEN**: implement `DuckDuckGoSearchBackend.search()` with `httpx`

### T04 — Result assembly
- **RED**: `test_fallback_result_schema` — `WebSearchFallbackSource.fetch(canonical_id, output_dir, config)` returns `SourceAttempt` with `success=True`, `formats_retrieved=[]`; plus `urls` list in result dict
- **RED**: `test_fallback_notes_in_result` — result includes explanatory note about being a fallback
- **GREEN**: implement `WebSearchFallbackSource.fetch()` — generate queries, call DDG, score URLs, return SourceAttempt

### T05 — SerpAPI backend (optional)
- **RED**: `test_serpapi_called_when_ddg_fails` — mock DDG to return error; mock SerpAPI; verify SerpAPI called
- **RED**: `test_serpapi_skipped_when_no_key` — `config.serpapi_key=None`; SerpAPI not tried
- **GREEN**: fallback chain: DDG → SerpAPI → Bing (each skipped if no key)

### T06 — Only called as last resort
- **RED**: `test_fallback_returns_no_artifacts` — `SourceAttempt.formats_retrieved` is always `[]` for this source
- **GREEN**: no file writing in this source

---

## Rust Implementation

### T07 — Rust: mirror T01–T04
- Same query generation and confidence scoring in Rust
- HTTP mocked via mock server or `mockito`

---

## Acceptance Criteria
- Never writes files (returns URLs only)
- Never raises exceptions on any input
- URL confidence scoring is deterministic

## Dependencies
- `06-config`, `01-id-canon`
