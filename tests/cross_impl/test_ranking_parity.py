"""Cross-impl parity: search ranking.

Feeds identical hits and intent to both Python and Rust rankers,
verifies they produce the same scores and ordering.
"""
from __future__ import annotations

import json
import subprocess

import pytest

from patent_mcp.search.session_manager import PatentHit
from patent_mcp.search.planner import NaturalLanguagePlanner, SearchIntent
from patent_mcp.search.ranking import SearchRanker


def _make_hit(
    patent_id: str,
    title: str | None = None,
    abstract_: str | None = None,
    assignee: str | None = None,
    inventors: list[str] | None = None,
    date: str | None = None,
    source: str = "test",
) -> PatentHit:
    return PatentHit(
        patent_id=patent_id,
        title=title,
        abstract=abstract_,
        assignee=assignee,
        inventors=inventors or [],
        date=date,
        source=source,
    )


def _hit_to_dict(hit: PatentHit) -> dict:
    """Convert to dict matching Rust's PatentHit JSON schema."""
    return {
        "patent_id": hit.patent_id,
        "title": hit.title,
        "date": hit.date,
        "assignee": hit.assignee,
        "inventors": hit.inventors,
        "abstract_text": hit.abstract,  # Rust uses abstract_text
        "source": hit.source,
        "relevance": hit.relevance,
        "note": hit.note,
        "prior_art": hit.prior_art,
        "url": hit.url,
    }


def _python_rank(
    hits_by_query: dict[str, list[PatentHit]],
    intent: SearchIntent,
) -> list[dict]:
    ranker = SearchRanker()
    scored = ranker.rank(hits_by_query, intent)
    return [
        {
            "patent_id": s.hit.patent_id,
            "score": round(s.score, 4),
            "query_matches": s.query_matches,
        }
        for s in scored
    ]


def _rust_rank(
    rust_bin: str,
    hits_by_query: dict[str, list[PatentHit]],
    concepts: list[str],
    date_cutoff: str | None,
) -> list[dict]:
    # Build JSON input for Rust rank subcommand
    hbq_json = {}
    for query, hits in hits_by_query.items():
        hbq_json[query] = [_hit_to_dict(h) for h in hits]

    input_data = {
        "hits_by_query": hbq_json,
        "concepts": concepts,
        "date_cutoff": date_cutoff,
    }

    proc = subprocess.run(
        [rust_bin, "rank", "--input", "-"],
        input=json.dumps(input_data),
        capture_output=True,
        text=True,
        timeout=10,
    )
    assert proc.returncode == 0, f"Rust rank failed: {proc.stderr}"
    data = json.loads(proc.stdout)
    return [
        {
            "patent_id": s["hit"]["patent_id"],
            "score": round(s["score"], 4),
            "query_matches": s["query_matches"],
        }
        for s in data
    ]


class TestRankingParity:
    def test_single_query_ordering(self, rust_binary: str):
        """Same hits from one query should produce same order."""
        planner = NaturalLanguagePlanner()
        intent = planner.plan("wireless charging")

        good = _make_hit("US111", title="Wireless charging apparatus", date="2019-01-01")
        bad = _make_hit("US222", title="Unrelated fish tank", date="2019-06-01")

        hits_by_query = {"wireless charging": [good, bad]}

        py = _python_rank(hits_by_query, intent)
        rs = _rust_rank(rust_binary, hits_by_query, intent.concepts, intent.date_cutoff)

        assert [r["patent_id"] for r in py] == [r["patent_id"] for r in rs]

    def test_multi_query_bonus(self, rust_binary: str):
        """Patent found by 3 queries should have higher query_matches in both."""
        planner = NaturalLanguagePlanner()
        intent = planner.plan("wireless charging")

        hit = _make_hit("US111", title="Wireless charging system")

        hits_by_query = {
            "wireless charging": [hit],
            '"wireless charging" OR "inductive coupling"': [hit],
            "wireless AND charging": [hit],
        }

        py = _python_rank(hits_by_query, intent)
        rs = _rust_rank(rust_binary, hits_by_query, intent.concepts, intent.date_cutoff)

        assert py[0]["query_matches"] == 3
        assert rs[0]["query_matches"] == 3

    def test_date_cutoff_affects_ordering(self, rust_binary: str):
        """With a date cutoff, older patents should score higher in both."""
        planner = NaturalLanguagePlanner()
        intent = planner.plan("battery", date_cutoff="2020-01-01")

        old = _make_hit("US111", title="Battery system", date="2018-06-01")
        new = _make_hit("US222", title="Battery system", date="2022-06-01")

        hits_by_query = {"battery": [old, new]}

        py = _python_rank(hits_by_query, intent)
        rs = _rust_rank(rust_binary, hits_by_query, intent.concepts, intent.date_cutoff)

        assert py[0]["patent_id"] == "US111"
        assert rs[0]["patent_id"] == "US111"

    def test_deduplication(self, rust_binary: str):
        """Same patent ID in multiple queries should be deduped in both."""
        planner = NaturalLanguagePlanner()
        intent = planner.plan("sensor")

        hit1 = _make_hit("US111", title="Sensor device")
        hit2 = _make_hit("US111", title="Sensor device copy")

        hits_by_query = {
            "sensor": [hit1],
            '"detector"': [hit2],
        }

        py = _python_rank(hits_by_query, intent)
        rs = _rust_rank(rust_binary, hits_by_query, intent.concepts, intent.date_cutoff)

        assert len(py) == 1
        assert len(rs) == 1

    def test_score_agreement(self, rust_binary: str):
        """Scores should be identical (or very close) between implementations."""
        planner = NaturalLanguagePlanner()
        intent = planner.plan("wireless charging battery")

        hits_by_query = {
            "wireless charging battery": [
                _make_hit("US111", title="Wireless charging of battery", date="2019-01-01",
                          abstract_="A method for wirelessly charging batteries"),
                _make_hit("US222", title="Solar panel installation", date="2019-06-01"),
            ],
        }

        py = _python_rank(hits_by_query, intent)
        rs = _rust_rank(rust_binary, hits_by_query, intent.concepts, intent.date_cutoff)

        # Same ordering
        assert [r["patent_id"] for r in py] == [r["patent_id"] for r in rs]

        # Scores within tolerance
        for p, r in zip(py, rs):
            assert abs(p["score"] - r["score"]) < 0.01, (
                f"Score mismatch for {p['patent_id']}: Python={p['score']}, Rust={r['score']}"
            )
