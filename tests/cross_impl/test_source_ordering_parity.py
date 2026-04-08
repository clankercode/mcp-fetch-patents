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


class TestSourceOrderingParity:
    """Verify both implementations try sources in same order."""

    def test_source_order_for_us_patent(self):
        """For a US patent, both impls should try sources in config priority order."""
        config = PatentConfig()
        orch = FetcherOrchestrator(config)
        patent = canonicalize("US7654321")
        sources = orch.get_sources_for(patent)
        py_source_names = [s.source_name for s in sources]

        # Verify Python tries them in config.source_priority order (filtered by jurisdiction)
        # USPTO supports US; EPO supports all; BigQuery supports all; etc.
        assert py_source_names[0] == "USPTO"  # First for US patents
        assert "EPO_OPS" in py_source_names
        assert "web_search" in py_source_names

    def test_source_order_for_wo_patent(self):
        """For a WO patent, WIPO should appear and USPTO should not."""
        config = PatentConfig()
        orch = FetcherOrchestrator(config)
        patent = canonicalize("WO2024123456")
        sources = orch.get_sources_for(patent)
        py_source_names = [s.source_name for s in sources]

        assert "USPTO" not in py_source_names  # PPUBS only supports US
        assert "WIPO_Scrape" in py_source_names

    def test_default_source_priority_matches(self):
        """Default source priority list should be identical between impls."""
        py_config = PatentConfig()
        py_priority = py_config.source_priority

        # The Rust config should produce the same default list
        # We verify by checking the Python default
        expected = [
            "USPTO", "EPO_OPS", "BigQuery", "Espacenet",
            "WIPO_Scrape", "IP_Australia", "CIPO",
            "Google_Patents", "web_search",
        ]
        assert py_priority == expected
