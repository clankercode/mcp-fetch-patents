"""Format conversion pipeline: PDF → Markdown/Text, image download, OCR."""
from __future__ import annotations

import importlib.util
import shutil
import subprocess
import textwrap
from dataclasses import dataclass, field
from pathlib import Path
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from patent_mcp.cache import PatentMetadata
    from patent_mcp.config import PatentConfig


# ---------------------------------------------------------------------------
# Result types
# ---------------------------------------------------------------------------

@dataclass
class ConversionResult:
    success: bool
    output_path: Path | None = None
    converter_used: str | None = None
    error: str | None = None


@dataclass
class ImageResult:
    url: str
    local_path: Path
    ocr_text: str | None
    figure_number: int


# ---------------------------------------------------------------------------
# Tool availability
# ---------------------------------------------------------------------------

def check_available_tools(config: "PatentConfig") -> dict[str, bool]:
    """Return availability of each converter tool."""
    results: dict[str, bool] = {}
    for name in config.converters_order:
        if name in config.converters_disabled:
            results[name] = False
            continue
        if name == "pymupdf4llm":
            results[name] = importlib.util.find_spec("pymupdf4llm") is not None
        elif name == "pdfplumber":
            results[name] = importlib.util.find_spec("pdfplumber") is not None
        elif name == "pdftotext":
            results[name] = shutil.which("pdftotext") is not None
        elif name == "marker":
            results[name] = importlib.util.find_spec("marker") is not None
        else:
            results[name] = False
    results["tesseract"] = shutil.which("tesseract") is not None
    return results


# ---------------------------------------------------------------------------
# Individual converter implementations
# ---------------------------------------------------------------------------

def _try_pymupdf4llm(pdf_path: Path) -> str | None:
    """Try pymupdf4llm; return markdown string or None on failure."""
    try:
        import pymupdf4llm  # type: ignore[import]
        return pymupdf4llm.to_markdown(str(pdf_path))
    except (ImportError, Exception):
        return None


def _try_pdfplumber(pdf_path: Path) -> str | None:
    """Extract text + tables using pdfplumber; return markdown string or None."""
    try:
        import pdfplumber  # type: ignore[import]
        lines: list[str] = []
        with pdfplumber.open(str(pdf_path)) as pdf:
            for page in pdf.pages:
                tables = page.extract_tables()
                for table in tables:
                    if not table:
                        continue
                    # Convert table to markdown
                    header = table[0]
                    lines.append("| " + " | ".join(str(c or "") for c in header) + " |")
                    lines.append("| " + " | ".join("---" for _ in header) + " |")
                    for row in table[1:]:
                        lines.append("| " + " | ".join(str(c or "") for c in row) + " |")
                    lines.append("")
                text = page.extract_text() or ""
                if text.strip():
                    lines.append(text)
                    lines.append("")
        return "\n".join(lines) if lines else None
    except (ImportError, Exception):
        return None


def _merge_pymupdf4llm_with_pdfplumber(prose_md: str, tables_md: str) -> str:
    """Merge prose from pymupdf4llm with tables from pdfplumber, deduplicating."""
    if not tables_md:
        return prose_md
    if not prose_md:
        return tables_md

    # Extract table blocks from tables_md
    table_lines: list[str] = []
    in_table = False
    for line in tables_md.splitlines():
        if line.startswith("|"):
            in_table = True
            table_lines.append(line)
        elif in_table and not line.strip():
            table_lines.append("")
            in_table = False
        elif in_table:
            in_table = False

    if not table_lines:
        return prose_md

    tables_block = "\n".join(table_lines)

    # Check if table content already in prose (dedup)
    # Use a simple heuristic: if 80% of table lines are already in prose, skip
    table_data_lines = [l for l in table_lines if l.startswith("|") and "---" not in l]
    if table_data_lines:
        match_count = sum(1 for l in table_data_lines if l.strip() in prose_md)
        if match_count / len(table_data_lines) >= 0.8:
            return prose_md  # Already in prose

    return prose_md + "\n\n## Extracted Tables\n\n" + tables_block


def _try_pdftotext(pdf_path: Path) -> str | None:
    """Run pdftotext subprocess and return text or None on failure."""
    if shutil.which("pdftotext") is None:
        return None
    try:
        result = subprocess.run(
            ["pdftotext", "-layout", str(pdf_path), "-"],
            capture_output=True,
            text=True,
            timeout=60,
        )
        if result.returncode != 0:
            return None
        text = result.stdout
        if not text.strip():
            return None
        # Simple post-processing: treat ALL-CAPS short lines as headers
        lines = text.splitlines()
        md_lines: list[str] = []
        for line in lines:
            stripped = line.strip()
            if stripped and stripped.isupper() and len(stripped) < 80:
                md_lines.append(f"\n## {stripped.title()}\n")
            else:
                md_lines.append(line)
        return "\n".join(md_lines)
    except (subprocess.TimeoutExpired, Exception):
        return None


def _try_marker(pdf_path: Path) -> str | None:
    """Try marker-pdf; return markdown or None."""
    try:
        from marker.convert import convert_single_pdf  # type: ignore[import]
        full_text, _, _ = convert_single_pdf(str(pdf_path))
        return full_text
    except (ImportError, Exception):
        return None


# ---------------------------------------------------------------------------
# ConverterPipeline
# ---------------------------------------------------------------------------

class ConverterPipeline:
    def __init__(self, config: "PatentConfig") -> None:
        self._config = config

    def pdf_to_markdown(
        self,
        pdf_path: Path,
        output_path: Path,
        metadata: "PatentMetadata",
    ) -> ConversionResult:
        """Convert PDF to markdown using the configured converter chain."""
        disabled = set(self._config.converters_disabled)

        for name in self._config.converters_order:
            if name in disabled:
                continue

            if name == "pymupdf4llm":
                md = _try_pymupdf4llm(pdf_path)
                if md is not None:
                    # Optionally merge with pdfplumber tables
                    if "pdfplumber" not in disabled:
                        tables_md = _try_pdfplumber(pdf_path) or ""
                        md = _merge_pymupdf4llm_with_pdfplumber(md, tables_md)
                    output_path.parent.mkdir(parents=True, exist_ok=True)
                    output_path.write_text(md, encoding="utf-8")
                    return ConversionResult(
                        success=True,
                        output_path=output_path,
                        converter_used="pymupdf4llm",
                    )

            elif name == "pdfplumber":
                md = _try_pdfplumber(pdf_path)
                if md is not None:
                    output_path.parent.mkdir(parents=True, exist_ok=True)
                    output_path.write_text(md, encoding="utf-8")
                    return ConversionResult(
                        success=True,
                        output_path=output_path,
                        converter_used="pdfplumber",
                    )

            elif name == "pdftotext":
                md = _try_pdftotext(pdf_path)
                if md is not None:
                    output_path.parent.mkdir(parents=True, exist_ok=True)
                    output_path.write_text(md, encoding="utf-8")
                    return ConversionResult(
                        success=True,
                        output_path=output_path,
                        converter_used="pdftotext",
                    )

            elif name == "marker":
                md = _try_marker(pdf_path)
                if md is not None:
                    output_path.parent.mkdir(parents=True, exist_ok=True)
                    output_path.write_text(md, encoding="utf-8")
                    return ConversionResult(
                        success=True,
                        output_path=output_path,
                        converter_used="marker",
                    )

        return ConversionResult(
            success=False,
            error="no_converters_available",
        )

    def pdf_to_text(self, pdf_path: Path, output_path: Path) -> ConversionResult:
        """Extract plain text from PDF using PyMuPDF (fitz)."""
        try:
            import fitz  # type: ignore[import]  # PyMuPDF
            text_parts: list[str] = []
            with fitz.open(str(pdf_path)) as doc:
                for page in doc:
                    text_parts.append(page.get_text())
            text = "\n".join(text_parts)
            output_path.parent.mkdir(parents=True, exist_ok=True)
            output_path.write_text(text, encoding="utf-8")
            return ConversionResult(success=True, output_path=output_path, converter_used="pymupdf")
        except (ImportError, Exception) as e:
            return ConversionResult(success=False, error=f"pdf_to_text_failed: {e}")

    def download_images(
        self,
        image_urls: list[str],
        output_dir: Path,
    ) -> list[ImageResult]:
        """Download images from URLs to output_dir synchronously via httpx."""
        import httpx
        output_dir.mkdir(parents=True, exist_ok=True)
        results: list[ImageResult] = []
        for i, url in enumerate(image_urls, start=1):
            filename = f"fig{i:03d}{Path(url).suffix or '.png'}"
            dest = output_dir / filename
            try:
                with httpx.Client(timeout=30) as client:
                    resp = client.get(url)
                    resp.raise_for_status()
                    dest.write_bytes(resp.content)
                ocr_text = self.ocr_image(dest)
                results.append(ImageResult(url=url, local_path=dest, ocr_text=ocr_text, figure_number=i))
            except Exception as e:
                results.append(ImageResult(url=url, local_path=dest, ocr_text=None, figure_number=i))
        return results

    def ocr_image(self, image_path: Path) -> str | None:
        """Run tesseract OCR on image_path; return text or None if unavailable."""
        if shutil.which("tesseract") is None:
            return None
        try:
            result = subprocess.run(
                ["tesseract", str(image_path), "stdout", "-l", "eng"],
                capture_output=True,
                text=True,
                timeout=30,
            )
            if result.returncode == 0:
                return result.stdout.strip() or None
            return None
        except (subprocess.TimeoutExpired, Exception):
            return None

    def assemble_markdown(
        self,
        base_md: str,
        metadata: "PatentMetadata",
        images: list[ImageResult],
    ) -> str:
        """Assemble final patent.md from base markdown, metadata, and images."""
        parts: list[str] = []

        # Header
        title = metadata.title or metadata.canonical_id
        parts.append(f"# {title}")
        parts.append("")

        # Metadata block
        meta_lines: list[str] = []
        if metadata.canonical_id:
            meta_lines.append(f"**Patent ID:** {metadata.canonical_id}")
        if metadata.inventors:
            meta_lines.append(f"**Inventors:** {', '.join(metadata.inventors)}")
        if metadata.assignee:
            meta_lines.append(f"**Assignee:** {metadata.assignee}")
        if metadata.filing_date:
            meta_lines.append(f"**Filing Date:** {metadata.filing_date}")
        if metadata.publication_date:
            meta_lines.append(f"**Publication Date:** {metadata.publication_date}")
        if metadata.grant_date:
            meta_lines.append(f"**Grant Date:** {metadata.grant_date}")
        if meta_lines:
            parts.extend(meta_lines)
            parts.append("")

        # Abstract
        if metadata.abstract:
            parts.append("## Abstract")
            parts.append("")
            parts.append(metadata.abstract)
            parts.append("")

        # Body
        if base_md:
            parts.append(base_md)
            parts.append("")

        # Figures
        if images:
            parts.append("## Figures")
            parts.append("")
            for img in images:
                rel_path = img.local_path.name
                parts.append(f"![Figure {img.figure_number}]({rel_path})")
                if img.ocr_text:
                    parts.append(f"*{img.ocr_text}*")
                parts.append("")

        return "\n".join(parts)
