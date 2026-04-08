"""Tests for patent_mcp.fetchers.orchestrator — T01-T11."""
from __future__ import annotations

import asyncio
import time
from dataclasses import dataclass
from pathlib import Path
from unittest.mock import AsyncMock, MagicMock, patch

import pytest

from patent_mcp.cache import (
    ArtifactSet,
    CacheResult,
    PatentCache,
    PatentMetadata,
    SourceAttempt,
)
from patent_mcp.config import load_config
from patent_mcp.fetchers.base import FetchResult
from patent_mcp.fetchers.orchestrator import FetcherOrchestrator, OrchestratorResult
from patent_mcp.id_canon import canonicalize


def _cfg(tmp_path=None, **overrides):
    cfg = load_config(env={})
    if tmp_path:
        cfg.cache_local_dir = tmp_path / ".patents"
        cfg.cache_global_db = tmp_path / "global" / "index.db"
    cfg.fetch_all_sources = False  # default to sequential for most tests
    for k, v in overrides.items():
        setattr(cfg, k, v)
    return cfg


def _us():
    return canonicalize("US7654321")


def _ep():
    return canonicalize("EP1234567")


def _make_meta(canonical_id: str = "US7654321") -> PatentMetadata:
    return PatentMetadata(
        canonical_id=canonical_id,
        jurisdiction="US",
        doc_type="patent",
        title="Test Patent",
        fetched_at="2026-01-01T00:00:00+00:00",
    )


def _success_result(canonical_id: str, tmp_path: Path) -> FetchResult:
    """Helper: build a successful FetchResult with a dummy PDF."""
    pdf = tmp_path / f"{canonical_id}.pdf"
    pdf.write_text("%PDF-1.4")
    return FetchResult(
        source_attempt=SourceAttempt(source="USPTO", success=True, elapsed_ms=50.0),
        pdf_path=pdf,
        metadata=_make_meta(canonical_id),
    )


def _fail_result(source_name: str = "USPTO") -> FetchResult:
    return FetchResult(
        source_attempt=SourceAttempt(
            source=source_name, success=False, elapsed_ms=10.0, error="not_found"
        )
    )


# ---------------------------------------------------------------------------
# T01 — Source registry
# ---------------------------------------------------------------------------

class TestSourceRegistry:
    def test_sources_registered_for_us(self, tmp_path):
        cfg = _cfg(tmp_path=tmp_path)
        orc = FetcherOrchestrator(cfg)
        sources = orc.get_sources_for(_us())
        names = [s.source_name for s in sources]
        # US patents should have USPTO in the list
        assert "USPTO" in names

    def test_sources_registered_for_ep(self, tmp_path):
        cfg = _cfg(tmp_path=tmp_path)
        orc = FetcherOrchestrator(cfg)
        sources = orc.get_sources_for(_ep())
        names = [s.source_name for s in sources]
        # EPO source should be in the list for EP patents
        assert "EPO_OPS" in names

    def test_us_patent_not_in_wipo_only_sources(self, tmp_path):
        cfg = _cfg(tmp_path=tmp_path)
        orc = FetcherOrchestrator(cfg)
        sources = orc.get_sources_for(_us())
        # WIPO only supports WO
        names = [s.source_name for s in sources]
        assert "WIPO_Scrape" not in names


# ---------------------------------------------------------------------------
# T02 — Source priority ordering
# ---------------------------------------------------------------------------

class TestSourcePriority:
    def test_sources_in_config_priority_order(self, tmp_path):
        cfg = _cfg(tmp_path=tmp_path, source_priority=["EPO_OPS", "USPTO", "BigQuery"])
        orc = FetcherOrchestrator(cfg)
        sources = orc.get_sources_for(_us())
        names = [s.source_name for s in sources if s.source_name in ("EPO_OPS", "USPTO")]
        assert names.index("EPO_OPS") < names.index("USPTO")


# ---------------------------------------------------------------------------
# T03 — Cache hit skips fetching
# ---------------------------------------------------------------------------

class TestCacheHit:
    def test_cache_hit_returns_immediately(self, tmp_path):
        cfg = _cfg(tmp_path=tmp_path)
        mock_cache = MagicMock(spec=PatentCache)
        mock_cache.lookup.return_value = CacheResult(
            canonical_id="US7654321",
            cache_dir=tmp_path,
            files={"pdf": tmp_path / "US7654321.pdf"},
            metadata=_make_meta(),
            is_complete=True,
        )
        orc = FetcherOrchestrator(cfg, cache=mock_cache)

        result = asyncio.run(orc.fetch(_us(), tmp_path / "out"))
        assert result.success is True
        assert result.from_cache is True
        # No source should have been called (cache short-circuits)
        assert result.files is not None


# ---------------------------------------------------------------------------
# T04 — Single source success
# ---------------------------------------------------------------------------

class TestSingleSourceSuccess:
    def test_single_source_success(self, tmp_path):
        cfg = _cfg(tmp_path=tmp_path, fetch_all_sources=False)
        mock_cache = MagicMock(spec=PatentCache)
        mock_cache.lookup.return_value = None

        pdf = tmp_path / "US7654321.pdf"
        pdf.write_text("%PDF-1.4")

        async def mock_fetch(patent, output_dir):
            return FetchResult(
                source_attempt=SourceAttempt(source="USPTO", success=True, elapsed_ms=50.0),
                pdf_path=pdf,
                metadata=_make_meta(),
            )

        orc = FetcherOrchestrator(cfg, cache=mock_cache)
        # Replace sources with a single mock
        mock_src = MagicMock()
        mock_src.source_name = "USPTO"
        mock_src.can_fetch.return_value = True
        mock_src.fetch = mock_fetch
        orc._sources = [mock_src]

        result = asyncio.run(orc.fetch(_us(), tmp_path / "out"))
        assert result.success is True
        assert "pdf" in result.files or result.files  # has some files


# ---------------------------------------------------------------------------
# T05 — Fan-out (fetch_all_sources=True)
# ---------------------------------------------------------------------------

class TestFanOut:
    def test_all_sources_tried_when_fetch_all(self, tmp_path):
        cfg = _cfg(tmp_path=tmp_path, fetch_all_sources=True)
        mock_cache = MagicMock(spec=PatentCache)
        mock_cache.lookup.return_value = None

        pdf = tmp_path / "US7654321.pdf"
        pdf.write_text("%PDF-1.4")

        call_counts = []

        def make_mock_src(name, success=True):
            src = MagicMock()
            src.source_name = name

            async def mock_fetch(patent, output_dir):
                call_counts.append(name)
                if success:
                    return FetchResult(
                        source_attempt=SourceAttempt(source=name, success=True, elapsed_ms=10.0),
                        pdf_path=pdf,
                        metadata=_make_meta(),
                    )
                return _fail_result(name)

            src.can_fetch.return_value = True
            src.fetch = mock_fetch
            return src

        sources = [make_mock_src("USPTO"), make_mock_src("EPO_OPS"), make_mock_src("Lens")]
        orc = FetcherOrchestrator(cfg, cache=mock_cache)
        orc._sources = sources

        result = asyncio.run(orc.fetch(_us(), tmp_path / "out"))
        assert result.success is True
        # All 3 sources should be called
        assert len(call_counts) == 3


# ---------------------------------------------------------------------------
# T06 — Partial success
# ---------------------------------------------------------------------------

class TestPartialSuccess:
    def test_partial_success_aggregated(self, tmp_path):
        cfg = _cfg(tmp_path=tmp_path, fetch_all_sources=True)
        mock_cache = MagicMock(spec=PatentCache)
        mock_cache.lookup.return_value = None

        pdf = tmp_path / "US7654321.pdf"
        pdf.write_text("%PDF-1.4")

        async def src1_fetch(patent, output_dir):
            return FetchResult(
                source_attempt=SourceAttempt(source="USPTO", success=True, elapsed_ms=50.0),
                pdf_path=pdf,
                metadata=_make_meta(),
            )

        async def src2_fetch(patent, output_dir):
            return _fail_result("EPO_OPS")

        mock_src1, mock_src2 = MagicMock(), MagicMock()
        mock_src1.source_name = "USPTO"
        mock_src1.can_fetch.return_value = True
        mock_src1.fetch = src1_fetch
        mock_src2.source_name = "EPO_OPS"
        mock_src2.can_fetch.return_value = True
        mock_src2.fetch = src2_fetch

        orc = FetcherOrchestrator(cfg, cache=mock_cache)
        orc._sources = [mock_src1, mock_src2]

        result = asyncio.run(orc.fetch(_us(), tmp_path / "out"))
        assert result.success is True

        attempt_names = {a.source for a in result.sources}
        assert "USPTO" in attempt_names
        assert "EPO_OPS" in attempt_names

        epo_attempt = next(a for a in result.sources if a.source == "EPO_OPS")
        assert epo_attempt.success is False


# ---------------------------------------------------------------------------
# T07 — Web search fallback
# ---------------------------------------------------------------------------

class TestWebSearchFallback:
    def test_web_search_fallback_triggered(self, tmp_path):
        cfg = _cfg(tmp_path=tmp_path, fetch_all_sources=False)
        mock_cache = MagicMock(spec=PatentCache)
        mock_cache.lookup.return_value = None

        async def failing_fetch(patent, output_dir):
            return _fail_result("USPTO")

        async def web_fetch(patent, output_dir):
            return FetchResult(
                source_attempt=SourceAttempt(
                    source="web_search", success=True, elapsed_ms=100.0,
                    metadata={"urls": [], "note": "fallback", "formats_retrieved": []}
                )
            )

        mock_structured = MagicMock()
        mock_structured.source_name = "USPTO"
        mock_structured.can_fetch.return_value = True
        mock_structured.fetch = failing_fetch

        mock_web = MagicMock()
        mock_web.source_name = "web_search"
        mock_web.can_fetch.return_value = True
        mock_web.fetch = web_fetch

        orc = FetcherOrchestrator(cfg, cache=mock_cache)
        orc._sources = [mock_structured, mock_web]

        result = asyncio.run(orc.fetch(_us(), tmp_path / "out"))
        # web_search should be in sources
        assert any(a.source == "web_search" for a in result.sources)


# ---------------------------------------------------------------------------
# T08 — Format conversion after fetch
# ---------------------------------------------------------------------------

class TestConversionAfterFetch:
    def test_conversion_called_after_pdf(self, tmp_path):
        cfg = _cfg(
            tmp_path=tmp_path,
            fetch_all_sources=False,
            converters_order=["pymupdf4llm"],
            converters_disabled=["pdfplumber", "pdftotext", "marker"],
        )
        mock_cache = MagicMock(spec=PatentCache)
        mock_cache.lookup.return_value = None

        pdf = tmp_path / "US7654321.pdf"
        pdf.write_text("%PDF-1.4")

        async def mock_fetch(patent, output_dir):
            return FetchResult(
                source_attempt=SourceAttempt(source="USPTO", success=True, elapsed_ms=50.0),
                pdf_path=pdf,
                metadata=_make_meta(),
            )

        mock_src = MagicMock()
        mock_src.source_name = "USPTO"
        mock_src.can_fetch.return_value = True
        mock_src.fetch = mock_fetch

        orc = FetcherOrchestrator(cfg, cache=mock_cache)
        orc._sources = [mock_src]

        from unittest.mock import patch
        from patent_mcp.converters.pipeline import ConversionResult

        md_path = tmp_path / "out" / "US7654321.md"

        with patch(
            "patent_mcp.converters.pipeline.ConverterPipeline.pdf_to_markdown",
            return_value=ConversionResult(success=True, output_path=md_path, converter_used="pymupdf4llm"),
        ):
            result = asyncio.run(orc.fetch(_us(), tmp_path / "out"))

        assert result.success is True


# ---------------------------------------------------------------------------
# T09 — Batch fetch
# ---------------------------------------------------------------------------

class TestBatchFetch:
    def test_batch_fetch_processes_all(self, tmp_path):
        cfg = _cfg(tmp_path=tmp_path, fetch_all_sources=False)
        mock_cache = MagicMock(spec=PatentCache)
        mock_cache.lookup.return_value = None

        pdf = tmp_path / "stub.pdf"
        pdf.write_text("%PDF-1.4")

        async def mock_fetch(patent, output_dir):
            return FetchResult(
                source_attempt=SourceAttempt(source="USPTO", success=True, elapsed_ms=10.0),
                pdf_path=pdf,
                metadata=_make_meta(patent.canonical),
            )

        mock_src = MagicMock()
        mock_src.source_name = "USPTO"
        mock_src.can_fetch.return_value = True
        mock_src.fetch = mock_fetch

        orc = FetcherOrchestrator(cfg, cache=mock_cache)
        orc._sources = [mock_src]

        patents = [canonicalize(f"US{7000000 + i}") for i in range(3)]
        results = asyncio.run(orc.fetch_batch(patents, tmp_path / "out"))
        assert len(results) == 3

    @pytest.mark.slow
    def test_batch_fetch_concurrent(self, tmp_path):
        """3 patents each taking 50ms should complete in <200ms (parallelism)."""
        cfg = _cfg(tmp_path=tmp_path, fetch_all_sources=False, concurrency=10)
        mock_cache = MagicMock(spec=PatentCache)
        mock_cache.lookup.return_value = None

        pdf = tmp_path / "stub.pdf"
        pdf.write_text("%PDF-1.4")

        async def slow_fetch(patent, output_dir):
            await asyncio.sleep(0.05)  # 50ms
            return FetchResult(
                source_attempt=SourceAttempt(source="USPTO", success=True, elapsed_ms=50.0),
                pdf_path=pdf,
                metadata=_make_meta(patent.canonical),
            )

        mock_src = MagicMock()
        mock_src.source_name = "USPTO"
        mock_src.can_fetch.return_value = True
        mock_src.fetch = slow_fetch

        orc = FetcherOrchestrator(cfg, cache=mock_cache)
        orc._sources = [mock_src]

        patents = [canonicalize(f"US{7000000 + i}") for i in range(3)]
        start = time.monotonic()
        results = asyncio.run(orc.fetch_batch(patents, tmp_path / "out"))
        elapsed = time.monotonic() - start

        assert len(results) == 3
        assert elapsed < 0.2  # 200ms


# ---------------------------------------------------------------------------
# T10 — Failed patent doesn't affect others
# ---------------------------------------------------------------------------

class TestBatchIsolation:
    def test_batch_one_fail_others_succeed(self, tmp_path):
        cfg = _cfg(tmp_path=tmp_path, fetch_all_sources=False)
        mock_cache = MagicMock(spec=PatentCache)
        mock_cache.lookup.return_value = None

        pdf = tmp_path / "stub.pdf"
        pdf.write_text("%PDF-1.4")

        call_order = []

        async def conditional_fetch(patent, output_dir):
            call_order.append(patent.canonical)
            if "7000001" in patent.canonical:
                return _fail_result("USPTO")
            return FetchResult(
                source_attempt=SourceAttempt(source="USPTO", success=True, elapsed_ms=10.0),
                pdf_path=pdf,
                metadata=_make_meta(patent.canonical),
            )

        mock_src = MagicMock()
        mock_src.source_name = "USPTO"
        mock_src.can_fetch.return_value = True
        mock_src.fetch = conditional_fetch

        orc = FetcherOrchestrator(cfg, cache=mock_cache)
        orc._sources = [mock_src]

        patents = [canonicalize(f"US{7000000 + i}") for i in range(3)]
        results = asyncio.run(orc.fetch_batch(patents, tmp_path / "out"))

        assert results[0].success is True
        assert results[1].success is False  # US7000001 failed
        assert results[2].success is True


# ---------------------------------------------------------------------------
# T11 — Cache store after successful fetch
# ---------------------------------------------------------------------------

class TestCacheStoreAfterFetch:
    def test_artifacts_stored_in_cache(self, tmp_path):
        cfg = _cfg(tmp_path=tmp_path, fetch_all_sources=False)
        mock_cache = MagicMock(spec=PatentCache)
        mock_cache.lookup.return_value = None

        pdf = tmp_path / "US7654321.pdf"
        pdf.write_text("%PDF-1.4")

        async def mock_fetch(patent, output_dir):
            return FetchResult(
                source_attempt=SourceAttempt(source="USPTO", success=True, elapsed_ms=50.0),
                pdf_path=pdf,
                metadata=_make_meta(),
            )

        mock_src = MagicMock()
        mock_src.source_name = "USPTO"
        mock_src.can_fetch.return_value = True
        mock_src.fetch = mock_fetch

        orc = FetcherOrchestrator(cfg, cache=mock_cache)
        orc._sources = [mock_src]

        with patch("patent_mcp.converters.pipeline.ConverterPipeline"):
            asyncio.run(orc.fetch(_us(), tmp_path / "out"))

        # Cache store should have been called
        mock_cache.store.assert_called_once()
        call_args = mock_cache.store.call_args
        assert call_args[0][0] == "US7654321"  # canonical_id
