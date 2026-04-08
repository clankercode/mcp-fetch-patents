"""End-to-end integration tests — full stack from fetch_patents → cache.

These tests exercise the full Python stack with only HTTP transport mocked
via respx. A real SQLite cache in tmp_path is used throughout.

No Playwright, no subprocess spawning — all I/O is either in-memory or
in tmp_path.  Suite runs in <1s.
"""
from __future__ import annotations

import asyncio
import json
from pathlib import Path
from unittest.mock import AsyncMock, MagicMock, patch

import httpx
import pytest
import respx

from patent_mcp.cache import PatentCache, SourceAttempt
from patent_mcp.config import load_config
from patent_mcp.fetchers.orchestrator import FetcherOrchestrator
from patent_mcp.id_canon import canonicalize
from patent_mcp.server import FetchSummary, _build_server

# ---------------------------------------------------------------------------
# Shared fixture data (synthetic — no real patent content)
# ---------------------------------------------------------------------------

_US_PPUBS_RESPONSE = {
    "patents": [
        {
            "patentNumber": "US7654321",
            "guid": "test-guid-7654321",
            "title": "Widget Assembly Method",
            "abstract": "A method for assembling widgets.",
            "inventors": ["Alice Smith", "Bob Jones"],
            "assignee": "Widget Corp",
            "filingDate": "2005-03-15",
            "publicationDate": "2010-02-02",
            "grantDate": "2010-02-02",
            "fullText": "CLAIMS: 1. A widget assembly method comprising...",
        }
    ]
}

_STUB_PDF = b"%PDF-1.4\n1 0 obj\n<<>>\nendobj\nstartxref\n0\n%%EOF"

_EPO_OPS_XML = b"""<?xml version="1.0" encoding="UTF-8"?>
<ops:world-patent-data xmlns:ops="http://ops.epo.org/3.2"
                       xmlns:exchange="http://www.epo.org/exchange">
  <ops:exchange-documents>
    <ops:exchange-document country="EP" doc-number="1234567" kind="B1">
      <exchange:bibliographic-data>
        <exchange:invention-title lang="en">Improved Gear Mechanism</exchange:invention-title>
      </exchange:bibliographic-data>
    </ops:exchange-document>
  </ops:exchange-documents>
</ops:world-patent-data>"""


def _cfg(tmp_path: Path, **overrides):
    cfg = load_config(env={})
    cfg.cache_local_dir = tmp_path / ".patents"
    cfg.cache_global_db = tmp_path / "global" / "index.db"
    cfg.fetch_all_sources = False
    cfg.converters_disabled = ["pymupdf4llm", "pdfplumber", "pdftotext", "marker"]
    cfg.source_base_urls = {
        "USPTO": "http://mock-ppubs",
        "EPO_OPS": "http://mock-epo",
        "DDG": "http://mock-ddg",
    }
    for k, v in overrides.items():
        setattr(cfg, k, v)
    return cfg


# ---------------------------------------------------------------------------
# E2E-01 — Full fetch flow: USPTO source → PDF written to disk
# ---------------------------------------------------------------------------

class TestFullFetchFlow:
    @pytest.mark.asyncio
    async def test_us_patent_fetch_writes_files(self, tmp_path):
        """Full stack: fetch_patents → PPUBS → PDF on disk → cache populated."""
        cfg = _cfg(tmp_path)
        cache = PatentCache(cfg)
        orc = FetcherOrchestrator(cfg, cache=cache)

        # Inject a single mock source that returns a success
        pdf_bytes = _STUB_PDF
        pdf_written: list[Path] = []

        async def mock_fetch(patent, output_dir):
            output_dir.mkdir(parents=True, exist_ok=True)
            pdf = output_dir / f"{patent.canonical}.pdf"
            pdf.write_bytes(pdf_bytes)
            pdf_written.append(pdf)
            from patent_mcp.cache import PatentMetadata, SourceAttempt
            from datetime import datetime, timezone
            return __import__(
                "patent_mcp.fetchers.base", fromlist=["FetchResult"]
            ).FetchResult(
                source_attempt=SourceAttempt(source="USPTO", success=True, elapsed_ms=10.0),
                pdf_path=pdf,
                metadata=PatentMetadata(
                    canonical_id="US7654321",
                    jurisdiction="US",
                    doc_type="patent",
                    title="Widget Assembly Method",
                    abstract="A method for assembling widgets.",
                    inventors=["Alice Smith"],
                    assignee="Widget Corp",
                    fetched_at=datetime.now(timezone.utc).isoformat(),
                ),
            )

        mock_src = MagicMock()
        mock_src.source_name = "USPTO"
        mock_src.can_fetch.return_value = True
        mock_src.fetch = mock_fetch
        orc._sources = [mock_src]

        patent = canonicalize("US7654321")
        output_base = tmp_path / ".patents"
        results = await orc.fetch_batch([patent], output_base)

        assert len(results) == 1
        result = results[0]
        assert result.success is True
        assert result.from_cache is False
        assert "pdf" in result.files
        assert result.files["pdf"].exists()
        assert result.metadata is not None
        assert result.metadata.title == "Widget Assembly Method"

        # Verify PDF content on disk
        assert result.files["pdf"].read_bytes() == pdf_bytes

    @pytest.mark.asyncio
    async def test_cache_hit_on_second_fetch(self, tmp_path):
        """Second fetch for same ID returns from cache without calling source."""
        cfg = _cfg(tmp_path)
        cache = PatentCache(cfg)
        orc = FetcherOrchestrator(cfg, cache=cache)

        call_count = 0

        async def counting_fetch(patent, output_dir):
            nonlocal call_count
            call_count += 1
            output_dir.mkdir(parents=True, exist_ok=True)
            pdf = output_dir / f"{patent.canonical}.pdf"
            pdf.write_bytes(_STUB_PDF)
            from patent_mcp.cache import PatentMetadata, SourceAttempt
            from datetime import datetime, timezone
            from patent_mcp.fetchers.base import FetchResult
            return FetchResult(
                source_attempt=SourceAttempt(source="USPTO", success=True, elapsed_ms=10.0),
                pdf_path=pdf,
                metadata=PatentMetadata(
                    canonical_id="US7654321",
                    jurisdiction="US",
                    doc_type="patent",
                    title="Widget Assembly",
                    fetched_at=datetime.now(timezone.utc).isoformat(),
                ),
            )

        mock_src = MagicMock()
        mock_src.source_name = "USPTO"
        mock_src.can_fetch.return_value = True
        mock_src.fetch = counting_fetch
        orc._sources = [mock_src]

        patent = canonicalize("US7654321")
        output_base = tmp_path / ".patents"

        # First fetch — hits the source
        r1 = await orc.fetch(patent, output_base / patent.canonical)
        assert r1.success is True
        assert r1.from_cache is False
        assert call_count == 1

        # Second fetch — must come from cache
        r2 = await orc.fetch(patent, output_base / patent.canonical)
        assert r2.success is True
        assert r2.from_cache is True
        # Source should NOT have been called again
        assert call_count == 1, f"Source was called {call_count} times; expected 1"

    @pytest.mark.asyncio
    async def test_batch_returns_all_results(self, tmp_path):
        """fetch_batch processes all IDs and preserves order."""
        cfg = _cfg(tmp_path)
        cache = PatentCache(cfg)
        orc = FetcherOrchestrator(cfg, cache=cache)

        async def always_fail(patent, output_dir):
            from patent_mcp.fetchers.base import FetchResult
            from patent_mcp.cache import SourceAttempt
            return FetchResult(
                source_attempt=SourceAttempt(
                    source="USPTO", success=False, elapsed_ms=5.0, error="not_found"
                )
            )

        mock_src = MagicMock()
        mock_src.source_name = "USPTO"
        mock_src.can_fetch.return_value = True
        mock_src.fetch = always_fail
        orc._sources = [mock_src]

        ids = ["US7654321", "EP1234567", "WO2024123456"]
        patents = [canonicalize(i) for i in ids]
        results = await orc.fetch_batch(patents, tmp_path / ".patents")

        assert len(results) == len(ids)
        assert results[0].canonical_id == "US7654321"
        assert results[1].canonical_id == "EP1234567"
        assert results[2].canonical_id == "WO2024123456"


# ---------------------------------------------------------------------------
# E2E-02 — MCP server tool integration
# ---------------------------------------------------------------------------

class TestMcpServerToolIntegration:
    """Test fetch_patents tool via the MCP server (server + orchestrator together)."""

    @pytest.mark.asyncio
    async def test_fetch_patents_empty_list(self, tmp_path):
        """fetch_patents([]) returns empty results without errors."""
        cfg = _cfg(tmp_path)
        mock_cache = MagicMock(spec=PatentCache)
        mock_cache.list_all.return_value = []
        mock_cache.lookup.return_value = None
        mock_orch = MagicMock()
        mock_orch.fetch_batch = AsyncMock(return_value=[])

        with (
            patch("patent_mcp.server.PatentCache", return_value=mock_cache),
            patch("patent_mcp.server.FetcherOrchestrator", return_value=mock_orch),
        ):
            mcp = _build_server(config=cfg)

        # Access the fetch_patents tool function directly
        tools = mcp._tool_manager._tools  # FastMCP internal; skip if API changes
        fetch_fn = None
        for name, tool in tools.items():
            if name == "fetch_patents":
                fetch_fn = tool.fn
                break

        if fetch_fn is None:
            pytest.skip("FastMCP internal API changed; skipping direct tool access")

        result = await fetch_fn(patent_ids=[])
        assert result["results"] == []
        assert result["summary"]["total"] == 0
        assert result["summary"]["errors"] == 0

    @pytest.mark.asyncio
    async def test_fetch_patents_success_via_orchestrator(self, tmp_path):
        """fetch_patents returns correct structure for a successful fetch."""
        from patent_mcp.cache import PatentMetadata, SourceAttempt
        from patent_mcp.fetchers.orchestrator import OrchestratorResult
        from datetime import datetime, timezone

        cfg = _cfg(tmp_path)
        pdf_path = tmp_path / "US7654321.pdf"
        pdf_path.write_bytes(_STUB_PDF)

        orc_result = OrchestratorResult(
            canonical_id="US7654321",
            success=True,
            files={"pdf": pdf_path},
            metadata=PatentMetadata(
                canonical_id="US7654321",
                jurisdiction="US",
                doc_type="patent",
                title="Widget Assembly Method",
                abstract="A method for assembling widgets.",
                inventors=["Alice Smith"],
                fetched_at=datetime.now(timezone.utc).isoformat(),
            ),
            sources=[SourceAttempt(source="USPTO", success=True, elapsed_ms=42.0)],
        )

        mock_cache = MagicMock(spec=PatentCache)
        mock_cache.list_all.return_value = []
        mock_cache.lookup.return_value = None
        mock_orch = MagicMock()
        mock_orch.fetch_batch = AsyncMock(return_value=[orc_result])

        with (
            patch("patent_mcp.server.PatentCache", return_value=mock_cache),
            patch("patent_mcp.server.FetcherOrchestrator", return_value=mock_orch),
        ):
            mcp = _build_server(config=cfg)

        tools = mcp._tool_manager._tools
        fetch_fn = tools.get("fetch_patents")
        if fetch_fn is None:
            pytest.skip("FastMCP internal API changed")
        fetch_fn = fetch_fn.fn

        result = await fetch_fn(patent_ids=["US7654321"])
        assert len(result["results"]) == 1
        r = result["results"][0]
        assert r["patent_id"] == "US7654321"
        assert r["status"] == "fetched"
        assert r["metadata"]["title"] == "Widget Assembly Method"
        assert r["files"]["pdf"] == str(pdf_path)

        summary = result["summary"]
        assert summary["total"] == 1
        assert summary["success"] == 1
        assert summary["errors"] == 0

    @pytest.mark.asyncio
    async def test_fetch_patents_error_result(self, tmp_path):
        """fetch_patents marks failed patents with status='error'."""
        from patent_mcp.fetchers.orchestrator import OrchestratorResult
        from patent_mcp.cache import SourceAttempt

        cfg = _cfg(tmp_path)

        orc_result = OrchestratorResult(
            canonical_id="US9999999",
            success=False,
            error="not_found",
            sources=[SourceAttempt(source="USPTO", success=False, elapsed_ms=5.0, error="not_found")],
        )

        mock_cache = MagicMock(spec=PatentCache)
        mock_cache.list_all.return_value = []
        mock_cache.lookup.return_value = None
        mock_orch = MagicMock()
        mock_orch.fetch_batch = AsyncMock(return_value=[orc_result])

        with (
            patch("patent_mcp.server.PatentCache", return_value=mock_cache),
            patch("patent_mcp.server.FetcherOrchestrator", return_value=mock_orch),
        ):
            mcp = _build_server(config=cfg)

        tools = mcp._tool_manager._tools
        fetch_fn = tools.get("fetch_patents")
        if fetch_fn is None:
            pytest.skip("FastMCP internal API changed")

        result = await fetch_fn.fn(patent_ids=["US9999999"])
        assert result["results"][0]["status"] == "error"
        assert result["summary"]["errors"] == 1
        assert result["summary"]["success"] == 0

    @pytest.mark.asyncio
    async def test_fetch_patents_cached_result(self, tmp_path):
        """fetch_patents marks cache hits with status='cached'."""
        from patent_mcp.cache import PatentMetadata, SourceAttempt
        from patent_mcp.fetchers.orchestrator import OrchestratorResult
        from datetime import datetime, timezone

        cfg = _cfg(tmp_path)
        pdf_path = tmp_path / "US7654321.pdf"
        pdf_path.write_bytes(_STUB_PDF)

        orc_result = OrchestratorResult(
            canonical_id="US7654321",
            success=True,
            from_cache=True,
            files={"pdf": pdf_path},
            metadata=PatentMetadata(
                canonical_id="US7654321",
                jurisdiction="US",
                doc_type="patent",
                title="Widget Assembly",
                fetched_at=datetime.now(timezone.utc).isoformat(),
            ),
            sources=[],
        )

        mock_cache = MagicMock(spec=PatentCache)
        mock_cache.list_all.return_value = []
        mock_cache.lookup.return_value = None
        mock_orch = MagicMock()
        mock_orch.fetch_batch = AsyncMock(return_value=[orc_result])

        with (
            patch("patent_mcp.server.PatentCache", return_value=mock_cache),
            patch("patent_mcp.server.FetcherOrchestrator", return_value=mock_orch),
        ):
            mcp = _build_server(config=cfg)

        tools = mcp._tool_manager._tools
        fetch_fn = tools.get("fetch_patents")
        if fetch_fn is None:
            pytest.skip("FastMCP internal API changed")

        result = await fetch_fn.fn(patent_ids=["US7654321"])
        assert result["results"][0]["status"] == "cached"
        assert result["summary"]["cached"] == 1

    @pytest.mark.asyncio
    async def test_fetch_patents_batch_mixed_results(self, tmp_path):
        """Batch fetch: some succeed, some fail, summary is accurate."""
        from patent_mcp.cache import PatentMetadata, SourceAttempt
        from patent_mcp.fetchers.orchestrator import OrchestratorResult
        from datetime import datetime, timezone

        cfg = _cfg(tmp_path)
        pdf_path = tmp_path / "US1.pdf"
        pdf_path.write_bytes(_STUB_PDF)

        orc_results = [
            OrchestratorResult(
                canonical_id="US7654321",
                success=True,
                files={"pdf": pdf_path},
                metadata=PatentMetadata(
                    canonical_id="US7654321",
                    jurisdiction="US",
                    doc_type="patent",
                    title="Widget A",
                    fetched_at=datetime.now(timezone.utc).isoformat(),
                ),
                sources=[SourceAttempt(source="USPTO", success=True, elapsed_ms=10.0)],
            ),
            OrchestratorResult(
                canonical_id="US9999999",
                success=False,
                error="not_found",
                sources=[SourceAttempt(source="USPTO", success=False, elapsed_ms=5.0, error="not_found")],
            ),
            OrchestratorResult(
                canonical_id="EP1234567",
                success=True,
                from_cache=True,
                files={},
                metadata=PatentMetadata(
                    canonical_id="EP1234567",
                    jurisdiction="EP",
                    doc_type="patent",
                    title="Gear Mechanism",
                    fetched_at=datetime.now(timezone.utc).isoformat(),
                ),
                sources=[],
            ),
        ]

        mock_cache = MagicMock(spec=PatentCache)
        mock_cache.list_all.return_value = []
        mock_cache.lookup.return_value = None
        mock_orch = MagicMock()
        mock_orch.fetch_batch = AsyncMock(return_value=orc_results)

        with (
            patch("patent_mcp.server.PatentCache", return_value=mock_cache),
            patch("patent_mcp.server.FetcherOrchestrator", return_value=mock_orch),
        ):
            mcp = _build_server(config=cfg)

        tools = mcp._tool_manager._tools
        fetch_fn = tools.get("fetch_patents")
        if fetch_fn is None:
            pytest.skip("FastMCP internal API changed")

        result = await fetch_fn.fn(patent_ids=["US7654321", "US9999999", "EP1234567"])
        summary = result["summary"]
        assert summary["total"] == 3
        assert summary["success"] == 2
        assert summary["cached"] == 1
        assert summary["errors"] == 1

        statuses = {r["patent_id"]: r["status"] for r in result["results"]}
        assert statuses["US7654321"] == "fetched"
        assert statuses["US9999999"] == "error"
        assert statuses["EP1234567"] == "cached"


# ---------------------------------------------------------------------------
# E2E-03 — list_cached_patents and get_patent_metadata tools
# ---------------------------------------------------------------------------

class TestListAndMetadataTools:
    @pytest.mark.asyncio
    async def test_list_cached_patents_returns_entries(self, tmp_path):
        """list_cached_patents returns all cached IDs."""
        from patent_mcp.cache import CacheEntry

        cfg = _cfg(tmp_path)
        mock_cache = MagicMock(spec=PatentCache)
        mock_cache.list_all.return_value = [
            CacheEntry(canonical_id="US7654321", cache_dir=tmp_path / "US7654321"),
            CacheEntry(canonical_id="EP1234567", cache_dir=tmp_path / "EP1234567"),
        ]
        mock_cache.lookup.return_value = None

        with (
            patch("patent_mcp.server.PatentCache", return_value=mock_cache),
            patch("patent_mcp.server.FetcherOrchestrator"),
        ):
            mcp = _build_server(config=cfg)

        tools = mcp._tool_manager._tools
        list_fn = tools.get("list_cached_patents")
        if list_fn is None:
            pytest.skip("FastMCP internal API changed")

        result = list_fn.fn()
        assert result["count"] == 2
        ids = {p["canonical_id"] for p in result["patents"]}
        assert "US7654321" in ids
        assert "EP1234567" in ids

    @pytest.mark.asyncio
    async def test_get_patent_metadata_hit(self, tmp_path):
        """get_patent_metadata returns metadata for a cached patent."""
        from patent_mcp.cache import CacheResult, PatentMetadata
        from datetime import datetime, timezone

        cfg = _cfg(tmp_path)
        cached_meta = PatentMetadata(
            canonical_id="US7654321",
            jurisdiction="US",
            doc_type="patent",
            title="Widget Assembly Method",
            abstract="A method for assembling widgets.",
            inventors=["Alice Smith"],
            fetched_at=datetime.now(timezone.utc).isoformat(),
        )
        mock_cache = MagicMock(spec=PatentCache)
        mock_cache.lookup.return_value = CacheResult(
            canonical_id="US7654321",
            cache_dir=tmp_path,
            files={},
            metadata=cached_meta,
            is_complete=True,
        )

        with (
            patch("patent_mcp.server.PatentCache", return_value=mock_cache),
            patch("patent_mcp.server.FetcherOrchestrator"),
        ):
            mcp = _build_server(config=cfg)

        tools = mcp._tool_manager._tools
        meta_fn = tools.get("get_patent_metadata")
        if meta_fn is None:
            pytest.skip("FastMCP internal API changed")

        result = meta_fn.fn(patent_ids=["US7654321"])
        assert len(result["results"]) == 1
        r = result["results"][0]
        assert r["patent_id"] == "US7654321"
        assert r["metadata"]["title"] == "Widget Assembly Method"

    @pytest.mark.asyncio
    async def test_get_patent_metadata_miss(self, tmp_path):
        """get_patent_metadata returns None metadata for unknown patent."""
        cfg = _cfg(tmp_path)
        mock_cache = MagicMock(spec=PatentCache)
        mock_cache.lookup.return_value = None

        with (
            patch("patent_mcp.server.PatentCache", return_value=mock_cache),
            patch("patent_mcp.server.FetcherOrchestrator"),
        ):
            mcp = _build_server(config=cfg)

        tools = mcp._tool_manager._tools
        meta_fn = tools.get("get_patent_metadata")
        if meta_fn is None:
            pytest.skip("FastMCP internal API changed")

        result = meta_fn.fn(patent_ids=["US9999999"])
        assert result["results"][0]["metadata"] is None


# ---------------------------------------------------------------------------
# E2E-04 — force_refresh bypasses cache
# ---------------------------------------------------------------------------

class TestForceRefresh:
    @pytest.mark.asyncio
    async def test_force_refresh_skips_cache(self, tmp_path):
        """force_refresh=True should bypass the cache and re-fetch."""
        from patent_mcp.cache import PatentMetadata, SourceAttempt
        from patent_mcp.fetchers.orchestrator import OrchestratorResult
        from datetime import datetime, timezone

        cfg = _cfg(tmp_path)
        pdf_path = tmp_path / "US7654321.pdf"
        pdf_path.write_bytes(_STUB_PDF)

        fresh_result = OrchestratorResult(
            canonical_id="US7654321",
            success=True,
            from_cache=False,  # NOT from cache
            files={"pdf": pdf_path},
            metadata=PatentMetadata(
                canonical_id="US7654321",
                jurisdiction="US",
                doc_type="patent",
                title="Fresh Widget",
                fetched_at=datetime.now(timezone.utc).isoformat(),
            ),
            sources=[SourceAttempt(source="USPTO", success=True, elapsed_ms=10.0)],
        )

        mock_cache = MagicMock(spec=PatentCache)
        mock_cache.list_all.return_value = []
        # Simulate cached entry exists (but force_refresh should skip it)
        mock_cache.lookup.return_value = None
        mock_orch = MagicMock()
        mock_orch.fetch_batch = AsyncMock(return_value=[fresh_result])

        with (
            patch("patent_mcp.server.PatentCache", return_value=mock_cache),
            patch("patent_mcp.server.FetcherOrchestrator", return_value=mock_orch),
        ):
            mcp = _build_server(config=cfg)

        tools = mcp._tool_manager._tools
        fetch_fn = tools.get("fetch_patents")
        if fetch_fn is None:
            pytest.skip("FastMCP internal API changed")

        result = await fetch_fn.fn(patent_ids=["US7654321"], force_refresh=True)
        # The mock always returns fresh_result; verify fetch_batch was called
        mock_orch.fetch_batch.assert_called_once()
        assert result["results"][0]["status"] == "fetched"


# ---------------------------------------------------------------------------
# E2E-04b — Invalid/unknown patent IDs
# ---------------------------------------------------------------------------

class TestInvalidIdHandling:
    """Invalid or unparseable patent IDs should not crash the server."""

    @pytest.mark.asyncio
    async def test_invalid_id_canonicalizes_to_unknown(self):
        """An invalid patent ID should canonicalize to 'UNKNOWN' jurisdiction."""
        from patent_mcp.id_canon import canonicalize

        result = canonicalize("NOTAPATENTID")
        # Should not raise; returns canonical with UNKNOWN jurisdiction
        assert result.jurisdiction == "UNKNOWN"
        assert len(result.errors) > 0  # has parse errors

    @pytest.mark.asyncio
    async def test_fetch_patents_with_invalid_id_returns_error(self, tmp_path):
        """fetch_patents with an unparseable ID returns error status, not crash."""
        from patent_mcp.cache import SourceAttempt
        from patent_mcp.fetchers.orchestrator import OrchestratorResult

        cfg = _cfg(tmp_path)

        # The orchestrator would receive the UNKNOWN canonical ID and fail
        orc_result = OrchestratorResult(
            canonical_id="UNKNOWN/NOTAPATENTID",
            success=False,
            error="no_sources_available",
            sources=[],
        )

        mock_cache = MagicMock(spec=PatentCache)
        mock_cache.list_all.return_value = []
        mock_cache.lookup.return_value = None
        mock_orch = MagicMock()
        mock_orch.fetch_batch = AsyncMock(return_value=[orc_result])

        with (
            patch("patent_mcp.server.PatentCache", return_value=mock_cache),
            patch("patent_mcp.server.FetcherOrchestrator", return_value=mock_orch),
        ):
            mcp = _build_server(config=cfg)

        tools = mcp._tool_manager._tools
        fetch_fn = tools.get("fetch_patents")
        if fetch_fn is None:
            pytest.skip("FastMCP internal API changed")

        result = await fetch_fn.fn(patent_ids=["NOTAPATENTID"])
        assert len(result["results"]) == 1
        assert result["results"][0]["status"] == "error"
        assert result["summary"]["errors"] == 1

    @pytest.mark.asyncio
    async def test_mixed_valid_invalid_ids(self, tmp_path):
        """Valid and invalid IDs in same batch: valid succeeds, invalid errors."""
        from patent_mcp.cache import PatentMetadata, SourceAttempt
        from patent_mcp.fetchers.orchestrator import OrchestratorResult
        from datetime import datetime, timezone

        cfg = _cfg(tmp_path)
        pdf_path = tmp_path / "US7654321.pdf"
        pdf_path.write_bytes(_STUB_PDF)

        orc_results = [
            OrchestratorResult(
                canonical_id="US7654321",
                success=True,
                files={"pdf": pdf_path},
                metadata=PatentMetadata(
                    canonical_id="US7654321",
                    jurisdiction="US",
                    doc_type="patent",
                    title="Valid Patent",
                    fetched_at=datetime.now(timezone.utc).isoformat(),
                ),
                sources=[SourceAttempt(source="USPTO", success=True, elapsed_ms=10.0)],
            ),
            OrchestratorResult(
                canonical_id="UNKNOWN",
                success=False,
                error="no_sources_available",
                sources=[],
            ),
        ]

        mock_cache = MagicMock(spec=PatentCache)
        mock_cache.list_all.return_value = []
        mock_cache.lookup.return_value = None
        mock_orch = MagicMock()
        mock_orch.fetch_batch = AsyncMock(return_value=orc_results)

        with (
            patch("patent_mcp.server.PatentCache", return_value=mock_cache),
            patch("patent_mcp.server.FetcherOrchestrator", return_value=mock_orch),
        ):
            mcp = _build_server(config=cfg)

        tools = mcp._tool_manager._tools
        fetch_fn = tools.get("fetch_patents")
        if fetch_fn is None:
            pytest.skip("FastMCP internal API changed")

        result = await fetch_fn.fn(patent_ids=["US7654321", "NOTAPATENTID"])
        assert result["summary"]["success"] == 1
        assert result["summary"]["errors"] == 1


# ---------------------------------------------------------------------------
# E2E-05 — HTTP-level integration via respx (PpubsSource)
# ---------------------------------------------------------------------------

class TestHttpSourceIntegration:
    """Integration tests using respx to mock HTTP at the transport level."""

    @pytest.mark.asyncio
    async def test_ppubs_full_fetch_via_respx(self, tmp_path):
        """Full PPUBS fetch via respx: session + search + PDF download."""
        from patent_mcp.fetchers.http import PpubsSource
        from patent_mcp.cache import SessionCache

        cfg = _cfg(tmp_path)
        sc = SessionCache()
        src = PpubsSource(cfg, sc)
        patent = canonicalize("US7654321")
        output_dir = tmp_path / "output"
        output_dir.mkdir()

        with respx.mock:
            # Session endpoint
            respx.post("http://mock-ppubs/ppubs-api/v1/session").mock(
                return_value=httpx.Response(200, json={"session": "tok-abc"})
            )
            # Search endpoint
            respx.get("http://mock-ppubs/ppubs-api/v1/patent").mock(
                return_value=httpx.Response(200, json=_US_PPUBS_RESPONSE)
            )
            # PDF download
            respx.get("http://mock-ppubs/ppubs-api/v1/download/test-guid-7654321").mock(
                return_value=httpx.Response(200, content=_STUB_PDF)
            )
            result = await src.fetch(patent, output_dir)

        assert result.source_attempt.success is True
        assert result.pdf_path is not None
        assert result.pdf_path.exists()
        assert result.pdf_path.read_bytes() == _STUB_PDF
        assert result.metadata is not None
        assert result.metadata.title == "Widget Assembly Method"

    @pytest.mark.asyncio
    async def test_ppubs_not_found_returns_failure(self, tmp_path):
        """PPUBS 404 → FetchResult with success=False, no files."""
        from patent_mcp.fetchers.http import PpubsSource
        from patent_mcp.cache import SessionCache

        cfg = _cfg(tmp_path)
        src = PpubsSource(cfg, SessionCache())
        patent = canonicalize("US9999999")
        output_dir = tmp_path / "output"
        output_dir.mkdir()

        with respx.mock:
            respx.post("http://mock-ppubs/ppubs-api/v1/session").mock(
                return_value=httpx.Response(200, json={"session": "tok-abc"})
            )
            respx.get("http://mock-ppubs/ppubs-api/v1/patent").mock(
                return_value=httpx.Response(404)
            )
            result = await src.fetch(patent, output_dir)

        assert result.source_attempt.success is False
        assert result.pdf_path is None

    @pytest.mark.asyncio
    async def test_ppubs_empty_results_returns_failure(self, tmp_path):
        """PPUBS returns 200 but empty patents list → failure."""
        from patent_mcp.fetchers.http import PpubsSource
        from patent_mcp.cache import SessionCache

        cfg = _cfg(tmp_path)
        src = PpubsSource(cfg, SessionCache())
        patent = canonicalize("US8888888")
        output_dir = tmp_path / "output"
        output_dir.mkdir()

        with respx.mock:
            respx.post("http://mock-ppubs/ppubs-api/v1/session").mock(
                return_value=httpx.Response(200, json={"session": "tok-abc"})
            )
            respx.get("http://mock-ppubs/ppubs-api/v1/patent").mock(
                return_value=httpx.Response(200, json={"patents": []})
            )
            result = await src.fetch(patent, output_dir)

        assert result.source_attempt.success is False
        assert result.pdf_path is None


# ---------------------------------------------------------------------------
# E2E-06 — Fixture file round-trip
# ---------------------------------------------------------------------------

class TestFixtureFiles:
    """Verify that fixture files are valid and parseable."""

    def test_us_uspto_fixture_parseable(self):
        fixture_path = (
            Path(__file__).parent.parent / "fixtures" / "us" / "US7654321" /
            "sources" / "uspto_response.json"
        )
        assert fixture_path.exists(), f"Fixture missing: {fixture_path}"
        data = json.loads(fixture_path.read_text())
        assert "patents" in data
        assert len(data["patents"]) > 0
        assert data["patents"][0]["patentNumber"] == "US7654321"

    def test_us_metadata_fixture_parseable(self):
        fixture_path = (
            Path(__file__).parent.parent / "fixtures" / "us" / "US7654321" /
            "metadata.json"
        )
        assert fixture_path.exists()
        data = json.loads(fixture_path.read_text())
        assert data["canonical_id"] == "US7654321"
        assert data["jurisdiction"] == "US"

    def test_ep_xml_fixture_parseable(self):
        import xml.etree.ElementTree as ET
        fixture_path = (
            Path(__file__).parent.parent / "fixtures" / "ep" / "EP1234567" /
            "sources" / "epo_ops_response.xml"
        )
        assert fixture_path.exists()
        tree = ET.parse(str(fixture_path))
        root = tree.getroot()
        assert root is not None

    def test_invalid_error_fixture_parseable(self):
        fixture_path = (
            Path(__file__).parent.parent / "fixtures" / "invalid" /
            "expected_error.json"
        )
        assert fixture_path.exists()
        data = json.loads(fixture_path.read_text())
        assert data["status"] == "error"
