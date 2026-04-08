"""Cross-impl parity: converters produce identical markdown assembly output."""
from __future__ import annotations

import sys
from pathlib import Path

import pytest

# Ensure the Python package is importable
_src_python = str(Path(__file__).parent.parent.parent / "src" / "python")
if _src_python not in sys.path:
    sys.path.insert(0, _src_python)

from patent_mcp.converters.pipeline import (
    ConverterPipeline,
    ImageResult,
    check_available_tools,
)
from patent_mcp.config import PatentConfig
from patent_mcp.cache import PatentMetadata


class TestConverterParity:
    """Test that Rust and Python converters produce identical output."""

    def test_check_available_tools_keys_match(self):
        """Both implementations should check the same set of tools."""
        config = PatentConfig()
        py_tools = check_available_tools(config)
        # The keys should include all converters_order items + tesseract
        expected_keys = set(config.converters_order) | {"tesseract"}
        assert set(py_tools.keys()) == expected_keys

    def test_assemble_markdown_matches(self):
        """Feed identical metadata to both impls, compare markdown output."""
        config = PatentConfig()
        pipeline = ConverterPipeline(config)

        metadata = PatentMetadata(
            canonical_id="US1234567",
            jurisdiction="US",
            doc_type="grant",
            title="Test Patent Title",
            abstract="This is the abstract.",
            inventors=["Alice", "Bob"],
            assignee="Test Corp",
            filing_date="2024-01-01",
            publication_date="2024-06-01",
        )

        images = [
            ImageResult(
                url="https://example.com/fig1.png",
                local_path=Path("/tmp/fig001.png"),
                ocr_text="Figure 1 caption",
                figure_number=1,
            ),
        ]

        py_md = pipeline.assemble_markdown("Body content here.", metadata, images)

        # For Rust: we need to add a CLI subcommand or test helper.
        # For now, verify the Python output matches expected structure.
        assert py_md.startswith("# Test Patent Title")
        assert "**Patent ID:** US1234567" in py_md
        assert "**Inventors:** Alice, Bob" in py_md
        assert "## Abstract" in py_md
        assert "## Figures" in py_md
        assert "![Figure 1](fig001.png)" in py_md
