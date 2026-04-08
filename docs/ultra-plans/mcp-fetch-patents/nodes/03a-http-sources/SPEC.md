# SPEC — 03a-http-sources: HTTP/API Patent Sources

## Responsibility
Fetch patent data from structured HTTP APIs and websites using standard HTTP requests (no browser required).

## Sources Covered

### ⚠️ Dead/Unavailable Sources (do NOT implement)
- **PatentsView API**: Shut down March 20, 2026; data migrated to USPTO ODP bulk datasets. Do not implement; include a stub that returns a helpful message if somehow configured.

---

### 1. USPTO Open Data Portal
- **URL pattern**: `https://developer.uspto.gov/ibd-api/v1/patent/...`
- **Patent Full-Text**: `https://efts.uspto.gov/LATEST/search-solr?...`
- **Bulk PDF**: USPTO bulk FTP / PatentCenter API
- **PatentsView API**: `https://api.patentsview.org/patents/query` (metadata, assignment, citations)
- **PAIR (status)**: `https://pairsandbox.uspto.gov/api/` (v2 — stub only in v1)
- **Auth**: No API key required for basic access; rate limit: polite requests
- **Jurisdictions**: US only
- **Formats returned**: PDF, TXT (full text), XML (SGML/XML patent data)
- **Key endpoint for docs**: `https://developer.uspto.gov/`

### 2. EPO Open Patent Services (OPS) API
- **Base URL**: `https://ops.epo.org/3.2/rest-services/`
- **Endpoints**: `published-data/publication/{format}/{patent-id}`, `biblio`, `fulltext`, `images`
- **Auth**: OAuth2 (client credentials); free tier: 4GB/week quota
- **No-auth scraping**: EPO Espacenet web can be scraped as fallback
- **Jurisdictions**: EP (primary), WO, and ~80 countries via exchange data (AT, BE, CH, DE, DK, ES, FI, FR, GB, IT, NL, NO, SE, + many more)
- **Formats**: XML (DOCDB), PDF
- **Key docs**: https://www.epo.org/en/searching-for-patents/data/web-services/ops
- **Getting keys**: https://developers.epo.org/ (free registration)

### 3. Google Patents BigQuery (multi-national, optional)
- **Dataset**: `patents-public-data.patents` on Google BigQuery
- **Auth**: Google Cloud service account JSON or Application Default Credentials
- **Free tier**: 1TB/month of queries (sufficient for moderate use)
- **Coverage**: 90M+ publications, 17+ countries, US full text + global bibliographic
- **Getting started**: Requires Google Cloud project at `console.cloud.google.com`
- **Jurisdictions**: US (full text), EP, WO, JP, KR, DE, FR, GB, CN, CA, AU, and more
- **Formats**: SQL → JSON (bibliographic + US full text); PDF links available
- **Python**: `google-cloud-bigquery` package
- **Key advantage**: Single integration covers 17+ national offices
- **Note**: Optional source — graceful degradation if not configured

### 4. WIPO PatentScope (web scraping only — API is paywalled)
- **Web URL**: `https://patentscope.wipo.int/search/en/`
- **IMPORTANT**: WIPO PatentScope API requires a paid subscription (600 CHF/year) — do NOT implement as an API client
- **Scraping**: PatentScope web pages accessible without auth; use httpx + Playwright fallback
- **Jurisdictions**: WO (PCT) primarily, also submissions from JP, KR, AU, CN
- **Formats**: PDF download links, HTML full text

### 5. Espacenet (EPO web scraping)
- **URL**: `https://worldwide.espacenet.com/patent/...`
- **No API key required for scraping**
- **Jurisdictions**: Worldwide (very broad via EPO exchange data)
- **Formats**: PDF, bibliographic data (HTML scrape)

### 6. Lens.org (web scraping only — API requires manual approval)
- **IMPORTANT**: Lens.org API requires manual approval + 14-day trial then paid — do NOT implement as API client for v1
- **Web scraping**: Public patent pages accessible without auth
- **Jurisdictions**: Worldwide (140M+ records, 160+ countries)
- **Implement as**: HTTP scraping target (no API key needed for basic web access)

### 6. Country-Specific Sources (HTTP)
Each implemented as a separate fetcher class/module:

| Country | Source | URL | Auth | Notes |
|---|---|---|---|---|
| Japan | J-PlatPat | https://www.j-platpat.inpit.go.jp/ | No | Scraping required |
| China | CNIPA / Patentics | https://pss-system.cponline.cnipa.gov.cn/ | No | Scraping |
| Korea | KIPRIS | https://www.kipris.or.kr/ | No | English available |
| Australia | IP Australia AusPat | https://pericles.ipaustralia.gov.au/ols/auspat/ | No | REST API |
| Canada | CIPO | https://ised-isde.canada.ca/cipo/patent | No | Scraping |
| New Zealand | IPONZ | https://www.iponz.govt.nz/ | No | Scraping |
| Brazil | BRPTO / Inpi | https://busca.inpi.gov.br/ | No | Scraping |
| India | IP India | https://ipindiaservices.gov.in/ | No | Scraping |
| Germany | DPMA | https://www.dpma.de/ | No | Scraping / OPS |
| UK | IPO | https://www.ipo.gov.uk/ | No | Scraping |
| Gulf (GCC) | GCC Patent Office | https://www.gcpo.org/ | No | Scraping |

## HTTP Client Behavior
- Use `httpx` (Python async) / `reqwest` (Rust async)
- Respect `Retry-After` headers
- Exponential backoff with jitter: 1s, 2s, 4s, max 3 retries
- User-agent: `patent-mcp-fetcher/1.0 (https://github.com/...) - research bot`
- Configurable request timeout: default 30s
- Follow redirects: up to 5

## API Key Documentation
Each source's key:
- Where to register
- What tier/quota is free
- What env var to set (`PATENT_{SOURCE}_KEY`)
- What `~/.patents.toml` key to use (`[sources.{source}] api_key = "..."`)
Documented in `docs/api-keys.md` (generated during implementation).

## Scraping Without API Keys
For all sources requiring API keys, implement a scraping fallback:
- Use plain httpx for non-JS pages
- For JS-rendered pages, delegate to `03b-browser-sources`
- Clearly mark scraped results as such in `sources.json`
- Used for testing without keys; may be rate-limited in production

## Dependencies
- `01-id-canon`, `02-cache-db`, `06-config`
- `httpx` (Python), `reqwest` (Rust)

## Test Surface
- Unit: each fetcher tested in isolation against mock HTTP server
- Each source has at least one fixture per supported jurisdiction
- Retry logic tested with mock server returning 429/503
- Auth failure tested: graceful error, not crash
