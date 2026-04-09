"""Google Patents browser scraper.

Uses Playwright (headless Chromium) to fetch patent pages that require JS rendering.
Falls back to HTML fixture files when PATENT_PLAYWRIGHT_MOCK_DIR is set (for tests).

Entry point for subprocess calls:
    python3 -m patent_mcp.scrapers.google_patents <canonical_id> <output_dir>

Output: single JSON line on stdout describing the SourceResult.
"""

from __future__ import annotations

import json
import os
import sys
import time
from dataclasses import dataclass, field, asdict
from pathlib import Path
from typing import Optional

import httpx

DEFAULT_USER_AGENT = (
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 "
    "(KHTML, like Gecko) Chrome/137.0.0.0 Safari/537.36 patent-mcp-server/0.1"
)


# ---------------------------------------------------------------------------
# Result type
# ---------------------------------------------------------------------------


@dataclass
class BrowserFetchResult:
    canonical_id: str
    success: bool
    source: str = "google_patents"
    pdf_path: Optional[str] = None
    txt_path: Optional[str] = None
    title: Optional[str] = None
    abstract: Optional[str] = None
    inventors: list[str] = field(default_factory=list)
    assignee: Optional[str] = None
    filing_date: Optional[str] = None
    publication_date: Optional[str] = None
    elapsed_ms: float = 0.0
    error: Optional[str] = None


# ---------------------------------------------------------------------------
# Mock mode (test fixtures)
# ---------------------------------------------------------------------------


def _fetch_from_fixture(canonical_id: str, output_dir: Path) -> BrowserFetchResult:
    """Load patent HTML fixture instead of launching a real browser."""
    mock_dir = Path(os.environ["PATENT_PLAYWRIGHT_MOCK_DIR"])
    html_file = mock_dir / "google_patents" / f"{canonical_id}.html"
    if not html_file.exists():
        return BrowserFetchResult(
            canonical_id=canonical_id,
            success=False,
            error=f"Fixture not found: {html_file}",
        )

    html = html_file.read_text(encoding="utf-8")
    return _parse_google_patents_html(canonical_id, html, output_dir)


def _parse_google_patents_html(
    canonical_id: str, html: str, output_dir: Path
) -> BrowserFetchResult:
    """Parse Google Patents HTML — extracts JSON-LD metadata."""
    import re

    result = BrowserFetchResult(canonical_id=canonical_id, success=False)

    # Extract JSON-LD
    ld_match = re.search(
        r'<script[^>]+type=["\']application/ld\+json["\'][^>]*>(.*?)</script>',
        html,
        re.DOTALL | re.IGNORECASE,
    )
    if not ld_match:
        result.error = "No JSON-LD found in HTML"
        return result

    try:
        ld = json.loads(ld_match.group(1))
    except json.JSONDecodeError as e:
        result.error = f"JSON-LD parse error: {e}"
        return result

    result.title = ld.get("name") or ld.get("headline")
    result.abstract = ld.get("description")
    result.assignee = (
        ld.get("assignee", {}).get("name")
        if isinstance(ld.get("assignee"), dict)
        else ld.get("assignee")
    )
    result.filing_date = ld.get("dateCreated") or ld.get("filingDate")
    result.publication_date = ld.get("datePublished")

    inventors_raw = ld.get("inventor", [])
    if isinstance(inventors_raw, list):
        result.inventors = [
            i.get("name", "") if isinstance(i, dict) else str(i) for i in inventors_raw
        ]
    elif isinstance(inventors_raw, dict):
        result.inventors = [inventors_raw.get("name", "")]

    result.success = bool(result.title)
    return result


# ---------------------------------------------------------------------------
# Real Playwright mode
# ---------------------------------------------------------------------------


def _fetch_with_playwright(canonical_id: str, output_dir: Path) -> BrowserFetchResult:
    """Fetch patent using a real Playwright headless browser."""
    try:
        from playwright.sync_api import sync_playwright
    except ImportError:
        return BrowserFetchResult(
            canonical_id=canonical_id,
            success=False,
            source="google_patents",
            error="Playwright not installed — run: pip install playwright && playwright install chromium",
        )

    url = f"https://patents.google.com/patent/{canonical_id}/en"
    t0 = time.monotonic()

    with sync_playwright() as p:
        try:
            browser = p.chromium.launch(headless=True)
            ctx = browser.new_context(
                user_agent=DEFAULT_USER_AGENT,
                viewport={"width": 1280, "height": 800},
            )
            page = ctx.new_page()
            page.goto(url, wait_until="networkidle", timeout=60_000)
            html = page.content()
            browser.close()
        except Exception as e:
            return BrowserFetchResult(
                canonical_id=canonical_id,
                success=False,
                source="google_patents",
                elapsed_ms=(time.monotonic() - t0) * 1000,
                error=f"Playwright navigation failed: {e}",
            )

    result = _parse_google_patents_html(canonical_id, html, output_dir)
    result.elapsed_ms = (time.monotonic() - t0) * 1000
    return result


# ---------------------------------------------------------------------------
# Public API
# ---------------------------------------------------------------------------


def fetch(canonical_id: str, output_dir: Path) -> BrowserFetchResult:
    """Fetch patent from Google Patents. Uses mock if PATENT_PLAYWRIGHT_MOCK_DIR is set."""
    output_dir.mkdir(parents=True, exist_ok=True)
    if os.environ.get("PATENT_PLAYWRIGHT_MOCK_DIR"):
        return _fetch_from_fixture(canonical_id, output_dir)
    return _fetch_with_playwright(canonical_id, output_dir)


# ---------------------------------------------------------------------------
# Subprocess entry point
# ---------------------------------------------------------------------------

if __name__ == "__main__":
    if len(sys.argv) < 3:
        print(
            json.dumps(
                {
                    "success": False,
                    "error": "Usage: google_patents.py <id> <output_dir>",
                }
            )
        )
        sys.exit(1)

    _id = sys.argv[1]
    _out = Path(sys.argv[2])
    _result = fetch(_id, _out)
    print(json.dumps(asdict(_result)))
