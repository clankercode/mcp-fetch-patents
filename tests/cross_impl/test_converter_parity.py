"""Cross-impl parity: converters produce identical markdown assembly output.

Rust parity: the Rust binary has no 'convert' subcommand, so converter output
cannot be verified via CLI.  A placeholder test is marked skip below.
"""

from __future__ import annotations

import sys
from pathlib import Path

import pytest

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


class TestPythonConverter:
    """Python converter pipeline tests."""

    def test_check_available_tools_keys_match(self):
        """Both implementations should check the same set of tools."""
        config = PatentConfig()
        py_tools = check_available_tools(config)
        expected_keys = set(config.converters_order) | {"tesseract"}
        assert set(py_tools.keys()) == expected_keys

    def test_assemble_markdown_matches(self):
        """Feed identical metadata to Python converter, verify output structure."""
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

        assert py_md.startswith("# Test Patent Title")
        assert "**Patent ID:** US1234567" in py_md
        assert "**Inventors:** Alice, Bob" in py_md
        assert "## Abstract" in py_md
        assert "## Figures" in py_md
        assert "![Figure 1](fig001.png)" in py_md


@pytest.mark.skip(
    reason="Rust binary has no 'convert' subcommand; converter parity cannot be tested via CLI"
)
def test_rust_converter_parity(rust_binary: str):
    """Rust converter parity -- blocked on CLI convert subcommand.

    The Rust binary (patent-mcp-server) exposes 'canonicalize', 'plan', and
    'rank' subcommands but no 'convert' subcommand.  To achieve full converter
    parity, add a 'convert' subcommand that accepts metadata JSON on stdin and
    prints assembled markdown to stdout.
    """
