# INTERFACE — 03c-web-search-fallback

## Exposes
```python
from patent_mcp.fetchers.web_search import WebSearchFallbackSource

# WebSearchFallbackSource IS a BasePatentSource subclass
# BUT it uses the extended SourceAttempt with urls field (see below)

# Note: 03c never writes files to disk. SourceAttempt.formats_retrieved is always [].
# URLs are returned in SourceAttempt.metadata["urls"] as a list of WebSearchUrl dicts.

@dataclass
class WebSearchUrl:
    url: str
    title: str
    snippet: str
    confidence: str  # "high" | "medium" | "low"

# SourceAttempt returned by this source has metadata populated:
# SourceAttempt(
#   source_name="web_search_fallback",
#   success=True,      # True if any URLs found; False if no results
#   formats_retrieved=[],   # always empty
#   url=None,
#   metadata={"urls": [WebSearchUrl(...)], "query": "..."},
#   error=None
# )

def generate_queries(canonical_id: CanonicalPatentId) -> list[str]: ...
def score_url_confidence(url: str, canonical_id: str) -> str: ...
```

**Design note on SourceAttempt.metadata:** The `metadata` field on `SourceAttempt` is
`dict | None` — this is where 03c stores URLs. The `metadata` field is intentionally a
flexible dict for source-specific extra data. See 03-source-fetchers INTERFACE.md for the
updated SourceAttempt definition that includes `metadata`.

## Rust Equivalents
```rust
pub struct WebSearchFallbackSource { ... }
pub fn generate_queries(id: &CanonicalPatentId) -> Vec<String>;
pub fn score_url_confidence(url: &str, canonical_id: &str) -> &'static str;
```

## Depends On
01-id-canon, 06-config

## Consumed By
03-source-fetchers (orchestrator) — called last, after all structured sources fail
