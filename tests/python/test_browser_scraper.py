"""Tests for browser-based patent scrapers (03b — Google Patents).

Uses HTML fixtures (PATENT_PLAYWRIGHT_MOCK_DIR) so no real browser is needed.
Browser tests that require Playwright are marked @pytest.mark.browser.
"""
from __future__ import annotations

import os
import pytest
from pathlib import Path

FIXTURES_DIR = Path(__file__).parent.parent / "fixtures" / "browser"


@pytest.fixture
def mock_env(tmp_path, monkeypatch):
    monkeypatch.setenv("PATENT_PLAYWRIGHT_MOCK_DIR", str(FIXTURES_DIR))
    return tmp_path


class TestGooglePatentsFixtureParsing:
    def test_fixture_html_is_valid_html(self):
        """T07b — HTML fixture must exist and be parseable."""
        html_file = FIXTURES_DIR / "google_patents" / "US7654321.html"
        assert html_file.exists(), f"Fixture missing: {html_file}"
        html = html_file.read_text()
        assert "application/ld+json" in html
        assert "patent" in html.lower()

    def test_json_ld_in_fixture_is_valid(self):
        """T07b — JSON-LD in fixture must be valid JSON."""
        import json, re
        html = (FIXTURES_DIR / "google_patents" / "US7654321.html").read_text()
        match = re.search(
            r'<script[^>]+type=["\']application/ld\+json["\'][^>]*>(.*?)</script>',
            html, re.DOTALL | re.IGNORECASE
        )
        assert match, "No JSON-LD script tag found"
        ld = json.loads(match.group(1))
        assert "name" in ld or "headline" in ld, "JSON-LD missing name/headline"
        assert "inventor" in ld or "inventors" in ld, "JSON-LD missing inventors"

    def test_fetch_from_fixture_returns_metadata(self, mock_env, tmp_path):
        """Fixture fetch must return title and inventors."""
        from patent_mcp.scrapers.google_patents import fetch
        result = fetch("US7654321", tmp_path / "US7654321")
        assert result.success is True
        assert result.title == "Synthetic Test Patent for Unit Testing"
        assert "Alice Inventor" in result.inventors
        assert "Bob Inventor" in result.inventors
        assert result.assignee == "Test Assignee Corp"
        assert result.filing_date == "2008-06-15"

    def test_fetch_missing_fixture_returns_failure(self, mock_env, tmp_path):
        """Missing fixture returns failure, not exception."""
        from patent_mcp.scrapers.google_patents import fetch
        result = fetch("US9999999_NONEXISTENT", tmp_path / "out")
        assert result.success is False
        assert result.error is not None

    def test_fetch_result_fields_present(self, mock_env, tmp_path):
        """Result object must have all required fields."""
        from patent_mcp.scrapers.google_patents import fetch, BrowserFetchResult
        import dataclasses
        result = fetch("US7654321", tmp_path / "out")
        assert isinstance(result, BrowserFetchResult)
        fields = {f.name for f in dataclasses.fields(result)}
        required = {"canonical_id", "success", "source", "title", "abstract",
                    "inventors", "assignee", "filing_date", "elapsed_ms", "error"}
        assert required <= fields


class TestGooglePatentsMockDir:
    def test_mock_dir_env_var_respected(self, monkeypatch, tmp_path):
        """PATENT_PLAYWRIGHT_MOCK_DIR must be checked before launching browser."""
        monkeypatch.setenv("PATENT_PLAYWRIGHT_MOCK_DIR", str(FIXTURES_DIR))
        from patent_mcp.scrapers.google_patents import fetch
        # Should not attempt to import playwright
        result = fetch("US7654321", tmp_path)
        assert result.source == "google_patents"


@pytest.mark.browser
class TestGooglePatentsRealBrowser:
    def test_playwright_import_available(self):
        """Playwright package is importable (requires: pip install playwright)."""
        try:
            from playwright.sync_api import sync_playwright
        except ImportError:
            pytest.skip("playwright not installed")
