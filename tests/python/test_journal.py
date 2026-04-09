"""Tests for patent_mcp.journal — activity journal."""
from __future__ import annotations

import json
from pathlib import Path

from patent_mcp.journal import ActivityJournal


class TestActivityJournal:
    def test_append_fetch(self, tmp_path):
        path = tmp_path / "activity.jsonl"
        j = ActivityJournal(path)
        j.log_fetch(["US7654321"], {"total": 1, "success": 1})

        lines = path.read_text().strip().splitlines()
        assert len(lines) == 1
        record = json.loads(lines[0])
        assert record["action"] == "fetch"
        assert record["patent_ids"] == ["US7654321"]
        assert "ts" in record
        assert record["results"]["total"] == 1

    def test_append_list(self, tmp_path):
        path = tmp_path / "activity.jsonl"
        j = ActivityJournal(path)
        j.log_list(5)

        lines = path.read_text().strip().splitlines()
        assert len(lines) == 1
        record = json.loads(lines[0])
        assert record["action"] == "list"
        assert record["count"] == 5

    def test_append_metadata(self, tmp_path):
        path = tmp_path / "activity.jsonl"
        j = ActivityJournal(path)
        j.log_metadata(["US7654321", "EP1234567"], found=1, missing=1)

        lines = path.read_text().strip().splitlines()
        assert len(lines) == 1
        record = json.loads(lines[0])
        assert record["action"] == "metadata"
        assert record["found"] == 1
        assert record["missing"] == 1

    def test_multiple_records(self, tmp_path):
        path = tmp_path / "activity.jsonl"
        j = ActivityJournal(path)
        j.log_fetch(["US7654321"], {"total": 1})
        j.log_list(3)
        j.log_metadata(["US7654321"], found=1, missing=0)

        lines = path.read_text().strip().splitlines()
        assert len(lines) == 3

    def test_none_path_disables(self):
        j = ActivityJournal(None)
        # Should not raise
        j.log_fetch(["US7654321"], {"total": 1})
        j.log_list(0)
        j.log_metadata([], found=0, missing=0)

    def test_non_writable_path_does_not_crash(self):
        j = ActivityJournal(Path("/nonexistent/dir/activity.jsonl"))
        # Should not raise
        j.log_fetch(["US7654321"], {"total": 1})
