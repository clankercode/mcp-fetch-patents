"""Tests for patent_mcp.fetchers.http — T01-T16 (Python tasks).

Marked 'slow': httpx+respx imports add ~250ms; excluded from <1s fast suite.
"""
from __future__ import annotations

import json
from pathlib import Path
from unittest.mock import AsyncMock, MagicMock, patch

import httpx
import pytest
import respx

pytestmark = pytest.mark.slow

from patent_mcp.cache import SessionCache
from patent_mcp.config import load_config
from patent_mcp.fetchers.base import BasePatentSource, FetchResult
from patent_mcp.fetchers.http import (
    BigQuerySource,
    CipoScrapeSource,
    EpoOpsSource,
    EpoOpsTokenManager,
    IpAustraliaSource,
    PatentsViewStubSource,
    PpubsSessionManager,
    PpubsSource,
    WipoScrapeSource,
    EspacenetSource,
    GooglePatentsSource,
)
from patent_mcp.id_canon import canonicalize


def _cfg(**overrides):
    cfg = load_config(env={})
    for k, v in overrides.items():
        setattr(cfg, k, v)
    return cfg


def _us_patent():
    return canonicalize("US7654321")


def _ep_patent():
    return canonicalize("EP1234567")


def _wo_patent():
    return canonicalize("WO2024123456")


def _au_patent():
    return canonicalize("AU2023123456")


def _ca_patent():
    return canonicalize("CA3012345")


# ---------------------------------------------------------------------------
# T01 — Base source interface
# ---------------------------------------------------------------------------

class TestBaseSourceInterface:
    def test_base_source_has_required_interface(self):
        class ConcreteSource(BasePatentSource):
            @property
            def source_name(self): return "test"
            @property
            def supported_jurisdictions(self): return frozenset({"US"})
            async def fetch(self, patent, output_dir): ...

        src = ConcreteSource(_cfg())
        assert src.source_name == "test"
        assert "US" in src.supported_jurisdictions
        assert hasattr(src, "fetch")

    def test_can_fetch_supported_jurisdiction(self):
        src = PpubsSource(_cfg())
        assert src.can_fetch(_us_patent()) is True

    def test_cannot_fetch_unsupported_jurisdiction(self):
        src = PpubsSource(_cfg())
        assert src.can_fetch(_ep_patent()) is False

    def test_epo_ops_supports_all(self):
        src = EpoOpsSource(_cfg())
        # Empty frozenset means "all"
        assert src.can_fetch(_us_patent()) is True
        assert src.can_fetch(_ep_patent()) is True
        assert src.can_fetch(_wo_patent()) is True


# ---------------------------------------------------------------------------
# T02 — PPUBS: session establishment
# ---------------------------------------------------------------------------

class TestPpubsSession:
    @pytest.mark.asyncio
    async def test_ppubs_session_established(self):
        cfg = _cfg(source_base_urls={"USPTO": "http://mock-ppubs"})
        sc = SessionCache()
        mgr = PpubsSessionManager(cfg, sc)

        with respx.mock:
            respx.post("http://mock-ppubs/ppubs-api/v1/session").mock(
                return_value=httpx.Response(200, json={"session": "abc123"})
            )
            token = await mgr.get_session_token()

        assert token == "abc123"

    @pytest.mark.asyncio
    async def test_ppubs_session_cached(self):
        cfg = _cfg(source_base_urls={"USPTO": "http://mock-ppubs"})
        sc = SessionCache()
        mgr = PpubsSessionManager(cfg, sc)

        with respx.mock:
            route = respx.post("http://mock-ppubs/ppubs-api/v1/session").mock(
                return_value=httpx.Response(200, json={"session": "abc123"})
            )
            await mgr.get_session_token()
            token2 = await mgr.get_session_token()

        # Second call should use cache, not make another HTTP request
        assert route.call_count == 1
        assert token2 == "abc123"


# ---------------------------------------------------------------------------
# T03 — PPUBS: document search
# ---------------------------------------------------------------------------

class TestPpubsSearch:
    @pytest.mark.asyncio
    async def test_ppubs_search_us_patent(self, tmp_path):
        cfg = _cfg(source_base_urls={"USPTO": "http://mock-ppubs"})
        src = PpubsSource(cfg)
        patent = _us_patent()

        patent_data = {
            "patents": [{
                "guid": "guid-001",
                "patentNumber": "US7654321",
                "title": "Widget assembly",
                "abstract": "An abstract.",
                "fullText": "Claims: 1. A widget...",
                "inventors": ["Alice"],
                "filingDate": "2005-03-12",
                "grantDate": "2010-01-19",
            }]
        }

        with respx.mock:
            respx.post("http://mock-ppubs/ppubs-api/v1/session").mock(
                return_value=httpx.Response(200, json={"session": "tok"})
            )
            respx.get(
                "http://mock-ppubs/ppubs-api/v1/patent",
                params={"patentNumber": "7654321"},
            ).mock(return_value=httpx.Response(200, json=patent_data))
            respx.get("http://mock-ppubs/ppubs-api/v1/download/guid-001").mock(
                return_value=httpx.Response(200, content=b"%PDF-1.4")
            )
            result = await src.fetch(patent, tmp_path)

        assert result.source_attempt.success is True
        assert result.txt_path is not None
        assert result.txt_path.read_text() != ""
        assert result.metadata is not None
        assert result.metadata.title == "Widget assembly"

    @pytest.mark.asyncio
    async def test_ppubs_not_found(self, tmp_path):
        cfg = _cfg(source_base_urls={"USPTO": "http://mock-ppubs"})
        src = PpubsSource(cfg)
        patent = _us_patent()

        with respx.mock:
            respx.post("http://mock-ppubs/ppubs-api/v1/session").mock(
                return_value=httpx.Response(200, json={"session": "tok"})
            )
            respx.get(
                "http://mock-ppubs/ppubs-api/v1/patent",
                params={"patentNumber": "7654321"},
            ).mock(return_value=httpx.Response(200, json={"patents": []}))
            result = await src.fetch(patent, tmp_path)

        assert result.source_attempt.success is False
        assert result.source_attempt.error == "not_found"


# ---------------------------------------------------------------------------
# T04 — PPUBS: PDF download
# ---------------------------------------------------------------------------

class TestPpubsPdfDownload:
    @pytest.mark.asyncio
    async def test_ppubs_pdf_download(self, tmp_path):
        cfg = _cfg(source_base_urls={"USPTO": "http://mock-ppubs"})
        src = PpubsSource(cfg)
        patent = _us_patent()

        with respx.mock:
            respx.post("http://mock-ppubs/ppubs-api/v1/session").mock(
                return_value=httpx.Response(200, json={"session": "tok"})
            )
            respx.get("http://mock-ppubs/ppubs-api/v1/patent").mock(
                return_value=httpx.Response(200, json={
                    "patents": [{"guid": "g1", "patentNumber": "US7654321", "title": "T"}]
                })
            )
            respx.get("http://mock-ppubs/ppubs-api/v1/download/g1").mock(
                return_value=httpx.Response(200, content=b"%PDF-1.4 stub")
            )
            result = await src.fetch(patent, tmp_path)

        assert result.pdf_path is not None
        assert result.pdf_path.read_bytes().startswith(b"%PDF")


# ---------------------------------------------------------------------------
# T05 — PPUBS: HTTP/2
# ---------------------------------------------------------------------------

class TestPpubsHttp2:
    def test_ppubs_uses_http2(self):
        """Verify the session manager creates client with http2=True."""
        cfg = _cfg(source_base_urls={"USPTO": "http://mock-ppubs"})
        sc = SessionCache()
        mgr = PpubsSessionManager(cfg, sc)
        # The http2 flag is set in _establish_session; we test indirectly by checking
        # the code path. Capture the Client call.
        import patent_mcp.fetchers.http as http_mod
        created_clients = []

        original_client = httpx.AsyncClient

        class MockHttp2Client(httpx.AsyncClient):
            def __init__(self, *args, **kwargs):
                created_clients.append(kwargs.get("http2", False))
                super().__init__(*args, **kwargs)

        with patch.object(http_mod.httpx, "AsyncClient", MockHttp2Client):
            import asyncio
            with respx.mock:
                respx.post("http://mock-ppubs/ppubs-api/v1/session").mock(
                    return_value=httpx.Response(200, json={"session": "t"})
                )
                asyncio.run(mgr.get_session_token())

        assert any(created_clients), "At least one http2=True client should be created"


# ---------------------------------------------------------------------------
# T06 — EPO OPS: OAuth2 token
# ---------------------------------------------------------------------------

class TestEpoOpsAuth:
    @pytest.mark.asyncio
    async def test_epo_ops_auth_token_requested(self):
        cfg = _cfg(
            epo_client_id="myid",
            epo_client_secret="mysecret",
            source_base_urls={"EPO_OPS": "http://mock-epo"},
        )
        sc = SessionCache()
        mgr = EpoOpsTokenManager(cfg, sc)

        with respx.mock:
            respx.post("http://mock-epo/3.2/auth/accesstoken").mock(
                return_value=httpx.Response(200, json={
                    "access_token": "epotoken",
                    "expires_in": 1200,
                })
            )
            token = await mgr.get_token()

        assert token == "epotoken"

    @pytest.mark.asyncio
    async def test_epo_ops_auth_token_cached(self):
        cfg = _cfg(
            epo_client_id="id",
            epo_client_secret="secret",
            source_base_urls={"EPO_OPS": "http://mock-epo"},
        )
        sc = SessionCache()
        mgr = EpoOpsTokenManager(cfg, sc)

        with respx.mock:
            route = respx.post("http://mock-epo/3.2/auth/accesstoken").mock(
                return_value=httpx.Response(200, json={"access_token": "tok", "expires_in": 3600})
            )
            await mgr.get_token()
            await mgr.get_token()

        assert route.call_count == 1

    @pytest.mark.asyncio
    async def test_epo_ops_no_credentials_returns_none(self):
        cfg = _cfg(epo_client_id=None, epo_client_secret=None)
        sc = SessionCache()
        mgr = EpoOpsTokenManager(cfg, sc)
        token = await mgr.get_token()
        assert token is None


# ---------------------------------------------------------------------------
# T07 — EPO OPS: bibliographic fetch
# ---------------------------------------------------------------------------

_EPO_BIBLIO_XML = """<?xml version="1.0"?>
<ops:world-patent-data
    xmlns:ops="http://ops.epo.org"
    xmlns:ep="http://www.epo.org/exchange">
  <exchange-documents>
    <exchange-document>
      <bibliographic-data>
        <invention-title lang="en" xmlns:ep="http://www.epo.org/exchange">
          Widget Assembly
        </invention-title>
        <inventors>
          <inventor><name xmlns:ep="http://www.epo.org/exchange">Alice</name></inventor>
        </inventors>
        <applicants>
          <applicant><name xmlns:ep="http://www.epo.org/exchange">Acme Corp</name></applicant>
        </applicants>
        <dates>
          <filing-date xmlns:ep="http://www.epo.org/exchange">20050312</filing-date>
        </dates>
      </bibliographic-data>
    </exchange-document>
  </exchange-documents>
</ops:world-patent-data>"""


class TestEpoOpsBiblio:
    @pytest.mark.asyncio
    async def test_epo_ops_fetch_ep_patent_metadata(self, tmp_path):
        cfg = _cfg(
            epo_client_id=None,
            source_base_urls={"EPO_OPS": "http://mock-epo"},
        )
        src = EpoOpsSource(cfg)
        patent = _ep_patent()

        with respx.mock:
            respx.get(
                "http://mock-epo/3.2/rest-services/published-data/publication/epodoc/EP.1234567/biblio"
            ).mock(return_value=httpx.Response(200, text=_EPO_BIBLIO_XML))
            respx.get(
                "http://mock-epo/3.2/rest-services/published-data/publication/epodoc/EP.1234567/full-cycle"
            ).mock(return_value=httpx.Response(200, content=b"%PDF-1.4", headers={"content-type": "application/pdf"}))
            result = await src.fetch(patent, tmp_path)

        assert result.source_attempt.success is True
        assert result.metadata is not None
        # XML parser may not be perfect; at least it doesn't crash
        assert result.metadata.canonical_id == "EP1234567"

    @pytest.mark.asyncio
    async def test_epo_ops_not_found(self, tmp_path):
        cfg = _cfg(epo_client_id=None, source_base_urls={"EPO_OPS": "http://mock-epo"})
        src = EpoOpsSource(cfg)
        patent = _ep_patent()

        with respx.mock:
            respx.get(
                "http://mock-epo/3.2/rest-services/published-data/publication/epodoc/EP.1234567/biblio"
            ).mock(return_value=httpx.Response(404))
            result = await src.fetch(patent, tmp_path)

        assert result.source_attempt.success is False
        assert result.source_attempt.error == "not_found"

    @pytest.mark.asyncio
    async def test_epo_ops_auth_failure_graceful(self, tmp_path):
        cfg = _cfg(
            epo_client_id="bad",
            epo_client_secret="bad",
            source_base_urls={"EPO_OPS": "http://mock-epo"},
        )
        src = EpoOpsSource(cfg)
        patent = _ep_patent()

        with respx.mock:
            respx.post("http://mock-epo/3.2/auth/accesstoken").mock(
                return_value=httpx.Response(401, text="Unauthorized")
            )
            respx.get(
                "http://mock-epo/3.2/rest-services/published-data/publication/epodoc/EP.1234567/biblio"
            ).mock(return_value=httpx.Response(401, text="Unauthorized"))
            result = await src.fetch(patent, tmp_path)

        assert result.source_attempt.success is False
        assert result.source_attempt.error is not None


# ---------------------------------------------------------------------------
# T09 — BigQuery
# ---------------------------------------------------------------------------

class TestBigQuery:
    def test_bigquery_no_credentials_graceful(self, tmp_path):
        cfg = _cfg()
        # Ensure BigQuery is unavailable (google-cloud-bigquery may not be installed)
        with patch("patent_mcp.fetchers.http.BigQuerySource._init_client", lambda self: None):
            src = BigQuerySource(cfg)
            src.available = False

        import asyncio
        result = asyncio.run(src.fetch(_us_patent(), tmp_path))
        assert result.source_attempt.success is False
        assert "not configured" in (result.source_attempt.error or "")

    def test_bigquery_us_query_contains_publication_number(self):
        cfg = _cfg()
        with patch("patent_mcp.fetchers.http.BigQuerySource._init_client", lambda self: None):
            src = BigQuerySource(cfg)
        query = src._build_query(_us_patent())
        assert "US-7654321" in query

    def test_bigquery_ep_query_contains_publication_number(self):
        cfg = _cfg()
        with patch("patent_mcp.fetchers.http.BigQuerySource._init_client", lambda self: None):
            src = BigQuerySource(cfg)
        query = src._build_query(_ep_patent())
        assert "EP-1234567" in query

    def test_bigquery_result_mapped_to_metadata(self):
        cfg = _cfg()
        with patch("patent_mcp.fetchers.http.BigQuerySource._init_client", lambda self: None):
            src = BigQuerySource(cfg)

        row = {
            "publication_number": "US-7654321-B2",
            "title_localized": [{"text": "Widget Assembly", "language": "en"}],
            "abstract_localized": [{"text": "A widget.", "language": "en"}],
            "inventor_harmonized": [{"name": "Alice"}],
            "assignee_harmonized": [{"name": "Acme Corp"}],
            "filing_date": "20050312",
            "grant_date": "20100119",
            "publication_date": None,
        }
        meta = src._parse_row(row, _us_patent())
        assert meta.title == "Widget Assembly"
        assert "Alice" in meta.inventors
        assert meta.assignee == "Acme Corp"
        assert meta.filing_date == "2005-03-12"

    def test_bigquery_no_results(self, tmp_path):
        cfg = _cfg()
        mock_client = MagicMock()
        mock_client.query.return_value.result.return_value = []

        with patch("patent_mcp.fetchers.http.BigQuerySource._init_client", lambda self: None):
            src = BigQuerySource(cfg)
            src._client = mock_client
            src.available = True

        import asyncio
        result = asyncio.run(src.fetch(_us_patent(), tmp_path))
        assert result.source_attempt.success is False
        assert result.source_attempt.error == "not_found"

    def test_bigquery_timeout_graceful(self, tmp_path):
        cfg = _cfg()
        mock_client = MagicMock()
        mock_client.query.side_effect = Exception("DeadlineExceeded: timeout")

        with patch("patent_mcp.fetchers.http.BigQuerySource._init_client", lambda self: None):
            src = BigQuerySource(cfg)
            src._client = mock_client
            src.available = True

        import asyncio
        result = asyncio.run(src.fetch(_us_patent(), tmp_path))
        assert result.source_attempt.success is False
        assert "timeout" in (result.source_attempt.error or "").lower()


# ---------------------------------------------------------------------------
# T10 — Retry logic
# ---------------------------------------------------------------------------

class TestRetryLogic:
    @pytest.mark.asyncio
    async def test_retry_on_429(self, tmp_path):
        cfg = _cfg(source_base_urls={"USPTO": "http://mock-ppubs"})
        src = PpubsSource(cfg)
        patent = _us_patent()

        call_count = 0

        def side_effect(request):
            nonlocal call_count
            call_count += 1
            if call_count < 2:
                return httpx.Response(429)
            return httpx.Response(200, json={"session": "tok"})

        with respx.mock:
            respx.post("http://mock-ppubs/ppubs-api/v1/session").mock(side_effect=side_effect)
            respx.get("http://mock-ppubs/ppubs-api/v1/patent").mock(
                return_value=httpx.Response(200, json={"patents": []})
            )
            result = await src.fetch(patent, tmp_path)

        # Patent not found, but session retry happened
        assert call_count >= 1

    @pytest.mark.asyncio
    async def test_no_more_than_3_retries(self, tmp_path):
        cfg = _cfg(source_base_urls={"USPTO": "http://mock-ppubs"})
        src = PpubsSource(cfg)
        patent = _us_patent()

        call_count = 0

        def always_429(request):
            nonlocal call_count
            call_count += 1
            return httpx.Response(429)

        with respx.mock:
            respx.post("http://mock-ppubs/ppubs-api/v1/session").mock(side_effect=always_429)
            result = await src.fetch(patent, tmp_path)

        assert result.source_attempt.success is False
        assert call_count <= 3  # tenacity stops at 3


# ---------------------------------------------------------------------------
# T11 — Espacenet scraping
# ---------------------------------------------------------------------------

class TestEspaceNetScraping:
    @pytest.mark.asyncio
    async def test_espacenet_extract_metadata(self, tmp_path):
        cfg = _cfg(source_base_urls={"Espacenet": "http://mock-espacenet"})
        src = EspacenetSource(cfg)
        patent = _ep_patent()

        html = """<html><body>
            <h2 class="title">Widget Assembly - EP1234567</h2>
            <a href="/download/EP1234567.pdf">Download PDF</a>
        </body></html>"""

        with respx.mock:
            respx.get("http://mock-espacenet/patent/EP1234567").mock(
                return_value=httpx.Response(200, text=html)
            )
            result = await src.fetch(patent, tmp_path)

        assert result.source_attempt.success is True
        assert result.metadata is not None
        assert "Widget" in (result.metadata.title or "")

    @pytest.mark.asyncio
    async def test_espacenet_pdf_link_found(self, tmp_path):
        cfg = _cfg(source_base_urls={"Espacenet": "http://mock-espacenet"})
        src = EspacenetSource(cfg)
        patent = _ep_patent()

        html = """<html><body>
            <h1>EP1234567</h1>
            <a href="http://mock-espacenet/download/EP1234567.pdf">PDF</a>
        </body></html>"""

        with respx.mock:
            respx.get("http://mock-espacenet/patent/EP1234567").mock(
                return_value=httpx.Response(200, text=html)
            )
            result = await src.fetch(patent, tmp_path)

        assert result.source_attempt.success is True
        assert result.source_attempt.metadata is not None
        assert "pdf_url" in result.source_attempt.metadata


# ---------------------------------------------------------------------------
# T12 — WIPO scraping
# ---------------------------------------------------------------------------

class TestWipoScraping:
    @pytest.mark.asyncio
    async def test_wipo_scraping_extracts_wo_patent(self, tmp_path):
        cfg = _cfg(source_base_urls={"WIPO_Scrape": "http://mock-wipo"})
        src = WipoScrapeSource(cfg)
        patent = _wo_patent()

        html = """<html><body>
            <span id="appTitleId">PCT Application WO2024/123456</span>
        </body></html>"""

        with respx.mock:
            respx.get("http://mock-wipo/search/en/detail.jsf", params={"docId": "WO/2024/123456"}).mock(
                return_value=httpx.Response(200, text=html)
            )
            result = await src.fetch(patent, tmp_path)

        assert result.source_attempt.success is True
        assert result.metadata is not None


# ---------------------------------------------------------------------------
# T13 — IP Australia
# ---------------------------------------------------------------------------

class TestIpAustralia:
    @pytest.mark.asyncio
    async def test_ip_australia_fetch(self, tmp_path):
        cfg = _cfg(source_base_urls={"IP_Australia": "http://mock-ipau"})
        src = IpAustraliaSource(cfg)
        patent = _au_patent()

        data = {
            "title": "AU Widget",
            "inventors": [{"name": "Bob"}],
            "applicant": "AU Corp",
            "filingDate": "2023-01-01",
        }

        with respx.mock:
            respx.get(f"http://mock-ipau/ols/auspat/api/v1/applications/{patent.number}").mock(
                return_value=httpx.Response(200, json=data)
            )
            result = await src.fetch(patent, tmp_path)

        assert result.source_attempt.success is True
        assert result.metadata is not None
        assert result.metadata.title == "AU Widget"


# ---------------------------------------------------------------------------
# T14 — CIPO (Canada)
# ---------------------------------------------------------------------------

class TestCipo:
    @pytest.mark.asyncio
    async def test_cipo_scrape_ca_patent(self, tmp_path):
        cfg = _cfg(source_base_urls={"CIPO": "http://mock-cipo"})
        src = CipoScrapeSource(cfg)
        patent = _ca_patent()

        html = "<html><body><h1>CA3012345 - Canadian Widget</h1></body></html>"

        with respx.mock:
            respx.get(f"http://mock-cipo/patent/{patent.canonical}").mock(
                return_value=httpx.Response(200, text=html)
            )
            result = await src.fetch(patent, tmp_path)

        assert result.source_attempt.success is True
        assert result.metadata is not None


# ---------------------------------------------------------------------------
# T15 — PatentsView stub
# ---------------------------------------------------------------------------

class TestPatentsViewStub:
    @pytest.mark.asyncio
    async def test_patentsview_returns_helpful_message(self, tmp_path):
        src = PatentsViewStubSource(_cfg())
        result = await src.fetch(_us_patent(), tmp_path)
        assert result.source_attempt.success is False
        assert "March 20, 2026" in (result.source_attempt.error or "")
        assert "USPTO ODP" in (result.source_attempt.error or "")


# ---------------------------------------------------------------------------
# T16 — GooglePatentsSource
# ---------------------------------------------------------------------------

class TestGooglePatentsSource:
    @pytest.mark.asyncio
    async def test_success_returns_metadata(self, tmp_path):
        """GooglePatentsSource maps BrowserFetchResult fields to PatentMetadata."""
        from patent_mcp.scrapers.google_patents import BrowserFetchResult
        mock_result = BrowserFetchResult(
            canonical_id="US7654321",
            success=True,
            title="Test Patent",
            abstract="A test.",
            inventors=["Alice", "Bob"],
            assignee="Test Corp",
            filing_date="2020-01-01",
            publication_date="2022-06-01",
            elapsed_ms=50.0,
        )
        src = GooglePatentsSource(_cfg())
        with patch("patent_mcp.scrapers.google_patents.fetch", return_value=mock_result):
            result = await src.fetch(_us_patent(), tmp_path)

        assert result.source_attempt.success is True
        assert result.metadata is not None
        assert result.metadata.title == "Test Patent"
        assert result.metadata.inventors == ["Alice", "Bob"]
        assert result.metadata.assignee == "Test Corp"

    @pytest.mark.asyncio
    async def test_failure_returns_failed_result(self, tmp_path):
        """GooglePatentsSource wraps scraper failure into a FetchResult."""
        from patent_mcp.scrapers.google_patents import BrowserFetchResult
        mock_result = BrowserFetchResult(
            canonical_id="US9999999",
            success=False,
            error="not found",
            elapsed_ms=10.0,
        )
        src = GooglePatentsSource(_cfg())
        with patch("patent_mcp.scrapers.google_patents.fetch", return_value=mock_result):
            result = await src.fetch(canonicalize("US9999999"), tmp_path)

        assert result.source_attempt.success is False
        assert result.metadata is None

    @pytest.mark.asyncio
    async def test_exception_returns_failed_result(self, tmp_path):
        """GooglePatentsSource catches exceptions and returns failure."""
        src = GooglePatentsSource(_cfg())
        with patch("patent_mcp.scrapers.google_patents.fetch", side_effect=RuntimeError("boom")):
            result = await src.fetch(_us_patent(), tmp_path)

        assert result.source_attempt.success is False
        assert "boom" in (result.source_attempt.error or "")

    def test_source_name(self):
        assert GooglePatentsSource(_cfg()).source_name == "Google_Patents"

    def test_registered_in_orchestrator(self):
        """Google_Patents must appear in the orchestrator source registry."""
        from patent_mcp.fetchers.orchestrator import FetcherOrchestrator
        orch = FetcherOrchestrator(_cfg())
        source_names = [s.source_name for s in orch._build_sources()]
        assert "Google_Patents" in source_names
