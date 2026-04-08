"""Tests for patent_mcp.converters.pipeline — T01-T12."""
from __future__ import annotations

import importlib.util
import shutil
import subprocess
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest

from patent_mcp.cache import PatentMetadata
from patent_mcp.config import load_config
from patent_mcp.converters.pipeline import (
    ConversionResult,
    ConverterPipeline,
    ImageResult,
    _merge_pymupdf4llm_with_pdfplumber,
    _try_pdftotext,
    check_available_tools,
)


def _make_config(tmp_path: Path | None = None, **overrides):
    cfg = load_config(env={})
    if tmp_path:
        cfg.cache_local_dir = tmp_path / ".patents"
        cfg.cache_global_db = tmp_path / "global" / "index.db"
    for k, v in overrides.items():
        setattr(cfg, k, v)
    return cfg


def _make_meta(canonical_id: str = "US7654321") -> PatentMetadata:
    return PatentMetadata(
        canonical_id=canonical_id,
        jurisdiction="US",
        doc_type="patent",
        title="Test Patent",
        abstract="A test abstract.",
        inventors=["Alice"],
        fetched_at="2026-01-01T00:00:00+00:00",
    )


def _stub_pdf(tmp_path: Path) -> Path:
    p = tmp_path / "stub.pdf"
    p.write_text("%PDF-1.4 stub")
    return p


# ---------------------------------------------------------------------------
# T01 — Tool availability check
# ---------------------------------------------------------------------------

class TestCheckAvailableTools:
    def test_all_present(self):
        cfg = _make_config(converters_order=["pymupdf4llm", "pdfplumber", "pdftotext"])
        with (
            patch.object(importlib.util, "find_spec", return_value=MagicMock()),
            patch("shutil.which", return_value="/usr/bin/pdftotext"),
        ):
            result = check_available_tools(cfg)
        assert result["pymupdf4llm"] is True
        assert result["pdfplumber"] is True
        assert result["pdftotext"] is True

    def test_none_present(self):
        cfg = _make_config(converters_order=["pymupdf4llm", "pdfplumber", "pdftotext"])
        with (
            patch.object(importlib.util, "find_spec", return_value=None),
            patch("shutil.which", return_value=None),
        ):
            result = check_available_tools(cfg)
        assert result["pymupdf4llm"] is False
        assert result["pdfplumber"] is False
        assert result["pdftotext"] is False

    def test_disabled_converters_show_false(self):
        cfg = _make_config(
            converters_order=["marker"],
            converters_disabled=["marker"],
        )
        with patch.object(importlib.util, "find_spec", return_value=MagicMock()):
            result = check_available_tools(cfg)
        assert result["marker"] is False


# ---------------------------------------------------------------------------
# T02 — pymupdf4llm conversion (mocked)
# ---------------------------------------------------------------------------

class TestPymupdf4llmConversion:
    def test_pymupdf4llm_converts_stub_pdf(self, tmp_path):
        cfg = _make_config(
            converters_order=["pymupdf4llm"],
            converters_disabled=["marker"],
        )
        pipeline = ConverterPipeline(cfg)
        pdf = _stub_pdf(tmp_path)
        out = tmp_path / "output.md"
        meta = _make_meta()

        mock_module = MagicMock()
        mock_module.to_markdown.return_value = "# Test Patent\n\nContent here."

        with patch.dict("sys.modules", {"pymupdf4llm": mock_module}):
            result = pipeline.pdf_to_markdown(pdf, out, meta)

        assert result.success is True
        assert result.converter_used == "pymupdf4llm"
        assert out.exists()
        assert "Test Patent" in out.read_text()

    def test_pymupdf4llm_importerror_falls_through(self, tmp_path):
        cfg = _make_config(
            converters_order=["pymupdf4llm", "pdftotext"],
            converters_disabled=["pdfplumber", "marker"],
        )
        pipeline = ConverterPipeline(cfg)
        pdf = _stub_pdf(tmp_path)
        out = tmp_path / "output.md"

        with (
            patch.dict("sys.modules", {"pymupdf4llm": None}),
            patch(
                "patent_mcp.converters.pipeline._try_pdftotext",
                return_value="pdftotext output",
            ),
        ):
            result = pipeline.pdf_to_markdown(pdf, out, _make_meta())

        assert result.converter_used == "pdftotext"


# ---------------------------------------------------------------------------
# T03 — pdfplumber extraction + table merging
# ---------------------------------------------------------------------------

class TestPdfplumberAndMerge:
    def test_pdfplumber_extracts_tables(self, tmp_path):
        cfg = _make_config(
            converters_order=["pdfplumber"],
            converters_disabled=["pymupdf4llm", "pdftotext", "marker"],
        )
        pipeline = ConverterPipeline(cfg)
        pdf = _stub_pdf(tmp_path)
        out = tmp_path / "output.md"

        mock_page = MagicMock()
        mock_page.extract_tables.return_value = [
            [["Col1", "Col2"], ["a", "b"], ["c", "d"]]
        ]
        mock_page.extract_text.return_value = "Some body text"

        mock_pdf_ctx = MagicMock()
        mock_pdf_ctx.__enter__ = MagicMock(return_value=mock_pdf_ctx)
        mock_pdf_ctx.__exit__ = MagicMock(return_value=False)
        mock_pdf_ctx.pages = [mock_page]

        mock_pdfplumber = MagicMock()
        mock_pdfplumber.open.return_value = mock_pdf_ctx

        with patch.dict("sys.modules", {"pdfplumber": mock_pdfplumber}):
            result = pipeline.pdf_to_markdown(pdf, out, _make_meta())

        assert result.success is True
        content = out.read_text()
        assert "| Col1 |" in content

    def test_merge_no_duplicate_content(self):
        prose = "# Patent\n\n| A | B |\n| --- | --- |\n| x | y |\n"
        tables = "| A | B |\n| --- | --- |\n| x | y |\n"
        merged = _merge_pymupdf4llm_with_pdfplumber(prose, tables)
        # x | y should appear only once
        assert merged.count("| x | y |") == 1

    def test_merge_adds_missing_tables(self):
        prose = "# Patent\n\nSome text here.\n"
        tables = "| Fig | Description |\n| --- | --- |\n| 1 | Widget |\n"
        merged = _merge_pymupdf4llm_with_pdfplumber(prose, tables)
        assert "Widget" in merged

    def test_merge_empty_tables_returns_prose(self):
        prose = "# Patent\n\nContent."
        merged = _merge_pymupdf4llm_with_pdfplumber(prose, "")
        assert merged == prose

    def test_merge_empty_prose_returns_tables(self):
        tables = "| A | B |\n| --- | --- |\n| 1 | 2 |\n"
        merged = _merge_pymupdf4llm_with_pdfplumber("", tables)
        assert "| A | B |" in merged


# ---------------------------------------------------------------------------
# T04 — pdftotext subprocess
# ---------------------------------------------------------------------------

class TestPdftotext:
    def test_pdftotext_called_correctly(self, tmp_path):
        pdf = _stub_pdf(tmp_path)
        mock_result = MagicMock()
        mock_result.returncode = 0
        mock_result.stdout = "TITLE\nAbstract text"

        with (
            patch("shutil.which", return_value="/usr/bin/pdftotext"),
            patch("subprocess.run", return_value=mock_result) as mock_run,
        ):
            result = _try_pdftotext(pdf)

        args = mock_run.call_args[0][0]
        assert args[0] == "pdftotext"
        assert "-layout" in args
        assert str(pdf) in args
        assert result is not None

    def test_pdftotext_converts_output_with_header_heuristic(self, tmp_path):
        pdf = _stub_pdf(tmp_path)
        mock_result = MagicMock()
        mock_result.returncode = 0
        mock_result.stdout = "CLAIMS\nThis is a claim."

        with (
            patch("shutil.which", return_value="/usr/bin/pdftotext"),
            patch("subprocess.run", return_value=mock_result),
        ):
            result = _try_pdftotext(pdf)

        assert result is not None
        assert "Claims" in result  # title-cased header

    def test_pdftotext_missing_returns_none(self, tmp_path):
        pdf = _stub_pdf(tmp_path)
        with patch("shutil.which", return_value=None):
            result = _try_pdftotext(pdf)
        assert result is None

    def test_pdftotext_nonzero_exit_returns_none(self, tmp_path):
        pdf = _stub_pdf(tmp_path)
        mock_result = MagicMock()
        mock_result.returncode = 1
        mock_result.stdout = ""

        with (
            patch("shutil.which", return_value="/usr/bin/pdftotext"),
            patch("subprocess.run", return_value=mock_result),
        ):
            result = _try_pdftotext(pdf)
        assert result is None


# ---------------------------------------------------------------------------
# T05 — Marker disabled
# ---------------------------------------------------------------------------

class TestMarkerDisabled:
    def test_marker_skipped_when_disabled(self, tmp_path):
        cfg = _make_config(
            converters_order=["marker"],
            converters_disabled=["marker"],
        )
        pipeline = ConverterPipeline(cfg)
        pdf = _stub_pdf(tmp_path)
        out = tmp_path / "output.md"

        mock_marker = MagicMock()
        with patch.dict("sys.modules", {"marker": mock_marker, "marker.convert": mock_marker}):
            result = pipeline.pdf_to_markdown(pdf, out, _make_meta())

        # Marker was disabled so pipeline returns failure without calling marker
        assert result.success is False
        mock_marker.convert.assert_not_called()

    def test_marker_called_when_enabled(self, tmp_path):
        cfg = _make_config(
            converters_order=["marker"],
            converters_disabled=[],
        )
        pipeline = ConverterPipeline(cfg)
        pdf = _stub_pdf(tmp_path)
        out = tmp_path / "output.md"

        mock_convert = MagicMock()
        mock_convert.convert_single_pdf.return_value = ("# Marker Output", [], {})

        with patch.dict("sys.modules", {"marker": MagicMock(), "marker.convert": mock_convert}):
            result = pipeline.pdf_to_markdown(pdf, out, _make_meta())

        assert result.converter_used == "marker"


# ---------------------------------------------------------------------------
# T06 — Fallback chain
# ---------------------------------------------------------------------------

class TestFallbackChain:
    def test_fallback_to_pdftotext_when_pymupdf4llm_fails(self, tmp_path):
        cfg = _make_config(
            converters_order=["pymupdf4llm", "pdftotext"],
            converters_disabled=["pdfplumber", "marker"],
        )
        pipeline = ConverterPipeline(cfg)
        pdf = _stub_pdf(tmp_path)
        out = tmp_path / "output.md"

        with (
            patch.dict("sys.modules", {"pymupdf4llm": None}),
            patch(
                "patent_mcp.converters.pipeline._try_pdftotext",
                return_value="pdftotext content",
            ),
        ):
            result = pipeline.pdf_to_markdown(pdf, out, _make_meta())

        assert result.success is True
        assert result.converter_used == "pdftotext"

    def test_all_converters_fail_returns_error(self, tmp_path):
        cfg = _make_config(
            converters_order=["pymupdf4llm", "pdftotext"],
            converters_disabled=["pdfplumber", "marker"],
        )
        pipeline = ConverterPipeline(cfg)
        pdf = _stub_pdf(tmp_path)
        out = tmp_path / "output.md"

        with (
            patch.dict("sys.modules", {"pymupdf4llm": None}),
            patch("patent_mcp.converters.pipeline._try_pdftotext", return_value=None),
        ):
            result = pipeline.pdf_to_markdown(pdf, out, _make_meta())

        assert result.success is False
        assert result.error == "no_converters_available"


# ---------------------------------------------------------------------------
# T07 — Plain text extraction
# ---------------------------------------------------------------------------

class TestPdfToText:
    def test_pdf_to_text(self, tmp_path):
        cfg = _make_config()
        pipeline = ConverterPipeline(cfg)
        pdf = _stub_pdf(tmp_path)
        out = tmp_path / "output.txt"

        mock_page = MagicMock()
        mock_page.get_text.return_value = "Patent text content."

        mock_doc = MagicMock()
        mock_doc.__enter__ = MagicMock(return_value=mock_doc)
        mock_doc.__exit__ = MagicMock(return_value=False)
        mock_doc.__iter__ = MagicMock(return_value=iter([mock_page]))

        mock_fitz = MagicMock()
        mock_fitz.open.return_value = mock_doc

        with patch.dict("sys.modules", {"fitz": mock_fitz}):
            result = pipeline.pdf_to_text(pdf, out)

        assert result.success is True
        assert out.exists()
        assert "Patent text content" in out.read_text()


# ---------------------------------------------------------------------------
# T08 — Image download
# ---------------------------------------------------------------------------

class TestImageDownload:
    def test_download_images_numbered(self, tmp_path):
        cfg = _make_config()
        pipeline = ConverterPipeline(cfg)
        images_dir = tmp_path / "images"

        mock_resp = MagicMock()
        mock_resp.content = b"\x89PNG\r\n"
        mock_resp.raise_for_status = MagicMock()

        mock_client = MagicMock()
        mock_client.get.return_value = mock_resp
        mock_client.__enter__ = MagicMock(return_value=mock_client)
        mock_client.__exit__ = MagicMock(return_value=False)

        with (
            patch("patent_mcp.converters.pipeline.shutil.which", return_value=None),
            patch("httpx.Client", return_value=mock_client),
        ):
            results = pipeline.download_images(
                ["http://example.com/fig1.png", "http://example.com/fig2.png"],
                images_dir,
            )

        assert len(results) == 2
        assert results[0].figure_number == 1
        assert results[1].figure_number == 2
        assert results[0].local_path.name == "fig001.png"
        assert results[1].local_path.name == "fig002.png"

    def test_download_images_three_numbered_correctly(self, tmp_path):
        cfg = _make_config()
        pipeline = ConverterPipeline(cfg)
        images_dir = tmp_path / "images"

        mock_resp = MagicMock()
        mock_resp.content = b"data"
        mock_resp.raise_for_status = MagicMock()
        mock_client = MagicMock()
        mock_client.get.return_value = mock_resp
        mock_client.__enter__ = MagicMock(return_value=mock_client)
        mock_client.__exit__ = MagicMock(return_value=False)

        with (
            patch("patent_mcp.converters.pipeline.shutil.which", return_value=None),
            patch("httpx.Client", return_value=mock_client),
        ):
            results = pipeline.download_images(
                [f"http://example.com/fig{i}.png" for i in range(1, 4)],
                images_dir,
            )

        names = [r.local_path.name for r in results]
        assert names == ["fig001.png", "fig002.png", "fig003.png"]


# ---------------------------------------------------------------------------
# T09 — Tesseract OCR
# ---------------------------------------------------------------------------

class TestOcrImage:
    def test_ocr_image(self, tmp_path):
        cfg = _make_config()
        pipeline = ConverterPipeline(cfg)
        image = tmp_path / "fig001.png"
        image.write_bytes(b"\x89PNG")

        mock_result = MagicMock()
        mock_result.returncode = 0
        mock_result.stdout = "Figure 1: Widget"

        with (
            patch("patent_mcp.converters.pipeline.shutil.which", return_value="/usr/bin/tesseract"),
            patch("subprocess.run", return_value=mock_result),
        ):
            text = pipeline.ocr_image(image)

        assert text == "Figure 1: Widget"

    def test_ocr_missing_tesseract_returns_none(self, tmp_path):
        cfg = _make_config()
        pipeline = ConverterPipeline(cfg)
        image = tmp_path / "fig001.png"
        image.write_bytes(b"\x89PNG")

        with patch("patent_mcp.converters.pipeline.shutil.which", return_value=None):
            text = pipeline.ocr_image(image)

        assert text is None


# ---------------------------------------------------------------------------
# T10 — Markdown assembly
# ---------------------------------------------------------------------------

class TestAssembleMarkdown:
    def test_assemble_with_metadata(self, tmp_path):
        cfg = _make_config()
        pipeline = ConverterPipeline(cfg)
        meta = _make_meta()
        meta.title = "Widget Assembly"
        meta.inventors = ["Alice", "Bob"]
        meta.assignee = "Acme Corp"

        result = pipeline.assemble_markdown(
            base_md="## Claims\n\n1. A widget...",
            metadata=meta,
            images=[
                ImageResult(
                    url="http://example.com/fig1.png",
                    local_path=tmp_path / "fig001.png",
                    ocr_text="Figure 1: Widget diagram",
                    figure_number=1,
                )
            ],
        )

        assert "# Widget Assembly" in result
        assert "**Inventors:**" in result
        assert "Alice" in result
        assert "Bob" in result
        assert "![Figure 1]" in result
        assert "Figure 1: Widget diagram" in result

    def test_assemble_no_images(self, tmp_path):
        cfg = _make_config()
        pipeline = ConverterPipeline(cfg)
        result = pipeline.assemble_markdown(
            base_md="## Abstract\n\nSome abstract.",
            metadata=_make_meta(),
            images=[],
        )
        assert "## Figures" not in result
        assert "Abstract" in result

    def test_assemble_with_abstract(self):
        cfg = _make_config()
        pipeline = ConverterPipeline(cfg)
        meta = _make_meta()
        meta.abstract = "This invention relates to..."
        result = pipeline.assemble_markdown(base_md="", metadata=meta, images=[])
        assert "## Abstract" in result
        assert "This invention relates to" in result


# ---------------------------------------------------------------------------
# T11 — Graceful degradation
# ---------------------------------------------------------------------------

class TestGracefulDegradation:
    def test_all_tools_missing_returns_error_not_exception(self, tmp_path):
        cfg = _make_config(
            converters_order=["pymupdf4llm", "pdfplumber", "pdftotext"],
            converters_disabled=["marker"],
        )
        pipeline = ConverterPipeline(cfg)
        pdf = _stub_pdf(tmp_path)
        out = tmp_path / "output.md"

        with (
            patch.dict("sys.modules", {"pymupdf4llm": None, "pdfplumber": None}),
            patch("patent_mcp.converters.pipeline._try_pdftotext", return_value=None),
        ):
            result = pipeline.pdf_to_markdown(pdf, out, _make_meta())

        assert result.success is False
        assert result.error is not None


# ---------------------------------------------------------------------------
# T12 — Full pipeline integration (mocked)
# ---------------------------------------------------------------------------

class TestFullPipeline:
    def test_full_pipeline_produces_patent_md(self, tmp_path):
        cfg = _make_config(
            converters_order=["pymupdf4llm"],
            converters_disabled=["pdfplumber", "pdftotext", "marker"],
        )
        pipeline = ConverterPipeline(cfg)
        pdf = _stub_pdf(tmp_path)
        output_dir = tmp_path / "output"
        output_dir.mkdir()
        out_md = output_dir / "patent.md"
        meta = _make_meta()
        meta.title = "Widget Assembly"
        meta.inventors = ["Alice"]
        meta.abstract = "A test abstract."

        mock_pymupdf4llm = MagicMock()
        mock_pymupdf4llm.to_markdown.return_value = "## Claims\n\n1. A widget."

        with patch.dict("sys.modules", {"pymupdf4llm": mock_pymupdf4llm}):
            conv_result = pipeline.pdf_to_markdown(pdf, out_md, meta)

        assert conv_result.success is True
        assert out_md.exists()

        # Assemble final markdown
        base = out_md.read_text()
        final = pipeline.assemble_markdown(base_md=base, metadata=meta, images=[])
        assert "# Widget Assembly" in final
        assert "A widget" in final
