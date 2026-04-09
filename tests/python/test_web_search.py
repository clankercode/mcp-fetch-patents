"""Tests for patent_mcp.fetchers.web_search — T01-T06."""

from __future__ import annotations

from pathlib import Path
from unittest.mock import patch

import pytest

pytest.importorskip("httpx")
pytest.importorskip("respx")

import httpx
import respx

from patent_mcp.config import load_config
from patent_mcp.fetchers.web_search import (
    DuckDuckGoSearchBackend,
    WebSearchFallbackSource,
    generate_queries,
    score_url_confidence,
)
from patent_mcp.id_canon import canonicalize


def _cfg(**overrides):
    cfg = load_config(env={})
    for k, v in overrides.items():
        setattr(cfg, k, v)
    return cfg


def _us():
    return canonicalize("US7654321")


def _ep():
    return canonicalize("EP1234567")


def _wo():
    return canonicalize("WO2024123456")


# ---------------------------------------------------------------------------
# T01 — Query generation
# ---------------------------------------------------------------------------


class TestQueryGeneration:
    def test_query_generation_us(self):
        queries = generate_queries(_us())
        assert any("US7654321" in q for q in queries)
        assert any("PDF" in q or "full text" in q for q in queries)

    def test_query_generation_ep(self):
        queries = generate_queries(_ep())
        assert any("EP1234567" in q for q in queries)
        assert any("epo.org" in q or "European" in q or "EP" in q for q in queries)

    def test_query_generation_wo(self):
        queries = generate_queries(_wo())
        assert any("WO" in q or "PCT" in q for q in queries)

    def test_queries_are_list(self):
        queries = generate_queries(_us())
        assert isinstance(queries, list)
        assert len(queries) >= 2


# ---------------------------------------------------------------------------
# T02 — URL confidence scoring
# ---------------------------------------------------------------------------


class TestUrlConfidence:
    def test_confidence_high_when_id_in_url(self):
        url = "https://patents.google.com/patent/US7654321"
        assert score_url_confidence(url, "US7654321") == "high"

    def test_confidence_high_when_known_domain(self):
        url = "https://patents.google.com/patent/US9876543"
        assert score_url_confidence(url, "US7654321") == "high"

    def test_confidence_medium_when_patent_in_domain(self):
        url = "https://somepatent.example.com/US9876543"
        result = score_url_confidence(url, "US7654321")
        assert result in ("medium", "low")

    def test_confidence_low_generic(self):
        url = "https://example.com/some-page"
        result = score_url_confidence(url, "US7654321")
        assert result == "low"


# ---------------------------------------------------------------------------
# T03 — DuckDuckGo backend
# ---------------------------------------------------------------------------


class TestDuckDuckGoBackend:
    @pytest.mark.asyncio
    async def test_ddg_api_called(self):
        cfg = _cfg(source_base_urls={"DDG": "http://mock-ddg/"})
        ddg = DuckDuckGoSearchBackend(cfg)

        ddg_resp = {
            "Results": [{"FirstURL": "https://patents.google.com/patent/US7654321"}],
            "RelatedTopics": [],
        }

        with respx.mock:
            respx.get("http://mock-ddg/").mock(
                return_value=httpx.Response(200, json=ddg_resp)
            )
            urls = await ddg.search("US7654321 patent PDF")

        assert len(urls) >= 1
        assert "patents.google.com" in urls[0]

    @pytest.mark.asyncio
    async def test_ddg_empty_results_no_exception(self):
        cfg = _cfg(source_base_urls={"DDG": "http://mock-ddg/"})
        ddg = DuckDuckGoSearchBackend(cfg)

        with respx.mock:
            respx.get("http://mock-ddg/").mock(
                return_value=httpx.Response(
                    200, json={"Results": [], "RelatedTopics": []}
                )
            )
            urls = await ddg.search("US7654321 patent")

        assert urls == []


# ---------------------------------------------------------------------------
# T04 — Result assembly
# ---------------------------------------------------------------------------


class TestWebSearchFallbackResult:
    @pytest.mark.asyncio
    async def test_fallback_result_schema(self, tmp_path):
        cfg = _cfg(source_base_urls={"DDG": "http://mock-ddg/"})
        src = WebSearchFallbackSource(cfg)

        with respx.mock:
            respx.get("http://mock-ddg/").mock(
                return_value=httpx.Response(
                    200,
                    json={
                        "Results": [
                            {"FirstURL": "https://patents.google.com/patent/US7654321"}
                        ],
                        "RelatedTopics": [],
                    },
                )
            )
            result = await src.fetch(_us(), tmp_path)

        assert result.source_attempt.source == "web_search"
        assert result.source_attempt.success is True
        assert result.source_attempt.metadata is not None
        assert "urls" in result.source_attempt.metadata

    @pytest.mark.asyncio
    async def test_fallback_note_in_result(self, tmp_path):
        cfg = _cfg(source_base_urls={"DDG": "http://mock-ddg/"})
        src = WebSearchFallbackSource(cfg)

        with respx.mock:
            respx.get("http://mock-ddg/").mock(
                return_value=httpx.Response(
                    200, json={"Results": [], "RelatedTopics": []}
                )
            )
            result = await src.fetch(_us(), tmp_path)

        meta = result.source_attempt.metadata
        assert meta is not None
        assert "note" in meta
        assert "fallback" in meta["note"].lower()


# ---------------------------------------------------------------------------
# T05 — SerpAPI backend
# ---------------------------------------------------------------------------


class TestSerpApiBackend:
    @pytest.mark.asyncio
    async def test_serpapi_skipped_when_no_key(self, tmp_path):
        cfg = _cfg(serpapi_key=None, source_base_urls={"DDG": "http://mock-ddg/"})
        src = WebSearchFallbackSource(cfg)

        call_count = 0

        def ddg_handler(request):
            nonlocal call_count
            call_count += 1
            return httpx.Response(200, json={"Results": [], "RelatedTopics": []})

        with respx.mock:
            respx.get("http://mock-ddg/").mock(side_effect=ddg_handler)
            result = await src.fetch(_us(), tmp_path)

        # SerpAPI should not have been called (no key)
        assert result.source_attempt.success is True


# ---------------------------------------------------------------------------
# T06 — Never writes files
# ---------------------------------------------------------------------------


class TestFallbackNoArtifacts:
    @pytest.mark.asyncio
    async def test_fallback_returns_no_artifacts(self, tmp_path):
        cfg = _cfg(source_base_urls={"DDG": "http://mock-ddg/"})
        src = WebSearchFallbackSource(cfg)

        with respx.mock:
            respx.get("http://mock-ddg/").mock(
                return_value=httpx.Response(
                    200, json={"Results": [], "RelatedTopics": []}
                )
            )
            result = await src.fetch(_us(), tmp_path)

        # No files should be written
        assert result.pdf_path is None
        assert result.txt_path is None
        assert result.image_urls == []
        # formats_retrieved should be empty
        meta = result.source_attempt.metadata or {}
        assert meta.get("formats_retrieved", []) == []
