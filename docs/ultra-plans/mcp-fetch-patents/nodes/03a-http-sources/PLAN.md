# PLAN — 03a-http-sources: HTTP/API Patent Sources

*Depends on: 06-config, 01-id-canon, 07-test-infra (mock server)*
*All HTTP calls routed to mock server via `config.source_base_urls`*

---

## Python Implementation

### T01 — Base source interface
- **RED**: `test_base_source_has_required_interface` — create a minimal subclass of `BasePatentSource`; verify it has `source_name`, `supported_jurisdictions`, `fetch()` method
- **GREEN**: implement `BasePatentSource` ABC with those abstract members

### T02 — USPTO PPUBS: session establishment
- **RED**: `test_ppubs_session_established` — mock server returns session token JSON at `/ppubs-api/v1/session`; `PpubsSource.get_session(config)` → returns token string
- **RED**: `test_ppubs_session_cached` — call `get_session()` twice; mock server hit only once (session cache)
- **GREEN**: implement session establishment + cache with TTL; use `SessionCache` from 02-cache-db

### T03 — USPTO PPUBS: document search
- **RED**: `test_ppubs_search_us_patent` — mock server at `/ppubs-api/v1/patent?patentNumber=7654321`; returns fixture JSON; assert `SourceAttempt(success=True, formats_retrieved=["txt"])`
- **RED**: `test_ppubs_not_found` — mock returns empty results; `SourceAttempt(success=False, error="not_found")`
- **GREEN**: implement `PpubsSource.fetch()` — search endpoint → extract document GUID → retrieve full text

### T04 — USPTO PPUBS: PDF download
- **RED**: `test_ppubs_pdf_download` — mock server at `/ppubs-api/v1/download/...` returns stub PDF bytes; verify file written to output_dir
- **GREEN**: add PDF download step to `PpubsSource.fetch()`; use `httpx` async streaming download

### T05 — USPTO PPUBS: HTTP/2
- **RED**: `test_ppubs_uses_http2` — verify `httpx.AsyncClient(http2=True)` is used (check client creation in source)
- **GREEN**: create client with `http2=True`; add `h2` to dependencies

### T06 — EPO OPS: OAuth2 token
- **RED**: `test_epo_ops_auth_token_requested` — mock token endpoint at `/3.2/auth/accesstoken`; verify POST with client credentials; `EpoOpsSource._get_token(config)` → returns token
- **RED**: `test_epo_ops_auth_token_cached` — token cached; only one auth request
- **RED**: `test_epo_ops_no_credentials_returns_none` — `config.epo_client_id=None`; `_get_token()` → None
- **GREEN**: implement OAuth2 client credentials flow; token cached with TTL from response

### T07 — EPO OPS: bibliographic fetch
- **RED**: `test_epo_ops_fetch_ep_patent` — mock server at `/3.2/rest-services/published-data/publication/biblio/EP1234567`; returns fixture XML; assert metadata extracted (title, dates, inventors)
- **RED**: `test_epo_ops_fetch_us_via_exchange` — mock returns exchange data for US patent; assert success
- **GREEN**: implement `EpoOpsSource.fetch()` — auth + bibliographic endpoint + XML parsing with `lxml` or `xml.etree`

### T08 — EPO OPS: PDF download
- **RED**: `test_epo_ops_pdf_download` — mock PDF endpoint; verify PDF written
- **GREEN**: add PDF fetch step to `EpoOpsSource`

### T09 — BigQuery source (optional)

**T09a — Authentication and graceful init failure**
- **RED**: `test_bigquery_no_credentials_graceful` — no `GOOGLE_APPLICATION_CREDENTIALS`; `BigQuerySource.__init__()` warns, sets `available=False`; `fetch()` → `SourceAttempt(success=False, error="BigQuery not configured: no credentials")`
- **RED**: `test_bigquery_service_account_loaded` — set `GOOGLE_APPLICATION_CREDENTIALS=/path/to/creds.json`; mock `google.auth.load_credentials_from_file()`; verify BigQuery client created
- **RED**: `test_bigquery_adc_fallback` — no explicit credentials but `GOOGLE_CLOUD_PROJECT` set; verify ADC path attempted
- **GREEN**: wrap BigQuery client init in `try/except (google.auth.exceptions.DefaultCredentialsError, Exception)`; set `self.available` flag

**T09b — SQL query construction**
- **RED**: `test_bigquery_us_query_contains_publication_number` — `BigQuerySource._build_query(CanonicalPatentId(canonical="US7654321"))` → SQL string contains `WHERE publication_number = 'US-7654321'` (BigQuery format: `{CC}-{NUM}`)
- **RED**: `test_bigquery_ep_query` — EP canonical → `WHERE publication_number = 'EP-1234567'`
- **RED**: `test_bigquery_kind_code_handled` — `US7654321B2` → query strips kind code for initial lookup, uses `LIKE 'US-7654321%'` or family lookup
- **GREEN**: implement `_build_query()` generating parameterized BigQuery SQL against `patents-public-data.patents.publications`

**T09c — Result parsing and metadata extraction**
- **RED**: `test_bigquery_result_mapped_to_metadata` — mock BigQuery row with `title_localized`, `filing_date`, `grant_date`, `inventor_harmonized` columns; verify `PatentMetadata` populated correctly
- **RED**: `test_bigquery_no_results` — mock returns empty row list; `SourceAttempt(success=False, error="not_found")`
- **GREEN**: implement `_parse_row()` mapping BigQuery column names to `PatentMetadata` fields

**T09d — Quota / timeout handling**
- **RED**: `test_bigquery_timeout_graceful` — mock BigQuery raises `google.cloud.exceptions.DeadlineExceeded`; → `SourceAttempt(success=False, error="BigQuery timeout: ...")`
- **RED**: `test_bigquery_quota_exceeded_graceful` — mock `google.api_core.exceptions.ResourceExhausted`; → graceful failure, logs warning
- **GREEN**: catch specific BigQuery exceptions

**T09e — BigQuery fixture format**
- Add to `tests/fixtures/us/US7654321/sources/bigquery_response.json`:
  ```json
  [{"publication_number": "US-7654321-B2", "title_localized": [...], "inventor_harmonized": [...], "filing_date": "20050312", "grant_date": "20100119"}]
  ```
- Routes in `mock_server/routes.json` don't apply to BigQuery (SDK, not HTTP) — use mock via `unittest.mock.patch`

### T10 — Retry logic (all sources)
- **RED**: `test_retry_on_429` — mock server returns 429 then 200; verify `PpubsSource.fetch()` retries and succeeds; verify 2 HTTP calls made
- **RED**: `test_retry_on_503` — same for 503
- **RED**: `test_no_more_than_3_retries` — mock always returns 429; verify exactly 3 attempts then fail
- **GREEN**: implement retry with `tenacity`: `stop_after_attempt(3)`, `wait_exponential(min=1, max=8)`

### T11 — Espacenet scraping
- **RED**: `test_espacenet_extract_metadata` — mock server returns fixture HTML at `/patent/EP1234567`; verify title, dates extracted via BeautifulSoup
- **RED**: `test_espacenet_pdf_link_found` — HTML contains PDF link; verify URL captured
- **GREEN**: implement `EspacenetSource` using `httpx` + `BeautifulSoup`

### T12 — WIPO PatentScope scraping (no API key)
- **RED**: `test_wipo_scraping_extracts_wo_patent` — mock HTML page; verify WO patent metadata extracted
- **GREEN**: implement `WipoScrapeSource` using httpx + BeautifulSoup

### T13 — Country-specific: IP Australia
- **RED**: `test_ip_australia_fetch` — IP Australia has a REST API (AusPat); mock response; verify AU patent metadata
- **GREEN**: implement `IpAustraliaSource`

### T14 — Country-specific: CIPO (Canada)
- **RED**: `test_cipo_scrape_ca_patent` — mock HTML; verify CA patent metadata
- **GREEN**: implement `CipoScrapeSource`

### T15 — Dead source graceful degradation (PatentsView)
- **RED**: `test_patentsview_returns_helpful_message` — if someone configures PatentsView; `PatentsViewSource.fetch()` → `SourceAttempt(success=False, error="PatentsView API was shut down March 20, 2026. Use USPTO ODP instead.")`
- **GREEN**: implement stub `PatentsViewSource` with helpful message

### T16 — Auth failure handling
- **RED**: `test_epo_ops_auth_failure_graceful` — mock auth endpoint returns 401; `EpoOpsSource.fetch()` → `SourceAttempt(success=False, error="EPO OPS auth failed: ...")`; no exception
- **GREEN**: catch auth failures; return SourceAttempt with error

---

## Rust Implementation

### T17 — Rust: PPUBS + EPO OPS in `fetchers/http_tests.rs`
- Mirror T02–T08 using `reqwest` async client
- Point to Python mock server via `source_base_urls`

### T18 — Rust: retry logic
- Mirror T10 using `reqwest` + custom retry loop (or `reqwest-retry` crate)

### T19 — Parity: Python SourceAttempt == Rust SourceAttempt
- `test_http_source_parity` — same fixture ID, same mock server → JSON outputs match

---

## Acceptance Criteria
- All HTTP calls mocked; unit tests run in <200ms
- Each source returns `SourceAttempt` with same schema regardless of success/failure
- Retry logic tested explicitly
- BigQuery missing → warning, not crash

## Dependencies
- `06-config`, `01-id-canon`, `07-test-infra` (mock server up)
- Python deps: `httpx[http2]`, `h2`, `lxml`, `beautifulsoup4`, `tenacity`, `google-cloud-bigquery` (optional)
