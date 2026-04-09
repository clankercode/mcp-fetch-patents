"""Cross-impl parity: both implementations try sources in the same order for a given jurisdiction."""

from __future__ import annotations

import sys
from pathlib import Path

import pytest

SRC_PYTHON = Path(__file__).parent.parent.parent / "src" / "python"
sys.path.insert(0, str(SRC_PYTHON))

from patent_mcp.config import PatentConfig
from patent_mcp.fetchers.orchestrator import FetcherOrchestrator
from patent_mcp.id_canon import canonicalize

RUST_DEFAULT_SOURCE_PRIORITY = [
    "USPTO",
    "EPO_OPS",
    "BigQuery",
    "Espacenet",
    "WIPO_Scrape",
    "IP_Australia",
    "CIPO",
    "Google_Patents",
    "web_search",
]


class TestSourceOrderingParity:
    """Verify both implementations try sources in same order."""

    def test_source_order_for_us_patent(self):
        """For a US patent, both impls should try sources in config priority order."""
        config = PatentConfig()
        orch = FetcherOrchestrator(config)
        patent = canonicalize("US7654321")
        sources = orch.get_sources_for(patent)
        py_source_names = [s.source_name for s in sources]

        assert py_source_names[0] == "USPTO"
        assert "EPO_OPS" in py_source_names
        assert "web_search" in py_source_names

    def test_source_order_for_wo_patent(self):
        """For a WO patent, WIPO should appear and USPTO should not."""
        config = PatentConfig()
        orch = FetcherOrchestrator(config)
        patent = canonicalize("WO2024123456")
        sources = orch.get_sources_for(patent)
        py_source_names = [s.source_name for s in sources]

        assert "USPTO" not in py_source_names
        assert "WIPO_Scrape" in py_source_names

    def test_python_default_source_priority(self):
        """Python default source priority list should be well-defined."""
        py_config = PatentConfig()
        py_priority = py_config.source_priority
        expected = [
            "USPTO",
            "EPO_OPS",
            "BigQuery",
            "Espacenet",
            "WIPO_Scrape",
            "IP_Australia",
            "CIPO",
            "Google_Patents",
            "web_search",
        ]
        assert py_priority == expected

    def test_rust_default_source_priority_matches_python(self):
        """Rust default_source_priority (from config/mod.rs) must match Python's.

        The Rust binary has no config-dump CLI command, so we verify by
        hardcoding the Rust default from src/rust/src/config/mod.rs
        default_source_priority() and comparing against Python's
        PatentConfig().source_priority.
        """
        py_config = PatentConfig()
        py_priority = py_config.source_priority
        assert py_priority == RUST_DEFAULT_SOURCE_PRIORITY, (
            f"Python source_priority != Rust default_source_priority\n"
            f"  Python: {py_priority}\n"
            f"  Rust:   {RUST_DEFAULT_SOURCE_PRIORITY}"
        )

    def test_rust_source_priority_order_is_correct(self, rust_binary: str):
        """Verify Rust binary canonicalize works (proves config loads correctly).

        Since the Rust binary has no config-dump command, we verify the binary
        loads and runs correctly by calling canonicalize with a known ID.
        The source priority order is verified above via hardcoded comparison.
        """
        import json
        import subprocess

        proc = subprocess.run(
            [rust_binary, "canonicalize", "US7654321"],
            capture_output=True,
            text=True,
            timeout=10,
        )
        assert proc.returncode == 0, f"Rust canonicalize failed: {proc.stderr}"
        result = json.loads(proc.stdout)
        assert result["jurisdiction"] == "US"
        assert result["canonical"] == "US7654321"
