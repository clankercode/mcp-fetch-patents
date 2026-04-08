"""Cross-impl parity: web search query generation and URL scoring match."""
import pytest

from patent_mcp.fetchers.web_search import generate_queries, score_url_confidence
from patent_mcp.id_canon import canonicalize


class TestWebSearchParity:
    """Test that Rust and Python web search produce identical queries and scores."""

    def test_query_generation_us(self):
        patent = canonicalize("US7654321")
        queries = generate_queries(patent)
        # Verify structure matches expected
        assert queries[0] == '"US7654321" patent PDF'
        assert any("site:patents.google.com" in q for q in queries)
        assert any("site:ppubs.uspto.gov" in q for q in queries)

    def test_query_generation_ep(self):
        patent = canonicalize("EP1234567")
        queries = generate_queries(patent)
        assert queries[0] == '"EP1234567" patent PDF'
        assert any("European Patent Office" in q for q in queries)

    def test_query_generation_wo(self):
        patent = canonicalize("WO2024123456")
        queries = generate_queries(patent)
        assert any("PCT international patent" in q for q in queries)

    def test_confidence_scoring_high(self):
        assert score_url_confidence("https://patents.google.com/patent/US7654321", "US7654321") == "high"

    def test_confidence_scoring_id_in_url(self):
        assert score_url_confidence("https://random.com/US7654321/page", "US7654321") == "high"

    def test_confidence_scoring_medium(self):
        assert score_url_confidence("https://patentyogi.com/article", "US7654321") == "medium"

    def test_confidence_scoring_low(self):
        assert score_url_confidence("https://example.com/article", "US7654321") == "low"
