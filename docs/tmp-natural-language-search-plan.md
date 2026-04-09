# Natural Language Patent Search Plan

## Current State

- Natural-language patent search already exists, but only in the Python `patent-search` MCP server.
- `patent_search_natural` currently just sends the raw description to SerpAPI Google Patents if `PATENT_SERPAPI_KEY` is set, then dedupes results.
- Search sessions, exports, annotations, citation/family/classification tools already exist in Python.
- There is no Rust-side search MCP yet.
- The existing Playwright Google code is for fetching a patent detail page by ID, not for running Google Patents search flows.

Relevant code:
- `src/python/patent_mcp/search/server.py`
- `src/python/patent_mcp/search/searchers.py`
- `src/python/patent_mcp/search/session_manager.py`
- `src/python/patent_mcp/scrapers/google_patents.py`

## Recommendation

Use a hybrid design:

1. Keep the Python `patent-search` server as the first home for natural-language search.
2. Add a new direct Google Patents browser-search backend using Playwright with persistent isolated profiles.
3. Keep SerpAPI as a fallback/fast path, not the primary path.
4. Do not treat “natural language search” as one query sent to one backend.
5. Instead, build a search planner that turns one user description into multiple Google-friendly search variants, runs them, dedupes, enriches, and reranks.
6. Reuse the existing fetch-by-ID pipeline to enrich top hits after search.

That is the smallest plan that fits the repo as it exists.

## Why Google Patents First

- Best broad coverage and best practical search UX for vague invention descriptions.
- Strong keyword, assignee, inventor, classification, and date filtering.
- Good result pages for later click-through and enrichment.
- More useful for natural-language-ish discovery than trying to start from USPTO syntax generation.

## Important Constraint

Google Patents is still not true semantic search. “Natural language” should mean:

- accept a plain-English invention description from the user
- expand it into multiple search formulations
- use Google Patents as the primary retrieval surface
- optionally fan out into USPTO/EPO once we have better terms and CPCs

That will work much better than just sending the raw paragraph unchanged.

## Full Plan

### 1. Product Goal

Add a search system that lets a user say something like:

- “Find prior art for wireless power transfer through metal barriers”
- “Find older patents similar to this invention summary”
- “Search for patents about robotic fruit picking using soft grippers before 2018”

And get back:

- high-quality candidate patents
- why each was matched
- the actual queries used
- session persistence
- optional enrichment via full fetch-by-ID
- optional Google-authenticated browser search if needed

### 2. Non-Goals For V1

- Full Rust port of the search server
- Full autonomous prior-art agent loop
- Citation graph crawling beyond current tools
- Guaranteed Google account automation
- CAPTCHA defeat or stealth browser escalation

### 3. Architecture

Build these layers in Python search MCP:

1. `NaturalLanguagePlanner`
2. `GooglePatentsBrowserSearchBackend`
3. `SearchAggregator`
4. `HitEnricher`
5. `ProfileManager`

Concrete integration points with the current codebase:

- `server.py` remains the MCP surface and request orchestrator for search tools.
- `searchers.py` remains where search backends live; the new Google browser backend should either live there or in a nearby module imported by it, not as a detached subsystem.
- `session_manager.py` remains the persistence layer; v1 should keep using `QueryRecord` rather than introducing a second durable record type.
- Enrichment should call the existing Python `FetcherOrchestrator` in `fetchers/orchestrator.py` directly, using canonical patent IDs and the existing cache/output conventions.
- Rust remains out of scope for search in v1; search stays Python-first because the existing MCP search server, sessions, and backends already live there.

Suggested file additions:

- `src/python/patent_mcp/search/planner.py`
- `src/python/patent_mcp/search/google_browser_backend.py`
- `src/python/patent_mcp/search/ranking.py`
- `src/python/patent_mcp/search/profile_manager.py`

Keep `server.py` as the MCP surface.

### 4. Search Flow

For `patent_search_natural(description=...)`:

1. Parse the user request into a normalized search intent.
2. Generate multiple query variants:
   - raw broad phrase
   - synonym-expanded phrase
   - title/abstract-focused query
   - CPC-seeking query
   - assignee/inventor/date constrained variants if user supplied hints
3. Run those against Google Patents browser backend.
4. Optionally run SerpAPI in parallel if configured.
5. Merge and dedupe by canonical patent ID.
6. Score and rerank hits.
7. Enrich top N hits using existing fetch-by-ID pipeline.
8. Save everything to session history.

Implementation note for enrichment:

- Each result must be normalized to a canonical patent ID before enrichment.
- Canonicalization should use the existing Python ID canonicalization module, not ad hoc parsing.
- Enrichment should call `FetcherOrchestrator.fetch_batch(...)` directly from Python, bounded by a small concurrency limit.
- Enrichment in v1 should be metadata-first. Do not fetch PDFs/Markdown by default during search unless explicitly requested later.
- Enrichment output should be merged into the returned search hit payload, while artifacts remain in the existing patent cache locations managed by the orchestrator.

### 5. Natural Language Planning Layer

Add a planner that converts plain English into structured search intent.

Input:
- free text description
- optional date cutoff
- optional jurisdictions
- optional patent type
- optional known companies/inventors
- optional “prior art” mode

Planner output:
- key concepts
- synonyms
- exclusions
- likely CPC/IPC seeds
- query variants
- search rationale

For v1, keep this deterministic and rule-based.
No LLM dependency required initially.

Example planner output for “wireless charging through metal”:
- concepts: wireless charging, inductive coupling, resonant transfer, metal barrier, conductive shield, through-wall transfer
- CPC seeds: `H02J50`, maybe `H01F`
- Google query variants:
  - `"wireless charging" AND metal`
  - `("inductive power transfer" OR "resonant power transfer") AND (metal OR conductive barrier)`
  - `CPC=H02J50/10 AND (metal OR barrier OR shield)`
  - `(TI=("wireless charging") OR AB=("wireless charging")) AND metal`
  - `before:priority:20200101 ...` if cutoff provided

### 6. Google Backend Design

Add a new backend that uses Playwright to drive Google Patents search result pages.

Implementation choice for v1:

- Use Playwright in a contained Python subprocess wrapper, similar in spirit to the existing Google detail-page scraper boundary, rather than embedding long-lived browser automation directly inside the MCP server process.
- The search MCP server should call that wrapper and parse structured JSON results.
- Reason: this keeps browser crashes, stuck pages, and Playwright dependency loading isolated from the main MCP process while matching the repo's existing tolerance for browser-specific wrappers.

Core functions:
- build Google search URL from query parameters
- navigate search page
- wait for results
- parse result cards
- paginate for more results
- return normalized `PatentHit`s

It should support:
- query string
- before/after date
- assignee
- inventor
- status grant/application
- country/jurisdiction if Google supports it
- result limit
- profile selection

### 7. Browser Profile / Login Design

This is the key new requirement.

Use persistent isolated Chromium profiles, one directory per search profile.

Suggested layout:
- `~/.local/share/patent-search/browser-profiles/default/`
- `~/.local/share/patent-search/browser-profiles/google-login-1/`

Add config:
- `PATENT_SEARCH_BROWSER_PROFILE_DIR`
- `PATENT_SEARCH_BROWSER_HEADLESS=true|false`
- `PATENT_SEARCH_BROWSER_CHANNEL=chromium|chrome`
- `PATENT_SEARCH_BROWSER_TIMEOUT=...`

Add MCP tools for v1:

- `patent_search_profile_login_start(name, headed=true)`

Deferred until after v1 proves stable:

- `patent_search_profile_create(name)`
- `patent_search_profile_list()`
- `patent_search_profile_delete(name)`
- `patent_search_profile_status(name)`

Behavior:
- `login_start` launches persistent browser in headed mode with that isolated profile.
- User logs into Google manually if desired.
- We store no credentials ourselves; Chromium profile owns them.
- Search runs later can use that named profile in headless or headed mode.

Important implementation detail:
- Use Playwright `launch_persistent_context`, not ephemeral `browser.new_context`.
- Enforce one active process per profile with a lock file.

Lifecycle decision for v1:

- A persistent profile directory may only be owned by one browser process at a time.
- `patent_search_profile_login_start(name, headed=true)` is a temporary interactive flow whose purpose is to populate/update the profile and then exit cleanly.
- Normal search calls do not attach to a resident browser. Instead, each search launches its own persistent context against the profile directory, performs the search, then closes it.
- If a login browser is currently using a profile, search calls against that profile must fail fast with a clear "profile busy" error rather than attempting to share it.
- Lock files must include PID, hostname, started-at timestamp, and command purpose (`login` or `search`).
- Stale-lock recovery should only clear a lock if the owning PID is gone; otherwise the profile remains busy.
- Profile deletion is not part of v1. It can be added later once lifecycle and safety semantics are proven.

This keeps the model simple: one profile directory, one owning process, short-lived searches, and explicit interactive login.

### 8. Search Modes

Support three modes:

1. `serpapi`
   - fastest
   - current backend
   - no browser/login
2. `browser`
   - primary new mode
   - Google Patents via Playwright
3. `hybrid`
   - run both
   - merge results

Default recommendation:
- `browser` first for v1 when Playwright is available
- fall back to `serpapi`
- last resort: current static suggestion tool only

Scope decision for v1:

- Keep `hybrid` implemented only if it falls out naturally after `browser` and `serpapi` exist behind the same aggregator.
- Do not make `hybrid` the default in v1. It increases spend, complexity, and debugging surface without being necessary for the first usable release.

### 9. Result Parsing

Each Google result should capture:

- `patent_id`
- `title`
- `snippet` or abstract preview
- `assignee`
- `inventors`
- `priority/publication date`
- `result URL`
- `matched_query`
- `page_number`
- `rank_within_query`
- `backend`

Do not rely on brittle CSS only.
Prefer:
- stable attributes if present
- URL pattern extraction for patent ID
- visible text fallback

Keep raw HTML snapshots in debug mode for parser repair.

### 10. Ranking / Relevance

Natural-language effectiveness depends more on reranking than on the first query.

Add a simple reranker using:

- query term coverage in title
- query term coverage in snippet
- CPC overlap with planner-suggested CPCs
- date cutoff satisfaction
- source confidence
- repeated appearance across multiple query variants
- assignee/inventor match bonuses if requested

This can stay heuristic in v1.
A hit found by 3 different query variants should rank above a one-off weak match.

### 11. Enrichment

After search, enrich the top results.

For top 5-10 hits:
- canonicalize result IDs using the existing ID canon module
- call Python `FetcherOrchestrator.fetch_batch(...)`
- use the existing cache/output layout under the patent cache
- pull canonical metadata
- optionally fetch PDF/markdown only in a later explicit mode
- merge better title/abstract/assignee/dates into search results

This reuses existing infrastructure instead of duplicating metadata logic in the search backend.

Concrete v1 rule:

- `enrich_top_n` defaults to a small number such as 3-5.
- Enrichment concurrency should be lower than general fetch concurrency to avoid turning one search into a large source fan-out burst.
- If enrichment fails for a hit, the raw search hit should still be returned.

### 12. Session Model Changes

Extend session storage so searches are auditable.

Persistence decision for v1:

- Keep `QueryRecord` as the durable session unit.
- Do not add a separate durable `SearchRun` type in v1.
- If richer planner/backend metadata is needed, add optional fields to `QueryRecord` and update `session_manager.py` serialization accordingly.
- `SearchAggregator` and `HitEnricher` are runtime components, not persistent model types.

Add to `QueryRecord` in v1 only if needed for auditability:
- `search_mode`
- `planner_output`
- `raw_queries`
- `backend_queries`
- `profile_name`
- `filters`
- `ranking_version`

If adding all of those makes `QueryRecord` too heavy, prefer a single optional `metadata: dict[str, Any]` field over inventing a second record model.

### 13. MCP API Changes

Keep current tools, but extend them.

Update `patent_search_natural` params:
- `backend: "auto" | "browser" | "serpapi" | "hybrid"`
- `profile_name: str | None`
- `prior_art_mode: bool = true`
- `enrich_top_n: int = 3`
- `debug: bool = false`

V1 tool surface decision:

- Extend `patent_search_natural` first.
- Keep `patent_search_structured` unchanged unless it becomes trivial to add `backend` selection.
- Add only one profile-management tool in v1: `patent_search_profile_login_start(name, headed=true)`.
- Defer `profile_create`, `profile_delete`, `profile_status`, and low-level `patent_search_google_browser(...)` unless implementation experience shows they are necessary.

Named profiles can be created implicitly on first use by `login_start` or a browser-backed search call.

### 14. Config Changes

Extend `PatentConfig` with search-specific config:

- `search_browser_enabled`
- `search_browser_profiles_dir`
- `search_browser_default_profile`
- `search_browser_headless`
- `search_browser_timeout`
- `search_browser_max_pages`
- `search_browser_debug_html_dir`
- `search_backend_default`
- `search_enrich_top_n`

### 15. Implementation Phases

#### Phase 1: Stabilize the planner
- add deterministic NL-to-query planner
- update `patent_search_natural` to generate multiple query variants
- keep using SerpAPI first
- add tests for planner output

#### Phase 2: Add Google browser backend
- implement Playwright search-page navigation
- parse first page of results
- add pagination
- add parser fixtures
- add backend-specific tests
- implement it behind a subprocess wrapper returning structured JSON to the MCP server

#### Phase 3: Add isolated persistent profiles
- add profile manager
- add login-start MCP tool
- add lock ownership, busy-profile errors, and stale-lock recovery

#### Phase 4: Add ranking and enrichment
- merge multi-query results
- heuristic reranking
- enrich top hits through fetch-by-ID
- persist richer session data

#### Phase 5: Documentation and workflow
- update `AGENT_USAGE.md`
- add setup docs for browser profiles and login
- add smoke-test recipes in `justfile`

### 16. Testing Plan

Unit tests:
- planner output for representative invention descriptions
- Google query builder
- result card parser from saved HTML fixtures
- ranking heuristics
- profile path and lock handling

Integration tests:
- mocked Playwright HTML result pages
- `patent_search_natural` with browser backend mocked
- session persistence with planner output included

Manual smoke tests:
1. anonymous browser search
2. headed login flow
4. logged-in profile reused in headless mode
5. search + enrich top hits
6. export session report

Add test fixtures for:
- zero results
- one-page results
- multi-page results
- Google layout drift cases

### 17. Operational Risks

- Google markup may drift
- login state can expire
- CAPTCHA / bot detection may appear
- headless may behave differently from headed
- browser automation is slower than SerpAPI

Mitigations:
- keep SerpAPI fallback
- save parser fixtures
- support headed retry mode
- keep browser backend isolated from core fetch-by-ID server
- use robust timeouts and retries
- store debug artifacts on parse failures

### 18. Recommended V1 Scope

The best first shippable slice is:

1. deterministic NL planner
2. multi-query Google browser search backend
3. persistent isolated profile support
4. session persistence of queries and results
5. top-5 enrichment via existing fetch pipeline
6. SerpAPI fallback

That gets the useful part working without needing a Rust rewrite or a full research-agent redesign.

Practical tightening for v1:

- Default to `browser` mode, not `hybrid`.
- Ship only the explicit login-start profile tool; defer broader profile administration.
- Keep enrichment metadata-only by default.
- Keep persistence on top of `QueryRecord`.
- Keep browser automation isolated in a subprocess wrapper.

### 19. What I Would Not Do First

- Port search to Rust immediately
- Depend on Google login for baseline functionality
- Use an LLM for every query generation step
- Build citation crawling into the same first milestone
- Make search browser logic part of the Rust fetch server

### 20. Concrete Next Deliverable

If we start implementation, I’d suggest this exact first milestone:

1. Add `planner.py`
2. Add `google_browser_backend.py` plus a subprocess wrapper entrypoint
3. Extend `patent_search_natural` to use planner + backend
4. Add `profile_manager.py` with profile locking and `login_start`
5. Add parser fixtures and tests
6. Add docs and a `just` smoke test

## Open Questions Before Implementation

- Should browser search be allowed to run headed in normal agent-triggered flows, or only through explicit profile/login tools?
- Should enriched search results fetch metadata only, or also download PDFs/Markdown by default?
- Should V1 persist raw Google search HTML snapshots only in debug mode, or always for reproducibility?
- Do we want one named default browser profile, or should all profiles be explicit to avoid state surprises?
- Should hybrid mode run SerpAPI and browser in parallel by default, or prefer browser first to reduce API spend?
