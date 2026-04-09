"""Heuristic reranking for multi-query patent search results.

Scores hits by: query-term coverage in title/snippet, date satisfaction,
multi-query appearance bonus, and source confidence.
"""
from __future__ import annotations

import re
from dataclasses import dataclass, field
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from patent_mcp.search.planner import SearchIntent
    from patent_mcp.search.session_manager import PatentHit


@dataclass
class ScoredHit:
    """A PatentHit with a computed relevance score."""
    hit: "PatentHit"
    score: float
    score_breakdown: dict[str, float]
    query_matches: int  # how many query variants found this hit


class SearchRanker:
    """Rank patent search results using heuristic scoring."""

    def rank(
        self,
        hits_by_query: dict[str, list["PatentHit"]],
        search_intent: "SearchIntent",
    ) -> list[ScoredHit]:
        """Merge hits from multiple queries, deduplicate, score, and sort.

        ``hits_by_query`` maps query text → list of hits found by that query.
        The same patent may appear in multiple query results — that's a signal.
        """
        # Count how many queries found each patent
        query_counts: dict[str, int] = {}
        merged: dict[str, "PatentHit"] = {}
        for _query, hits in hits_by_query.items():
            for hit in hits:
                pid = hit.patent_id
                query_counts[pid] = query_counts.get(pid, 0) + 1
                # Keep the hit with the most complete metadata
                if pid not in merged or _metadata_richness(hit) > _metadata_richness(merged[pid]):
                    merged[pid] = hit

        # Score each unique hit
        scored: list[ScoredHit] = []
        concepts = search_intent.concepts
        date_cutoff = search_intent.date_cutoff

        for pid, hit in merged.items():
            breakdown: dict[str, float] = {}

            # Title coverage: what fraction of concepts appear in the title
            breakdown["title_coverage"] = _text_coverage(
                hit.title, concepts,
            ) * 3.0  # weight: title matches are very valuable

            # Snippet/abstract coverage
            breakdown["snippet_coverage"] = _text_coverage(
                hit.abstract, concepts,
            ) * 2.0

            # Multi-query bonus: found by N variants → high signal
            n_queries = query_counts.get(pid, 1)
            breakdown["multi_query_bonus"] = min(n_queries - 1, 4) * 1.5

            # Date satisfaction: if we have a cutoff, prefer patents before it
            breakdown["date_satisfaction"] = _date_score(hit.date, date_cutoff)

            # Metadata completeness: prefer hits with title, abstract, assignee
            breakdown["completeness"] = _metadata_richness(hit) * 0.3

            total = sum(breakdown.values())
            scored.append(ScoredHit(
                hit=hit,
                score=total,
                score_breakdown=breakdown,
                query_matches=n_queries,
            ))

        scored.sort(key=lambda s: s.score, reverse=True)
        return scored


# ---------------------------------------------------------------------------
# Scoring helpers
# ---------------------------------------------------------------------------

def _text_coverage(text: str | None, concepts: list[str]) -> float:
    """Fraction of concepts that appear in text (0.0 – 1.0).

    Uses word-boundary matching to avoid false positives like "led" matching
    inside "assembled".
    """
    if not text or not concepts:
        return 0.0
    lower = text.lower()
    found = 0
    for c in concepts:
        # Multi-word concepts: use substring match (they're specific enough)
        # Single-word concepts: use word-boundary match to avoid false positives
        cl = c.lower()
        if " " in cl:
            if cl in lower:
                found += 1
        else:
            if re.search(r"(?<!\w)" + re.escape(cl) + r"(?!\w)", lower):
                found += 1
    return found / len(concepts)


def _date_score(date_str: str | None, cutoff: str | None) -> float:
    """Score based on date relative to cutoff.

    - No cutoff → 0.5 (neutral)
    - Before cutoff → 1.0
    - After cutoff → 0.0
    - No date on hit → 0.3 (slight penalty for unknown)
    """
    if not cutoff:
        return 0.5
    if not date_str:
        return 0.3

    # Normalise to comparable strings (YYYYMMDD)
    hit_date = re.sub(r"[^0-9]", "", date_str)[:8]
    cut_date = re.sub(r"[^0-9]", "", cutoff)[:8]

    if not hit_date or len(hit_date) < 4:
        return 0.3

    try:
        if hit_date <= cut_date:
            return 1.0
        else:
            return 0.0
    except Exception:
        return 0.3


def _metadata_richness(hit: "PatentHit") -> float:
    """Score 0–5 based on how many metadata fields are populated."""
    score = 0.0
    if hit.title:
        score += 1.0
    if hit.abstract:
        score += 1.0
    if hit.assignee:
        score += 1.0
    if hit.inventors:
        score += 1.0
    if hit.date:
        score += 1.0
    return score
