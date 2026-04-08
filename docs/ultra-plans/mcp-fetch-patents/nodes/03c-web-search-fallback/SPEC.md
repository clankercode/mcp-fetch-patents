# SPEC — 03c-web-search-fallback: Web Search Last Resort

## Responsibility
When all structured patent sources fail to find a patent, perform a web search to find direct links to the patent document. Return URLs (not downloaded artifacts) as a last-resort result.

## Behavior
This is the **Tier 4** last resort. It:
1. Is only called when all Tier 1-3 sources have failed or returned no results
2. Does NOT download any files
3. Returns a list of URLs that likely point to the patent
4. Marks results clearly as "web_search_fallback" source type

## Search Queries
For a given canonical ID (e.g., `US7654321`), generate and execute queries:
```
"US7654321" patent PDF
US7654321 patent full text
US patent 7654321 filetype:pdf
```
For international:
```
"EP1234567" patent PDF site:epo.org
"WO2024/123456" patent
```

## Search Backends (in order)
1. **DuckDuckGo Instant Answer API** (free, no key): `https://api.duckduckgo.com/?q={query}&format=json`
2. **SerpAPI** (requires API key): `https://serpapi.com/search?q={query}&api_key={key}`
3. **Bing Web Search API** (requires Azure key): `https://api.bing.microsoft.com/v7.0/search`

## Result Schema
```json
{
  "canonical_id": "US7654321",
  "source": "web_search_fallback",
  "success": true,
  "urls": [
    {
      "url": "https://patents.google.com/patent/US7654321",
      "title": "US7654321B2 - Widget assembly",
      "snippet": "...",
      "confidence": "high"   // "high" if URL contains patent ID
    }
  ],
  "note": "Patent not found in structured sources. Web search results returned for manual or downstream processing.",
  "artifacts": {}   // no files downloaded
}
```

## URL Confidence Scoring
- `high`: URL contains the canonical patent ID string
- `medium`: URL is from a known patent database domain (patents.google.com, patents.justia.com, etc.)
- `low`: general web result

## Dependencies
- `01-id-canon`, `06-config`
- `httpx` (Python), `reqwest` (Rust)

## Test Surface
- Unit: query generation for each jurisdiction
- Unit: URL confidence scoring
- Mock: DuckDuckGo API returns canned JSON, verify URL extraction
