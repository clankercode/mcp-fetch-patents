"""Google Patents search via Playwright browser.

Navigates to Google Patents search result pages, parses result cards, and
paginates. Uses the BrowserManager for lifecycle (start once, idle shutdown).
"""
from __future__ import annotations

import logging
import re
import urllib.parse
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, Any

from patent_mcp.search.session_manager import PatentHit

if TYPE_CHECKING:
    from patent_mcp.search.browser_manager import BrowserManager

log = logging.getLogger(__name__)

# Base URL for Google Patents search
_SEARCH_BASE = "https://patents.google.com/"


# ---------------------------------------------------------------------------
# Config
# ---------------------------------------------------------------------------

@dataclass
class GoogleSearchConfig:
    max_pages: int = 3
    results_per_page: int = 10
    timeout_ms: float = 60_000
    debug_html_dir: Path | None = None


# ---------------------------------------------------------------------------
# Backend
# ---------------------------------------------------------------------------

class GooglePatentsBrowserBackend:
    """Search Google Patents by driving Chromium via Playwright."""

    def __init__(
        self,
        browser_manager: "BrowserManager",
        config: GoogleSearchConfig | None = None,
    ) -> None:
        self._bm = browser_manager
        self._cfg = config or GoogleSearchConfig()

    # ------------------------------------------------------------------
    # Public API
    # ------------------------------------------------------------------

    def search(
        self,
        query: str,
        date_before: str | None = None,  # YYYY-MM-DD or YYYYMMDD
        date_after: str | None = None,
        assignee: str | None = None,
        inventor: str | None = None,
        country: str | None = None,
        status: str | None = None,       # "GRANT" or "APPLICATION"
        max_results: int | None = None,
    ) -> list[PatentHit]:
        """Run a search on Google Patents and return parsed results.

        Fetches up to ``max_pages`` pages of results. Each page yields
        ~10 results (Google Patents default).
        """
        max_pages = self._cfg.max_pages
        if max_results:
            max_pages = min(max_pages, (max_results + 9) // 10)

        url = self._build_search_url(
            query, date_before, date_after, assignee, inventor, country, status,
        )

        page = self._bm.get_page()
        try:
            return self._execute_search(page, url, query, max_pages, max_results)
        finally:
            self._bm.release_page(page)

    # ------------------------------------------------------------------
    # URL construction
    # ------------------------------------------------------------------

    @staticmethod
    def _build_search_url(
        query: str,
        date_before: str | None = None,
        date_after: str | None = None,
        assignee: str | None = None,
        inventor: str | None = None,
        country: str | None = None,
        status: str | None = None,
        page: int = 0,
    ) -> str:
        """Construct a Google Patents search URL with filters."""
        params: dict[str, str] = {"q": query}

        if date_before:
            d = date_before.replace("-", "")
            params["before"] = f"priority:{d}"
        if date_after:
            d = date_after.replace("-", "")
            params["after"] = f"priority:{d}"
        if assignee:
            params["assignee"] = assignee
        if inventor:
            params["inventor"] = inventor
        if country:
            params["country"] = country
        if status:
            params["status"] = status.upper()
        if page > 0:
            params["page"] = str(page)

        return _SEARCH_BASE + "?" + urllib.parse.urlencode(params)

    # ------------------------------------------------------------------
    # Search execution
    # ------------------------------------------------------------------

    def _execute_search(
        self,
        page: Any,
        url: str,
        query: str,
        max_pages: int,
        max_results: int | None,
    ) -> list[PatentHit]:
        results: list[PatentHit] = []

        for page_num in range(max_pages):
            current_url = url if page_num == 0 else self._set_page_param(url, page_num)

            try:
                page.goto(current_url, wait_until="domcontentloaded")
                # Wait for result elements rather than full network idle
                try:
                    page.wait_for_selector(
                        'search-result-item, a[href*="/patent/"], .result-item',
                        timeout=15000,
                    )
                except Exception:
                    pass  # Proceed — may be a zero-results page
            except Exception as e:
                log.warning("Failed to navigate to %s: %s", current_url, e)
                break

            # Optional: save debug HTML
            if self._cfg.debug_html_dir:
                self._save_debug_html(page, query, page_num)

            page_results = self._parse_results_page(page, query, page_num)
            if not page_results:
                break
            results.extend(page_results)

            if max_results and len(results) >= max_results:
                results = results[:max_results]
                break

            # Check if there's a next page
            if not self._has_next_page(page):
                break

        return results

    # ------------------------------------------------------------------
    # Result parsing
    # ------------------------------------------------------------------

    def _parse_results_page(
        self, page: Any, query: str, page_num: int,
    ) -> list[PatentHit]:
        """Parse search result items from the current page."""
        hits: list[PatentHit] = []

        # Strategy 1: find result links containing /patent/ path
        # Google Patents renders search results as cards with links to /patent/<ID>/
        try:
            result_elements = page.query_selector_all("search-result-item, .result-item, article")
            if result_elements:
                hits = self._parse_result_elements(result_elements, query, page_num)
                if hits:
                    return hits
        except Exception as e:
            log.debug("Strategy 1 (structured elements) failed: %s", e)

        # Strategy 2: find all links matching /patent/<ID>/ pattern
        try:
            links = page.query_selector_all('a[href*="/patent/"]')
            hits = self._parse_patent_links(links, query, page_num)
            if hits:
                return hits
        except Exception as e:
            log.debug("Strategy 2 (patent links) failed: %s", e)

        # Strategy 3: extract from full page text as last resort
        try:
            text = page.inner_text("body")
            hits = self._extract_from_text(text, query, page_num)
        except Exception as e:
            log.debug("Strategy 3 (text extraction) failed: %s", e)

        return hits

    def _parse_result_elements(
        self, elements: list[Any], query: str, page_num: int,
    ) -> list[PatentHit]:
        """Parse structured result elements (e.g., <search-result-item>)."""
        hits: list[PatentHit] = []
        seen_ids: set[str] = set()

        for rank, el in enumerate(elements):
            try:
                # Find the patent link within this element
                link = el.query_selector('a[href*="/patent/"]')
                if not link:
                    continue

                href = link.get_attribute("href") or ""
                patent_id = self._extract_patent_id_from_url(href)
                if not patent_id or patent_id in seen_ids:
                    continue
                seen_ids.add(patent_id)

                # Extract text fields
                title = self._get_text(el, "h3, .title, [id*=title]") or link.inner_text().strip()
                snippet = self._get_text(el, ".abstract, .snippet, [id*=abstract]")
                assignee = self._get_text(el, ".assignee, [id*=assignee]")
                date_str = self._get_text(el, ".date, time, [id*=date]")
                inventors_text = self._get_text(el, ".inventor, [id*=inventor]")

                inventors = []
                if inventors_text:
                    inventors = [i.strip() for i in inventors_text.split(",") if i.strip()]

                url = f"https://patents.google.com/patent/{patent_id}/en"

                hits.append(PatentHit(
                    patent_id=patent_id,
                    title=_clean_text(title),
                    date=date_str,
                    assignee=_clean_text(assignee),
                    inventors=inventors,
                    abstract=_clean_text(snippet),
                    source="Google_Patents_Browser",
                    relevance="unknown",
                    url=url,
                ))
            except Exception as e:
                log.debug("Failed to parse result element %d: %s", rank, e)
                continue

        return hits

    def _parse_patent_links(
        self, links: list[Any], query: str, page_num: int,
    ) -> list[PatentHit]:
        """Parse patent links found on the page."""
        hits: list[PatentHit] = []
        seen_ids: set[str] = set()

        for rank, link in enumerate(links):
            try:
                href = link.get_attribute("href") or ""
                patent_id = self._extract_patent_id_from_url(href)
                if not patent_id or patent_id in seen_ids:
                    continue
                seen_ids.add(patent_id)

                # Get the text of the link and its parent
                title = link.inner_text().strip()
                # Try to get surrounding context
                parent = link.evaluate_handle("el => el.closest('article, div, section, li')")
                snippet = None
                if parent:
                    try:
                        full_text = parent.inner_text()
                        # Remove the title from the full text to get the snippet
                        snippet = full_text.replace(title, "").strip()[:500]
                    except Exception:
                        pass

                url = f"https://patents.google.com/patent/{patent_id}/en"

                hits.append(PatentHit(
                    patent_id=patent_id,
                    title=_clean_text(title) if title else None,
                    abstract=_clean_text(snippet) if snippet else None,
                    source="Google_Patents_Browser",
                    relevance="unknown",
                    url=url,
                ))
            except Exception as e:
                log.debug("Failed to parse link %d: %s", rank, e)
                continue

        return hits

    def _extract_from_text(
        self, text: str, query: str, page_num: int,
    ) -> list[PatentHit]:
        """Last-resort: extract patent IDs from page text using regex."""
        hits: list[PatentHit] = []
        seen: set[str] = set()

        # Match patterns like US1234567, EP1234567, WO2024123456, etc.
        pattern = r"\b([A-Z]{2}\d{5,12}[A-Z]?\d?)\b"
        for m in re.finditer(pattern, text):
            pid = m.group(1)
            if pid not in seen and len(pid) >= 7:
                seen.add(pid)
                hits.append(PatentHit(
                    patent_id=pid,
                    source="Google_Patents_Browser",
                    relevance="unknown",
                    url=f"https://patents.google.com/patent/{pid}/en",
                ))

        return hits

    # ------------------------------------------------------------------
    # Helpers
    # ------------------------------------------------------------------

    @staticmethod
    def _extract_patent_id_from_url(url: str) -> str | None:
        """Extract patent ID from a Google Patents URL path."""
        m = re.search(r"/patent/([A-Z]{2}[A-Z0-9/-]+?)(?:/|$)", url)
        if m:
            raw = m.group(1)
            # Strip language suffix like /en
            raw = re.sub(r"/[a-z]{2}$", "", raw)
            return raw
        return None

    @staticmethod
    def _get_text(element: Any, selector: str) -> str | None:
        """Query a child element and return its text, or None."""
        try:
            child = element.query_selector(selector)
            if child:
                text = child.inner_text().strip()
                return text if text else None
        except Exception:
            pass
        return None

    @staticmethod
    def _set_page_param(url: str, page_num: int) -> str:
        """Set or replace the 'page' query parameter in a URL."""
        parsed = urllib.parse.urlparse(url)
        params = urllib.parse.parse_qs(parsed.query)
        params["page"] = [str(page_num)]
        new_query = urllib.parse.urlencode(params, doseq=True)
        return urllib.parse.urlunparse(parsed._replace(query=new_query))

    @staticmethod
    def _has_next_page(page: Any) -> bool:
        """Check if a 'next page' navigation element exists."""
        try:
            # Look for specific next-page navigation, not any link with "page="
            next_btn = page.query_selector(
                '[aria-label="Next"], [aria-label="Next page"], '
                'a.next, button.next, nav a[href*="page="]'
            )
            return next_btn is not None
        except Exception:
            return False

    def _save_debug_html(self, page: Any, query: str, page_num: int) -> None:
        """Save page HTML for parser debugging/repair."""
        try:
            d = self._cfg.debug_html_dir
            if d is None:
                return
            d.mkdir(parents=True, exist_ok=True)
            slug = re.sub(r"[^a-z0-9]+", "-", query.lower())[:40]
            path = d / f"search-{slug}-p{page_num}.html"
            html = page.content()
            path.write_text(html, encoding="utf-8")
            log.debug("Saved debug HTML to %s", path)
        except Exception as e:
            log.debug("Failed to save debug HTML: %s", e)


# ---------------------------------------------------------------------------
# Text utilities
# ---------------------------------------------------------------------------

def _clean_text(text: str | None) -> str | None:
    """Collapse whitespace and strip."""
    if not text:
        return None
    text = re.sub(r"\s+", " ", text).strip()
    return text if text else None
