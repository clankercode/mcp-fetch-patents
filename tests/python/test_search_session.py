"""Tests for patent_mcp.search.session_manager."""
from __future__ import annotations

import json
import re
import threading
import time
from pathlib import Path

import pytest

from patent_mcp.search.session_manager import (
    PatentHit,
    QueryRecord,
    Session,
    SessionManager,
    SessionSummary,
)


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _make_hit(
    patent_id: str = "US10000001B2",
    relevance: str = "high",
    note: str = "",
    date: str = "2020-01-15",
) -> PatentHit:
    return PatentHit(
        patent_id=patent_id,
        title=f"Title for {patent_id}",
        date=date,
        assignee="ACME Corp",
        inventors=["Alice", "Bob"],
        abstract="An abstract.",
        source="USPTO",
        relevance=relevance,
        note=note,
        prior_art=None,
    )


def _make_query(
    query_id: str = "q001",
    hits: list[PatentHit] | None = None,
    source: str = "USPTO",
) -> QueryRecord:
    if hits is None:
        hits = [_make_hit()]
    return QueryRecord(
        query_id=query_id,
        timestamp="2026-04-07T14:00:00+00:00",
        source=source,
        query_text="TTL/(wireless AND charging)",
        result_count=len(hits),
        results=hits,
    )


# ---------------------------------------------------------------------------
# T01 — Create session
# ---------------------------------------------------------------------------

class TestCreateSession:
    def test_session_id_format(self, tmp_path):
        mgr = SessionManager(tmp_path)
        session = mgr.create_session("Wireless Charging Through Metal Objects")
        # Should be YYYYMMDD-HHMMSS-<slug>
        assert re.match(r"^\d{8}-\d{6}-", session.session_id)

    def test_session_id_contains_slug(self, tmp_path):
        mgr = SessionManager(tmp_path)
        session = mgr.create_session("Wireless Charging")
        assert "wireless-charging" in session.session_id

    def test_session_id_slug_max_30(self, tmp_path):
        mgr = SessionManager(tmp_path)
        long_topic = "a" * 100
        session = mgr.create_session(long_topic)
        # slug portion after "YYYYMMDD-HHMMSS-" should be ≤ 30 chars
        parts = session.session_id.split("-", 2)
        assert len(parts) == 3
        assert len(parts[2]) <= 30

    def test_session_id_slug_only_alphanumeric_hyphens(self, tmp_path):
        mgr = SessionManager(tmp_path)
        session = mgr.create_session("Special! @ Characters #$%")
        slug = session.session_id.split("-", 2)[2]
        assert re.match(r"^[a-z0-9\-]*$", slug)

    def test_fields_set_correctly(self, tmp_path):
        mgr = SessionManager(tmp_path)
        session = mgr.create_session("My Topic", prior_art_cutoff="2020-01-01", notes="initial note")
        assert session.topic == "My Topic"
        assert session.prior_art_cutoff == "2020-01-01"
        assert session.notes == "initial note"
        assert session.queries == []
        assert session.classifications_explored == []
        assert session.citation_chains == {}
        assert session.patent_families == {}

    def test_created_at_and_modified_at_set(self, tmp_path):
        mgr = SessionManager(tmp_path)
        session = mgr.create_session("Topic")
        assert session.created_at
        assert session.modified_at

    def test_session_file_created(self, tmp_path):
        mgr = SessionManager(tmp_path)
        session = mgr.create_session("Topic")
        assert (tmp_path / f"{session.session_id}.json").exists()

    def test_index_created(self, tmp_path):
        mgr = SessionManager(tmp_path)
        mgr.create_session("Topic")
        assert (tmp_path / ".index.json").exists()


# ---------------------------------------------------------------------------
# T02 — Load session (round-trip)
# ---------------------------------------------------------------------------

class TestLoadSession:
    def test_round_trip(self, tmp_path):
        mgr = SessionManager(tmp_path)
        original = mgr.create_session("Wireless", prior_art_cutoff="2019-06-01", notes="note1")
        loaded = mgr.load_session(original.session_id)
        assert loaded.session_id == original.session_id
        assert loaded.topic == "Wireless"
        assert loaded.prior_art_cutoff == "2019-06-01"
        assert loaded.notes == "note1"

    def test_load_with_queries(self, tmp_path):
        mgr = SessionManager(tmp_path)
        session = mgr.create_session("Topic")
        query = _make_query(hits=[_make_hit("US10000001B2"), _make_hit("US10000002B2")])
        mgr.append_query_result(session.session_id, query)
        loaded = mgr.load_session(session.session_id)
        assert len(loaded.queries) == 1
        assert len(loaded.queries[0].results) == 2
        assert loaded.queries[0].results[0].patent_id == "US10000001B2"

    def test_load_missing_raises_file_not_found(self, tmp_path):
        mgr = SessionManager(tmp_path)
        with pytest.raises(FileNotFoundError):
            mgr.load_session("nonexistent-session-id")

    def test_patent_hit_fields_preserved(self, tmp_path):
        mgr = SessionManager(tmp_path)
        session = mgr.create_session("Topic")
        hit = PatentHit(
            patent_id="EP3456789A1",
            title="My Title",
            date="2021-03-10",
            assignee="Big Corp",
            inventors=["Charlie", "Diana"],
            abstract="Some abstract text.",
            source="EPO_OPS",
            relevance="medium",
            note="looks relevant",
            prior_art=True,
        )
        query = _make_query(hits=[hit])
        mgr.append_query_result(session.session_id, query)
        loaded = mgr.load_session(session.session_id)
        r = loaded.queries[0].results[0]
        assert r.patent_id == "EP3456789A1"
        assert r.title == "My Title"
        assert r.date == "2021-03-10"
        assert r.assignee == "Big Corp"
        assert r.inventors == ["Charlie", "Diana"]
        assert r.abstract == "Some abstract text."
        assert r.source == "EPO_OPS"
        assert r.relevance == "medium"
        assert r.note == "looks relevant"
        assert r.prior_art is True


# ---------------------------------------------------------------------------
# T03 — List sessions
# ---------------------------------------------------------------------------

class TestListSessions:
    def test_list_empty(self, tmp_path):
        mgr = SessionManager(tmp_path)
        assert mgr.list_sessions() == []

    def test_list_one_session(self, tmp_path):
        mgr = SessionManager(tmp_path)
        session = mgr.create_session("Topic A")
        summaries = mgr.list_sessions()
        assert len(summaries) == 1
        assert summaries[0].session_id == session.session_id
        assert summaries[0].topic == "Topic A"

    def test_list_sorted_by_modified_at_desc(self, tmp_path):
        mgr = SessionManager(tmp_path)
        s1 = mgr.create_session("First Topic")
        # Ensure different timestamps — sleep briefly OR manually set modified_at
        time.sleep(0.01)
        s2 = mgr.create_session("Second Topic")
        summaries = mgr.list_sessions()
        # Most recently modified first
        assert summaries[0].session_id == s2.session_id
        assert summaries[1].session_id == s1.session_id

    def test_list_summary_has_correct_query_count(self, tmp_path):
        mgr = SessionManager(tmp_path)
        session = mgr.create_session("Topic")
        mgr.append_query_result(session.session_id, _make_query("q001"))
        mgr.append_query_result(session.session_id, _make_query("q002"))
        summaries = mgr.list_sessions()
        assert summaries[0].query_count == 2

    def test_list_summary_has_correct_patent_count(self, tmp_path):
        mgr = SessionManager(tmp_path)
        session = mgr.create_session("Topic")
        # Two queries each with same patent — deduplicated count should be 1
        q1 = _make_query("q001", hits=[_make_hit("US10000001B2"), _make_hit("US10000002B2")])
        q2 = _make_query("q002", hits=[_make_hit("US10000001B2")])  # duplicate
        mgr.append_query_result(session.session_id, q1)
        mgr.append_query_result(session.session_id, q2)
        summaries = mgr.list_sessions()
        assert summaries[0].patent_count == 2  # 2 unique patents

    def test_list_with_limit(self, tmp_path):
        mgr = SessionManager(tmp_path)
        for i in range(5):
            time.sleep(0.01)
            mgr.create_session(f"Topic {i}")
        summaries = mgr.list_sessions(limit=3)
        assert len(summaries) == 3

    def test_list_falls_back_to_scan_when_no_index(self, tmp_path):
        mgr = SessionManager(tmp_path)
        s = mgr.create_session("Topic")
        # Remove the index file
        (tmp_path / ".index.json").unlink()
        summaries = mgr.list_sessions()
        assert len(summaries) == 1
        assert summaries[0].session_id == s.session_id


# ---------------------------------------------------------------------------
# T04 — Append query result
# ---------------------------------------------------------------------------

class TestAppendQueryResult:
    def test_append_adds_query(self, tmp_path):
        mgr = SessionManager(tmp_path)
        session = mgr.create_session("Topic")
        mgr.append_query_result(session.session_id, _make_query("q001"))
        loaded = mgr.load_session(session.session_id)
        assert len(loaded.queries) == 1

    def test_append_multiple_queries(self, tmp_path):
        mgr = SessionManager(tmp_path)
        session = mgr.create_session("Topic")
        mgr.append_query_result(session.session_id, _make_query("q001"))
        mgr.append_query_result(session.session_id, _make_query("q002"))
        loaded = mgr.load_session(session.session_id)
        assert len(loaded.queries) == 2
        assert loaded.queries[0].query_id == "q001"
        assert loaded.queries[1].query_id == "q002"

    def test_append_updates_modified_at(self, tmp_path):
        mgr = SessionManager(tmp_path)
        session = mgr.create_session("Topic")
        old_modified = session.modified_at
        time.sleep(0.01)
        mgr.append_query_result(session.session_id, _make_query())
        loaded = mgr.load_session(session.session_id)
        assert loaded.modified_at >= old_modified

    def test_append_updates_index(self, tmp_path):
        mgr = SessionManager(tmp_path)
        session = mgr.create_session("Topic")
        mgr.append_query_result(session.session_id, _make_query())
        index = json.loads((tmp_path / ".index.json").read_text())
        entry = next(s for s in index["sessions"] if s["session_id"] == session.session_id)
        assert entry["query_count"] == 1


# ---------------------------------------------------------------------------
# T05 — Add note
# ---------------------------------------------------------------------------

class TestAddNote:
    def test_add_note_to_empty(self, tmp_path):
        mgr = SessionManager(tmp_path)
        session = mgr.create_session("Topic")
        mgr.add_note(session.session_id, "First note")
        loaded = mgr.load_session(session.session_id)
        assert loaded.notes == "First note"

    def test_add_note_appends_with_separator(self, tmp_path):
        mgr = SessionManager(tmp_path)
        session = mgr.create_session("Topic", notes="Existing note")
        mgr.add_note(session.session_id, "Second note")
        loaded = mgr.load_session(session.session_id)
        assert loaded.notes == "Existing note\n\nSecond note"

    def test_add_note_twice(self, tmp_path):
        mgr = SessionManager(tmp_path)
        session = mgr.create_session("Topic")
        mgr.add_note(session.session_id, "Note 1")
        mgr.add_note(session.session_id, "Note 2")
        loaded = mgr.load_session(session.session_id)
        assert loaded.notes == "Note 1\n\nNote 2"


# ---------------------------------------------------------------------------
# T06 — Annotate patent
# ---------------------------------------------------------------------------

class TestAnnotatePatent:
    def test_annotate_updates_relevance(self, tmp_path):
        mgr = SessionManager(tmp_path)
        session = mgr.create_session("Topic")
        query = _make_query(hits=[_make_hit("US10000001B2", relevance="unknown")])
        mgr.append_query_result(session.session_id, query)
        mgr.annotate_patent(session.session_id, "US10000001B2", "Very relevant", "high")
        loaded = mgr.load_session(session.session_id)
        hit = loaded.queries[0].results[0]
        assert hit.relevance == "high"
        assert hit.note == "Very relevant"

    def test_annotate_patent_not_found_no_error(self, tmp_path):
        mgr = SessionManager(tmp_path)
        session = mgr.create_session("Topic")
        mgr.append_query_result(session.session_id, _make_query())
        # Should not raise even if patent_id not found
        mgr.annotate_patent(session.session_id, "NONEXISTENT", "note", "low")

    def test_annotate_patent_in_second_query(self, tmp_path):
        mgr = SessionManager(tmp_path)
        session = mgr.create_session("Topic")
        q1 = _make_query("q001", hits=[_make_hit("US10000001B2")])
        q2 = _make_query("q002", hits=[_make_hit("US10000002B2", relevance="unknown")])
        mgr.append_query_result(session.session_id, q1)
        mgr.append_query_result(session.session_id, q2)
        mgr.annotate_patent(session.session_id, "US10000002B2", "Found it", "medium")
        loaded = mgr.load_session(session.session_id)
        hit = loaded.queries[1].results[0]
        assert hit.relevance == "medium"
        assert hit.note == "Found it"

    def test_annotate_patent_in_multiple_queries(self, tmp_path):
        """Same patent_id in two queries — both should be updated."""
        mgr = SessionManager(tmp_path)
        session = mgr.create_session("Topic")
        q1 = _make_query("q001", hits=[_make_hit("US10000001B2", relevance="unknown")])
        q2 = _make_query("q002", hits=[_make_hit("US10000001B2", relevance="unknown")])
        mgr.append_query_result(session.session_id, q1)
        mgr.append_query_result(session.session_id, q2)
        mgr.annotate_patent(session.session_id, "US10000001B2", "Both updated", "high")
        loaded = mgr.load_session(session.session_id)
        for q in loaded.queries:
            assert q.results[0].relevance == "high"
            assert q.results[0].note == "Both updated"


# ---------------------------------------------------------------------------
# T07 — Export Markdown
# ---------------------------------------------------------------------------

class TestExportMarkdown:
    def test_export_creates_file(self, tmp_path):
        mgr = SessionManager(tmp_path)
        session = mgr.create_session("Wireless Charging")
        result_path = mgr.export_markdown(session.session_id)
        assert result_path.exists()

    def test_export_default_path(self, tmp_path):
        mgr = SessionManager(tmp_path)
        session = mgr.create_session("Wireless Charging")
        result_path = mgr.export_markdown(session.session_id)
        assert result_path == tmp_path / f"{session.session_id}-report.md"

    def test_export_custom_path(self, tmp_path):
        mgr = SessionManager(tmp_path)
        session = mgr.create_session("Topic")
        custom = tmp_path / "my_report.md"
        result_path = mgr.export_markdown(session.session_id, output_path=custom)
        assert result_path == custom
        assert custom.exists()

    def test_export_contains_topic(self, tmp_path):
        mgr = SessionManager(tmp_path)
        session = mgr.create_session("Wireless Charging Through Metal Objects")
        content = mgr.export_markdown(session.session_id).read_text()
        assert "Wireless Charging Through Metal Objects" in content

    def test_export_contains_patent_id(self, tmp_path):
        mgr = SessionManager(tmp_path)
        session = mgr.create_session("Topic")
        query = _make_query(hits=[_make_hit("US10999888B2")])
        mgr.append_query_result(session.session_id, query)
        content = mgr.export_markdown(session.session_id).read_text()
        assert "US10999888B2" in content

    def test_export_contains_notes(self, tmp_path):
        mgr = SessionManager(tmp_path)
        session = mgr.create_session("Topic", notes="This is my note.")
        content = mgr.export_markdown(session.session_id).read_text()
        assert "This is my note." in content

    def test_export_contains_query_text(self, tmp_path):
        mgr = SessionManager(tmp_path)
        session = mgr.create_session("Topic")
        q = QueryRecord(
            query_id="q001",
            timestamp="2026-04-07T10:00:00+00:00",
            source="USPTO",
            query_text="TTL/(wireless AND charging) AND ACLM/(metal)",
            result_count=1,
            results=[_make_hit()],
        )
        mgr.append_query_result(session.session_id, q)
        content = mgr.export_markdown(session.session_id).read_text()
        assert "TTL/(wireless AND charging) AND ACLM/(metal)" in content

    def test_export_includes_query_status_and_error(self, tmp_path):
        mgr = SessionManager(tmp_path)
        session = mgr.create_session("Topic")
        q = QueryRecord(
            query_id="q001",
            timestamp="2026-04-07T10:00:00+00:00",
            source="Google_Patents_Browser",
            query_text="wireless charging",
            result_count=0,
            results=[],
            metadata={
                "status": "error",
                "error": "429 Too Many Requests",
                "search_context": {
                    "effective_backend": "browser",
                    "browser_backend_error": "429 Too Many Requests",
                },
            },
        )
        mgr.append_query_result(session.session_id, q)
        content = mgr.export_markdown(session.session_id).read_text()
        assert "Status:** error" in content
        assert "429 Too Many Requests" in content
        assert "Search Context" in content

    def test_export_patents_sorted_by_relevance(self, tmp_path):
        mgr = SessionManager(tmp_path)
        session = mgr.create_session("Topic")
        hits = [
            _make_hit("US10000001B2", relevance="low"),
            _make_hit("US10000002B2", relevance="high"),
            _make_hit("US10000003B2", relevance="medium"),
        ]
        mgr.append_query_result(session.session_id, _make_query(hits=hits))
        content = mgr.export_markdown(session.session_id).read_text()
        # high should appear before medium, medium before low
        idx_high = content.index("US10000002B2")
        idx_medium = content.index("US10000003B2")
        idx_low = content.index("US10000001B2")
        # In the "Patents Found" table, high should come first
        # We just check the relative ordering
        assert idx_high < idx_medium < idx_low

    def test_export_returns_path(self, tmp_path):
        mgr = SessionManager(tmp_path)
        session = mgr.create_session("Topic")
        result = mgr.export_markdown(session.session_id)
        assert isinstance(result, Path)

    def test_export_with_prior_art_cutoff(self, tmp_path):
        mgr = SessionManager(tmp_path)
        session = mgr.create_session("Topic", prior_art_cutoff="2018-06-01")
        content = mgr.export_markdown(session.session_id).read_text()
        assert "2018-06-01" in content

    def test_export_deduplicates_patents_in_summary(self, tmp_path):
        mgr = SessionManager(tmp_path)
        session = mgr.create_session("Topic")
        # Same patent in two queries
        q1 = _make_query("q001", hits=[_make_hit("US10000001B2")])
        q2 = _make_query("q002", hits=[_make_hit("US10000001B2")])
        mgr.append_query_result(session.session_id, q1)
        mgr.append_query_result(session.session_id, q2)
        content = mgr.export_markdown(session.session_id).read_text()
        # Patent count in summary should say 1
        assert "Unique patents found:** 1" in content


# ---------------------------------------------------------------------------
# T08 — Atomic save
# ---------------------------------------------------------------------------

class TestAtomicSave:
    def test_no_tmp_file_left_after_save(self, tmp_path):
        mgr = SessionManager(tmp_path)
        session = mgr.create_session("Topic")
        # No .tmp files should remain
        tmp_files = list(tmp_path.glob("*.tmp"))
        assert tmp_files == []

    def test_concurrent_saves_do_not_corrupt(self, tmp_path):
        """Multiple threads saving different sessions concurrently should all succeed."""
        mgr = SessionManager(tmp_path)
        sessions = [mgr.create_session(f"Topic {i}") for i in range(10)]
        errors: list[Exception] = []

        def append_and_annotate(session_id: str, i: int) -> None:
            try:
                for j in range(3):
                    q = _make_query(
                        f"q{j:03d}",
                        hits=[_make_hit(f"US{i * 100 + j:08d}B2")],
                    )
                    mgr.append_query_result(session_id, q)
            except Exception as e:
                errors.append(e)

        threads = [
            threading.Thread(target=append_and_annotate, args=(s.session_id, idx))
            for idx, s in enumerate(sessions)
        ]
        for t in threads:
            t.start()
        for t in threads:
            t.join()

        assert errors == [], f"Errors during concurrent saves: {errors}"

        # Verify each session is valid JSON and has 3 queries
        for s in sessions:
            loaded = mgr.load_session(s.session_id)
            assert len(loaded.queries) == 3

    def test_save_is_valid_json(self, tmp_path):
        mgr = SessionManager(tmp_path)
        session = mgr.create_session("Topic")
        path = tmp_path / f"{session.session_id}.json"
        raw = path.read_text(encoding="utf-8")
        parsed = json.loads(raw)
        assert parsed["session_id"] == session.session_id


# ---------------------------------------------------------------------------
# T09 — .index.json updated on every save
# ---------------------------------------------------------------------------

class TestIndexUpdated:
    def test_index_has_session_after_create(self, tmp_path):
        mgr = SessionManager(tmp_path)
        session = mgr.create_session("Topic")
        index = json.loads((tmp_path / ".index.json").read_text())
        ids = [s["session_id"] for s in index["sessions"]]
        assert session.session_id in ids

    def test_index_updated_after_append_query(self, tmp_path):
        mgr = SessionManager(tmp_path)
        session = mgr.create_session("Topic")
        mgr.append_query_result(session.session_id, _make_query())
        index = json.loads((tmp_path / ".index.json").read_text())
        entry = next(s for s in index["sessions"] if s["session_id"] == session.session_id)
        assert entry["query_count"] == 1
        assert entry["patent_count"] == 1

    def test_index_updated_after_add_note(self, tmp_path):
        mgr = SessionManager(tmp_path)
        session = mgr.create_session("Topic")
        old_index = json.loads((tmp_path / ".index.json").read_text())
        old_modified = next(
            s["modified_at"]
            for s in old_index["sessions"]
            if s["session_id"] == session.session_id
        )
        time.sleep(0.01)
        mgr.add_note(session.session_id, "A note")
        index = json.loads((tmp_path / ".index.json").read_text())
        entry = next(s for s in index["sessions"] if s["session_id"] == session.session_id)
        assert entry["modified_at"] >= old_modified

    def test_index_contains_multiple_sessions(self, tmp_path):
        mgr = SessionManager(tmp_path)
        s1 = mgr.create_session("Topic A")
        s2 = mgr.create_session("Topic B")
        index = json.loads((tmp_path / ".index.json").read_text())
        ids = {s["session_id"] for s in index["sessions"]}
        assert s1.session_id in ids
        assert s2.session_id in ids

    def test_index_patent_count_deduplicates(self, tmp_path):
        mgr = SessionManager(tmp_path)
        session = mgr.create_session("Topic")
        # Same patent in two queries
        q1 = _make_query("q001", hits=[_make_hit("US10000001B2"), _make_hit("US10000002B2")])
        q2 = _make_query("q002", hits=[_make_hit("US10000001B2")])
        mgr.append_query_result(session.session_id, q1)
        mgr.append_query_result(session.session_id, q2)
        index = json.loads((tmp_path / ".index.json").read_text())
        entry = next(s for s in index["sessions"] if s["session_id"] == session.session_id)
        assert entry["patent_count"] == 2

    def test_index_no_tmp_file_left(self, tmp_path):
        mgr = SessionManager(tmp_path)
        mgr.create_session("Topic")
        assert not (tmp_path / ".index.json.tmp").exists()


# ---------------------------------------------------------------------------
# T10 — PATENT_SESSIONS_DIR env var
# ---------------------------------------------------------------------------

class TestEnvVar:
    def test_env_var_used_when_set(self, tmp_path, monkeypatch):
        custom_dir = tmp_path / "custom-sessions"
        monkeypatch.setenv("PATENT_SESSIONS_DIR", str(custom_dir))
        mgr = SessionManager()
        session = mgr.create_session("Topic")
        assert (custom_dir / f"{session.session_id}.json").exists()

    def test_default_dir_used_when_env_not_set(self, tmp_path, monkeypatch):
        monkeypatch.delenv("PATENT_SESSIONS_DIR", raising=False)
        monkeypatch.chdir(tmp_path)
        mgr = SessionManager()
        session = mgr.create_session("Topic")
        assert (tmp_path / ".patent-sessions" / f"{session.session_id}.json").exists()

    def test_explicit_dir_overrides_env(self, tmp_path, monkeypatch):
        monkeypatch.setenv("PATENT_SESSIONS_DIR", str(tmp_path / "env-dir"))
        explicit_dir = tmp_path / "explicit-dir"
        mgr = SessionManager(sessions_dir=explicit_dir)
        session = mgr.create_session("Topic")
        assert (explicit_dir / f"{session.session_id}.json").exists()


# ---------------------------------------------------------------------------
# T11 — SessionSummary fields
# ---------------------------------------------------------------------------

class TestSessionSummary:
    def test_summary_fields(self, tmp_path):
        mgr = SessionManager(tmp_path)
        session = mgr.create_session("My Topic")
        summaries = mgr.list_sessions()
        assert len(summaries) == 1
        s = summaries[0]
        assert isinstance(s, SessionSummary)
        assert s.session_id == session.session_id
        assert s.topic == "My Topic"
        assert s.created_at == session.created_at
        assert s.query_count == 0
        assert s.patent_count == 0
