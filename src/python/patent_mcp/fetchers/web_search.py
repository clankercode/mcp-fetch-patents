"""Web search fallback — last resort when all structured sources fail."""
from __future__ import annotations

import logging
import time
from pathlib import Path
from typing import TYPE_CHECKING

import httpx

from patent_mcp.cache import SourceAttempt
from patent_mcp.fetchers.base import BasePatentSource, FetchResult

if TYPE_CHECKING:
    from patent_mcp.config import PatentConfig
    from patent_mcp.id_canon import CanonicalPatentId

log = logging.getLogger(__name__)

# Known high-confidence patent domains
_HIGH_CONFIDENCE_DOMAINS = {
    "patents.google.com",
    "ppubs.uspto.gov",
    "ops.epo.org",
    "patentscope.wipo.int",
    "worldwide.espacenet.com",
    "patents.justia.com",
    "lens.org",
    "freepatentsonline.com",
}

_MEDIUM_CONFIDENCE_DOMAINS = {
    "patentyogi.com",
    "patent.ifixit.com",
}


# ---------------------------------------------------------------------------
# Query generation
# ---------------------------------------------------------------------------

def generate_queries(patent: "CanonicalPatentId") -> list[str]:
    """Generate a ranked list of search queries for a patent."""
    cid = patent.canonical
    queries = [f'"{cid}" patent PDF']

    if patent.jurisdiction == "US":
        queries += [
            f"{cid} patent full text",
            f"site:patents.google.com {cid}",
            f"site:ppubs.uspto.gov {cid}",
        ]
    elif patent.jurisdiction == "EP":
        queries += [
            f"{cid} patent European Patent Office",
            f"site:epo.org {cid}",
            f"site:worldwide.espacenet.com {cid}",
        ]
    elif patent.jurisdiction == "WO":
        queries += [
            f"{cid} PCT international patent",
            f"site:patentscope.wipo.int {cid}",
        ]
    else:
        queries += [
            f"{cid} patent full text PDF",
            f"{cid} {patent.jurisdiction} patent office",
        ]
    return queries


# ---------------------------------------------------------------------------
# URL confidence scoring
# ---------------------------------------------------------------------------

def score_url_confidence(url: str, canonical_id: str) -> str:
    """Score a URL's relevance to a patent: 'high', 'medium', or 'low'."""
    from urllib.parse import urlparse
    parsed = urlparse(url)
    domain = parsed.netloc.lower().lstrip("www.")

    # If the canonical ID appears in the URL path, it's high confidence
    if canonical_id.upper() in url.upper():
        return "high"
    if domain in _HIGH_CONFIDENCE_DOMAINS:
        return "high"
    if domain in _MEDIUM_CONFIDENCE_DOMAINS:
        return "medium"
    if "patent" in domain:
        return "medium"
    return "low"


# ---------------------------------------------------------------------------
# DuckDuckGo backend
# ---------------------------------------------------------------------------

class DuckDuckGoSearchBackend:
    """Search via DuckDuckGo Instant Answer API (no API key required)."""

    DDG_URL = "https://api.duckduckgo.com/"

    def __init__(self, config: "PatentConfig") -> None:
        self._config = config
        self._base = config.source_base_urls.get("DDG", self.DDG_URL)

    async def search(self, query: str) -> list[str]:
        """Return list of result URLs."""
        try:
            async with httpx.AsyncClient(timeout=15) as client:
                resp = await client.get(
                    self._base,
                    params={"q": query, "format": "json", "no_html": "1"},
                )
                resp.raise_for_status()
                data = resp.json()
                urls: list[str] = []
                for result in data.get("Results", []):
                    url = result.get("FirstURL") or result.get("url")
                    if url:
                        urls.append(url)
                for topic in data.get("RelatedTopics", []):
                    url = topic.get("FirstURL")
                    if url:
                        urls.append(url)
                return urls
        except Exception as e:
            log.debug("DDG search failed: %s", e)
            return []


# ---------------------------------------------------------------------------
# SerpAPI backend
# ---------------------------------------------------------------------------

class SerpApiSearchBackend:
    """Search via SerpAPI (requires API key)."""

    SERPAPI_URL = "https://serpapi.com/search"

    def __init__(self, config: "PatentConfig") -> None:
        self._config = config

    async def search(self, query: str) -> list[str]:
        if not self._config.serpapi_key:
            return []
        base = self._config.source_base_urls.get("SerpAPI", self.SERPAPI_URL)
        try:
            async with httpx.AsyncClient(timeout=20) as client:
                resp = await client.get(
                    base,
                    params={"q": query, "api_key": self._config.serpapi_key, "engine": "google"},
                )
                resp.raise_for_status()
                data = resp.json()
                return [r.get("link", "") for r in data.get("organic_results", []) if r.get("link")]
        except Exception as e:
            log.debug("SerpAPI search failed: %s", e)
            return []


# ---------------------------------------------------------------------------
# WebSearchFallbackSource
# ---------------------------------------------------------------------------

class WebSearchFallbackSource(BasePatentSource):
    """Last-resort web search; returns URLs only, never writes files."""

    @property
    def source_name(self) -> str:
        return "web_search"

    @property
    def supported_jurisdictions(self) -> frozenset[str]:
        return frozenset()  # handles all

    async def fetch(self, patent: "CanonicalPatentId", output_dir: Path) -> FetchResult:
        start = time.monotonic()
        attempt = SourceAttempt(source=self.source_name, success=False, elapsed_ms=0.0)
        try:
            queries = generate_queries(patent)
            ddg = DuckDuckGoSearchBackend(self._config)
            serpapi = SerpApiSearchBackend(self._config)

            all_urls: list[str] = []
            for q in queries[:2]:  # limit to 2 queries in fallback
                urls = await ddg.search(q)
                if not urls and self._config.serpapi_key:
                    urls = await serpapi.search(q)
                all_urls.extend(urls)

            # Deduplicate and score
            seen: set[str] = set()
            scored: list[dict] = []
            for url in all_urls:
                if url not in seen:
                    seen.add(url)
                    scored.append({
                        "url": url,
                        "confidence": score_url_confidence(url, patent.canonical),
                    })

            attempt.success = True  # web search always "succeeds" if it runs
            attempt.elapsed_ms = (time.monotonic() - start) * 1000
            attempt.metadata = {
                "urls": scored,
                "note": "Web search fallback — no structured sources returned results. "
                        "URLs returned for manual review or agent use.",
                "formats_retrieved": [],
            }
            return FetchResult(source_attempt=attempt)
        except Exception as e:
            attempt.elapsed_ms = (time.monotonic() - start) * 1000
            attempt.error = str(e)
            return FetchResult(source_attempt=attempt)
