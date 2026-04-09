"""Tests for patent_mcp.search.server — MCP tool functions.

Tests session tools, search tool routing, and helper functions.
Marks slow due to httpx/respx usage in search tests.
"""
from __future__ import annotations

import json
from unittest.mock import MagicMock, patch

import pytest

pytestmark = pytest.mark.slow


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _make_session_manager(tmp_path):
    from patent_mcp.search.session_manager import SessionManager
    return SessionManager(sessions_dir=tmp_path)


# ---------------------------------------------------------------------------
# Session tool tests
# ---------------------------------------------------------------------------

class TestPatentSessionCreate:
    def test_creates_session_and_returns_id(self, tmp_path):
        from patent_mcp.search import server

        with patch.object(server, "_get_session_manager", return_value=_make_session_manager(tmp_path)):
            result = server.patent_session_create(topic="test-topic")
        assert "session_id" in result
        assert result["topic"] == "test-topic"
        assert "test-topic" in result["session_id"]

    def test_with_prior_art_cutoff(self, tmp_path):
        from patent_mcp.search import server

        with patch.object(server, "_get_session_manager", return_value=_make_session_manager(tmp_path)):
            result = server.patent_session_create(topic="prior-art", prior_art_cutoff="2020-01-01")
        assert "session_id" in result

    def test_returns_sessions_dir(self, tmp_path):
        from patent_mcp.search import server

        with patch.object(server, "_get_session_manager", return_value=_make_session_manager(tmp_path)):
            result = server.patent_session_create(topic="my-session")
        assert "sessions_dir" in result
        assert str(tmp_path) in result["sessions_dir"]

    def test_message_includes_session_id(self, tmp_path):
        from patent_mcp.search import server

        with patch.object(server, "_get_session_manager", return_value=_make_session_manager(tmp_path)):
            result = server.patent_session_create(topic="my-session")
        assert result["session_id"] in result["message"]


class TestPatentSessionLoad:
    def test_loads_existing_session(self, tmp_path):
        from patent_mcp.search import server, session_manager

        sm = _make_session_manager(tmp_path)
        s = sm.create_session("test topic")
        sm.save_session(s)

        with patch.object(server, "_get_session_manager", return_value=sm):
            result = server.patent_session_load(s.session_id)
        assert result["session_id"] == s.session_id
        assert result["topic"] == "test topic"

    def test_missing_session_returns_error(self, tmp_path):
        from patent_mcp.search import server

        with patch.object(server, "_get_session_manager", return_value=_make_session_manager(tmp_path)):
            result = server.patent_session_load("nonexistent-session-id")
        assert "error" in result


class TestPatentSessionList:
    def test_empty_returns_empty_list(self, tmp_path):
        from patent_mcp.search import server

        with patch.object(server, "_get_session_manager", return_value=_make_session_manager(tmp_path)):
            result = server.patent_session_list()
        assert result["sessions"] == []
        assert result["total"] == 0

    def test_lists_created_sessions(self, tmp_path):
        from patent_mcp.search import server

        sm = _make_session_manager(tmp_path)
        for topic in ["topic-a", "topic-b", "topic-c"]:
            s = sm.create_session(topic)
            sm.save_session(s)

        with patch.object(server, "_get_session_manager", return_value=sm):
            result = server.patent_session_list()
        assert result["total"] == 3
        topics = [s["topic"] for s in result["sessions"]]
        assert set(topics) == {"topic-a", "topic-b", "topic-c"}

    def test_respects_limit(self, tmp_path):
        from patent_mcp.search import server

        sm = _make_session_manager(tmp_path)
        for i in range(5):
            s = sm.create_session(f"topic-{i}")
            sm.save_session(s)

        with patch.object(server, "_get_session_manager", return_value=sm):
            result = server.patent_session_list(limit=2)
        assert result["total"] <= 2


class TestPatentSessionNote:
    def test_adds_note_to_session(self, tmp_path):
        from patent_mcp.search import server

        sm = _make_session_manager(tmp_path)
        s = sm.create_session("session with note")
        sm.save_session(s)

        with patch.object(server, "_get_session_manager", return_value=sm):
            result = server.patent_session_note(s.session_id, "this is my note")
        assert result["status"] == "note added"

        reloaded = sm.load_session(s.session_id)
        assert "this is my note" in reloaded.notes

    def test_missing_session_returns_error(self, tmp_path):
        from patent_mcp.search import server

        with patch.object(server, "_get_session_manager", return_value=_make_session_manager(tmp_path)):
            result = server.patent_session_note("bad-id", "note")
        assert "error" in result


class TestPatentSessionAnnotate:
    def test_annotates_patent_in_session(self, tmp_path):
        from patent_mcp.search import server, session_manager

        sm = _make_session_manager(tmp_path)
        s = sm.create_session("annotate test")
        # Add a query with a patent
        hit = session_manager.PatentHit(patent_id="US10000001B2", title="Test Patent")
        q = session_manager.QueryRecord(
            query_id="q001", timestamp="2026-01-01T00:00:00+00:00",
            source="USPTO", query_text="test", result_count=1, results=[hit]
        )
        sm.append_query_result(s.session_id, q)

        with patch.object(server, "_get_session_manager", return_value=sm):
            result = server.patent_session_annotate(s.session_id, "US10000001B2", "highly relevant", "high")
        assert result["relevance"] == "high"
        assert result["patent_id"] == "US10000001B2"


class TestPatentSessionExport:
    def test_export_creates_report(self, tmp_path):
        from patent_mcp.search import server

        sm = _make_session_manager(tmp_path)
        s = sm.create_session("export test")
        sm.save_session(s)

        with patch.object(server, "_get_session_manager", return_value=sm):
            result = server.patent_session_export(s.session_id)
        assert result["status"] == "exported"
        assert "report_path" in result
        from pathlib import Path
        assert Path(result["report_path"]).exists()

    def test_export_missing_session_returns_error(self, tmp_path):
        from patent_mcp.search import server

        with patch.object(server, "_get_session_manager", return_value=_make_session_manager(tmp_path)):
            result = server.patent_session_export("bad-id")
        assert "error" in result


# ---------------------------------------------------------------------------
# Search tool tests (mocked backends)
# ---------------------------------------------------------------------------

class TestPatentSearchNatural:
    def test_no_serpapi_key_returns_empty(self, tmp_path):
        from patent_mcp.search import server
        from patent_mcp.config import PatentConfig

        cfg = PatentConfig(serpapi_key=None)
        with patch.object(server, "_get_config", return_value=cfg):
            result = server.patent_search_natural("wireless charging through metal")
        assert result["total_found"] == 0

    def test_with_serpapi_calls_backend(self, tmp_path):
        from patent_mcp.search import server, searchers
        from patent_mcp.config import PatentConfig

        cfg = PatentConfig(serpapi_key="test-key-123")
        mock_hit = searchers.PatentHit(
            patent_id="US10000001B2",
            title="Test Patent",
            source="Google_Patents",
        )

        async def fake_search(**kwargs):
            return [mock_hit]

        with patch.object(server, "_get_config", return_value=cfg), \
             patch("patent_mcp.search.searchers.SerpApiGooglePatentsBackend") as MockBackend:
            MockBackend.return_value.search = fake_search
            result = server.patent_search_natural("wireless charging")

        assert result["total_found"] == 1
        assert result["results"][0]["patent_id"] == "US10000001B2"

    def test_date_cutoff_passed_to_backend(self):
        from patent_mcp.search import server, searchers
        from patent_mcp.config import PatentConfig

        cfg = PatentConfig(serpapi_key="test-key-123")
        calls = []

        async def fake_search(**kwargs):
            calls.append(kwargs)
            return []

        with patch.object(server, "_get_config", return_value=cfg), \
             patch("patent_mcp.search.searchers.SerpApiGooglePatentsBackend") as MockBackend:
            MockBackend.return_value.search = fake_search
            server.patent_search_natural("test", date_cutoff="2020-01-01")

        # Planner generates multiple query variants, so SerpAPI may be called multiple times
        assert len(calls) >= 1
        assert all(c["date_to"] == "2020-01-01" for c in calls)

    def test_deduplicates_results(self):
        from patent_mcp.search import server, searchers
        from patent_mcp.config import PatentConfig

        cfg = PatentConfig(serpapi_key="test-key-123")
        hit = searchers.PatentHit(patent_id="US10000001B2", title="Dup", source="Google_Patents")

        async def fake_search(**kwargs):
            return [hit, hit]  # duplicate

        with patch.object(server, "_get_config", return_value=cfg), \
             patch("patent_mcp.search.searchers.SerpApiGooglePatentsBackend") as MockBackend:
            MockBackend.return_value.search = fake_search
            result = server.patent_search_natural("test")

        assert result["total_found"] == 1  # deduplicated


class TestPatentSuggestQueries:
    def test_returns_strategy_dict(self):
        from patent_mcp.search.server import patent_suggest_queries
        result = patent_suggest_queries("wireless charging through metal")
        assert "topic" in result
        assert "strategy" in result
        assert "step_1_natural_search" in result["strategy"]
        assert "step_2_classification" in result["strategy"]
        # Planner output is included
        assert "planner_output" in result
        assert "query_variants" in result["planner_output"]

    def test_prior_art_cutoff_adds_notes(self):
        from patent_mcp.search.server import patent_suggest_queries
        result = patent_suggest_queries("wireless charging", prior_art_cutoff="2020-01-01")
        assert "prior_art_notes" in result["strategy"]
        assert "2020-01-01" in result["strategy"]["prior_art_notes"]["cutoff_date"]

    def test_no_cutoff_no_prior_art_notes(self):
        from patent_mcp.search.server import patent_suggest_queries
        result = patent_suggest_queries("wireless charging")
        assert "prior_art_notes" not in result["strategy"]


class TestHitToDict:
    def test_converts_patent_hit(self):
        from patent_mcp.search.server import _hit_to_dict
        from patent_mcp.search.searchers import PatentHit
        hit = PatentHit(
            patent_id="US10000001B2",
            title="Test",
            date="2020-01-01",
            assignee="Apple Inc.",
            inventors=["John Doe"],
            abstract="A method...",
            source="USPTO",
            relevance="high",
            url="https://patents.google.com/...",
        )
        d = _hit_to_dict(hit)
        assert d["patent_id"] == "US10000001B2"
        assert d["title"] == "Test"
        assert d["assignee"] == "Apple Inc."
        assert d["url"] == "https://patents.google.com/..."

    def test_handles_none_fields(self):
        from patent_mcp.search.server import _hit_to_dict
        from patent_mcp.search.searchers import PatentHit
        hit = PatentHit(patent_id="US10000001B2")
        d = _hit_to_dict(hit)
        assert d["title"] is None
        assert d["abstract"] is None
