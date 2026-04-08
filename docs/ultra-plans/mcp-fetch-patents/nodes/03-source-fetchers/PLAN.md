# PLAN — 03-source-fetchers: Fetcher Orchestrator

*Depends on: 03a, 03b, 03c (all source implementations), 02-cache-db, 04-format-conversion*
*The orchestrator wires everything together*

---

## Python Implementation

### T01 — Source registry
- **RED**: `test_sources_registered_for_us` — `FetcherOrchestrator(config).get_sources_for(canonical_id_us)` → returns list including at least `PpubsSource`, `GooglePatentsSource`
- **RED**: `test_sources_registered_for_ep` — EP canonical → includes `EpoOpsSource`
- **GREEN**: implement `get_sources_for(id: CanonicalPatentId) -> list[BasePatentSource]` filtered by `supported_jurisdictions`

### T02 — Source priority ordering
- **RED**: `test_sources_in_config_priority_order` — config has `source_priority=["EPO_OPS", "USPTO"]`; EP patent → EPO_OPS first
- **GREEN**: sort returned sources by position in `config.source_priority`

### T03 — Cache hit skips all fetching
- **RED**: `test_cache_hit_returns_immediately` — mock `cache.lookup()` to return a `CacheResult`; `orchestrator.fetch(id, output_dir)` → returns without calling any source
- **GREEN**: check cache first; return `OrchestratorResult` with cached paths if hit

### T04 — Single source success
- **RED**: `test_single_source_success` — mock `PpubsSource.fetch()` → success with PDF; `orchestrator.fetch()` → `OrchestratorResult(success=True)` with PDF path
- **GREEN**: iterate sources; stop after first success if `fetch_all_sources=False`

### T05 — Fan-out for completeness (fetch_all_sources=True)
- **RED**: `test_all_sources_tried_when_fetch_all` — config `fetch_all_sources=True`; mock 3 sources; all 3 called even though first succeeds; artifacts aggregated
- **GREEN**: when `fetch_all_sources=True`, continue fetching remaining sources in background

### T06 — Partial success (some sources fail)
- **RED**: `test_partial_success` — source 1 returns PDF; source 2 returns 404; `OrchestratorResult(success=True)`, `sources[1].success==False`, artifacts has PDF
- **GREEN**: existing logic handles this — success=True if any source succeeded

### T07 — All sources fail → try web search fallback
- **RED**: `test_web_search_fallback_triggered` — all structured sources mock as failed; `WebSearchFallbackSource.fetch()` called; `OrchestratorResult` has `sources[-1].source_name=="web_search_fallback"`
- **GREEN**: add fallback trigger after all structured sources fail

### T08 — Format conversion triggered after fetch
- **RED**: `test_conversion_called_after_pdf_downloaded` — mock source returns PDF path; mock `ConverterPipeline.pdf_to_markdown()` called once; output_dir contains `patent.md`
- **GREEN**: call conversion pipeline after successful fetch

### T09 — Batch fetch
- **RED**: `test_batch_fetch_processes_all` — `orchestrator.fetch_batch([id1, id2, id3], cache_dir)` → list of 3 `OrchestratorResult`
- **RED**: `test_batch_fetch_concurrent` — measure time: 3 patents with 100ms mock delay each; batch should complete in <200ms (parallel execution)
- **GREEN**: use `asyncio.gather()` for batch; configurable concurrency limit

### T10 — Failed patent doesn't affect others in batch
- **RED**: `test_batch_one_fail_others_succeed` — 3 patents; middle one all sources fail; `results[0].success==True`, `results[1].success==False`, `results[2].success==True`
- **GREEN**: `asyncio.gather(return_exceptions=True)` + error handling per item

### T11 — Cache store after successful fetch
- **RED**: `test_artifacts_stored_in_cache` — mock sources return artifacts; verify `cache.store()` called with all artifacts
- **GREEN**: call `cache.store()` at end of successful fetch

---

## Rust Implementation

### T12 — Rust: orchestrator mirrors Python in `fetchers/orchestrator_test.rs`
- Mirror T01–T08 using tokio async
- Mock sources via trait objects

### T13 — Parity: orchestrator result JSON matches Python
- `test_orchestrator_parity` in `cross_impl/` — same fixture IDs, same mock server → `OrchestratorResult` JSON identical

---

## Acceptance Criteria
- Batch of 10 patents with mocked 50ms source latency completes in <200ms (parallelism)
- Cache hit is returned in <1ms (no source calls)
- `success=True` if at least one format from any source was acquired
- Fallback to web search always tried when all structured sources fail

## Dependencies
- `03a-http-sources`, `03b-browser-sources`, `03c-web-search-fallback`
- `02-cache-db`, `04-format-conversion`, `06-config`
