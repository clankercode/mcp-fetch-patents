"""Tests for heuristic search ranking."""
from __future__ import annotations

import pytest
from patent_mcp.search.session_manager import PatentHit
from patent_mcp.search.planner import NaturalLanguagePlanner, SearchIntent
from patent_mcp.search.ranking import SearchRanker, ScoredHit, _text_coverage, _date_score, _metadata_richness


@pytest.fixture
def ranker():
    return SearchRanker()


@pytest.fixture
def planner():
    return NaturalLanguagePlanner()


def _make_hit(
    patent_id="US1234567",
    title=None,
    abstract=None,
    assignee=None,
    inventors=None,
    date=None,
    source="test",
) -> PatentHit:
    return PatentHit(
        patent_id=patent_id,
        title=title,
        abstract=abstract,
        assignee=assignee,
        inventors=inventors or [],
        date=date,
        source=source,
    )


class TestTextCoverage:
    def test_full_coverage(self):
        assert _text_coverage("wireless charging battery", ["wireless", "charging", "battery"]) == 1.0

    def test_partial_coverage(self):
        assert _text_coverage("wireless power system", ["wireless", "charging"]) == 0.5

    def test_no_coverage(self):
        assert _text_coverage("totally unrelated text", ["wireless", "charging"]) == 0.0

    def test_empty_text(self):
        assert _text_coverage(None, ["wireless"]) == 0.0
        assert _text_coverage("", ["wireless"]) == 0.0

    def test_empty_concepts(self):
        assert _text_coverage("some text", []) == 0.0

    def test_case_insensitive(self):
        assert _text_coverage("WIRELESS Charging", ["wireless", "charging"]) == 1.0


class TestDateScore:
    def test_before_cutoff(self):
        assert _date_score("2019-06-15", "2020-01-01") == 1.0

    def test_after_cutoff(self):
        assert _date_score("2021-06-15", "2020-01-01") == 0.0

    def test_no_cutoff(self):
        assert _date_score("2019-06-15", None) == 0.5

    def test_no_date(self):
        assert _date_score(None, "2020-01-01") == 0.3

    def test_exact_cutoff(self):
        assert _date_score("20200101", "2020-01-01") == 1.0  # equal = before


class TestMetadataRichness:
    def test_fully_populated(self):
        hit = _make_hit(title="T", abstract="A", assignee="X", inventors=["I"], date="2020")
        assert _metadata_richness(hit) == 5.0

    def test_empty(self):
        hit = _make_hit()
        assert _metadata_richness(hit) == 0.0

    def test_partial(self):
        hit = _make_hit(title="T", date="2020")
        assert _metadata_richness(hit) == 2.0


class TestSearchRanker:
    def test_multi_query_bonus(self, ranker, planner):
        """Patent found by multiple queries should score higher."""
        intent = planner.plan("wireless charging")
        hit = _make_hit("US111", title="Wireless charging system")

        hits_by_query = {
            "wireless charging": [hit],
            '"wireless charging" OR "inductive coupling"': [hit],
            "wireless AND charging": [hit],
        }
        scored = ranker.rank(hits_by_query, intent)
        assert len(scored) == 1
        assert scored[0].query_matches == 3
        assert scored[0].score_breakdown["multi_query_bonus"] > 0

    def test_title_match_beats_no_match(self, ranker, planner):
        intent = planner.plan("wireless charging")
        good = _make_hit("US111", title="Wireless charging apparatus")
        bad = _make_hit("US222", title="Unrelated fish tank")

        hits_by_query = {
            "wireless charging": [good, bad],
        }
        scored = ranker.rank(hits_by_query, intent)
        assert scored[0].hit.patent_id == "US111"

    def test_deduplication(self, ranker, planner):
        intent = planner.plan("battery")
        hit1 = _make_hit("US111", title="Battery system")
        hit2 = _make_hit("US111", title="Battery system v2")  # same ID

        hits_by_query = {
            "battery": [hit1],
            '"energy storage"': [hit2],
        }
        scored = ranker.rank(hits_by_query, intent)
        assert len(scored) == 1
        assert scored[0].query_matches == 2

    def test_prefers_richer_metadata(self, ranker, planner):
        """When same patent appears in multiple queries, keep the richer version."""
        intent = planner.plan("sensor")
        sparse = _make_hit("US111")
        rich = _make_hit("US111", title="Sensor device", abstract="A sensor", assignee="Corp")

        hits_by_query = {
            "sensor": [sparse],
            '"detector"': [rich],
        }
        scored = ranker.rank(hits_by_query, intent)
        assert scored[0].hit.title == "Sensor device"

    def test_date_cutoff_affects_score(self, ranker, planner):
        intent = planner.plan("battery", date_cutoff="2020-01-01")
        old = _make_hit("US111", title="Battery", date="2018-06-01")
        new = _make_hit("US222", title="Battery", date="2022-06-01")

        hits_by_query = {"battery": [old, new]}
        scored = ranker.rank(hits_by_query, intent)
        # Old patent (before cutoff) should score higher
        assert scored[0].hit.patent_id == "US111"

    def test_empty_input(self, ranker, planner):
        intent = planner.plan("something")
        scored = ranker.rank({}, intent)
        assert scored == []
