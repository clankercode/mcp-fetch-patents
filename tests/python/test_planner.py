"""Tests for the NL search planner."""
from __future__ import annotations

import pytest
from patent_mcp.search.planner import NaturalLanguagePlanner, SearchIntent, QueryVariant


@pytest.fixture
def planner():
    return NaturalLanguagePlanner()


class TestConceptExtraction:
    def test_extracts_multi_word_phrases(self, planner):
        intent = planner.plan("wireless charging through metal barriers")
        assert "wireless charging" in intent.concepts

    def test_extracts_single_keywords(self, planner):
        intent = planner.plan("novel heat exchanger design for turbine cooling")
        assert "heat exchanger" in intent.concepts
        assert "turbine" in intent.concepts
        assert "cooling" in intent.concepts

    def test_removes_stop_words(self, planner):
        intent = planner.plan("a method for the wireless charging of batteries")
        # Stop words should not appear as concepts
        for stop in ["a", "for", "the", "of"]:
            assert stop not in intent.concepts

    def test_deduplicates_concepts(self, planner):
        intent = planner.plan("sensor sensor sensor detection")
        assert intent.concepts.count("sensor") == 1

    def test_multi_word_phrases_take_priority(self, planner):
        """Multi-word phrases from synonym table should be found even if words overlap."""
        intent = planner.plan("machine learning for computer vision tasks")
        assert "machine learning" in intent.concepts
        assert "computer vision" in intent.concepts

    def test_empty_description(self, planner):
        intent = planner.plan("")
        assert intent.concepts == []
        assert intent.query_variants == []


class TestSynonymExpansion:
    def test_known_terms_have_synonyms(self, planner):
        intent = planner.plan("wireless charging battery")
        assert "wireless charging" in intent.synonyms
        assert "battery" in intent.synonyms
        assert len(intent.synonyms["wireless charging"]) > 0
        assert len(intent.synonyms["battery"]) > 0

    def test_unknown_terms_have_no_synonyms(self, planner):
        intent = planner.plan("xyzzy frobnicator")
        assert "xyzzy" not in intent.synonyms
        assert "frobnicator" not in intent.synonyms

    def test_synonym_content(self, planner):
        intent = planner.plan("wireless charging")
        alts = intent.synonyms.get("wireless charging", [])
        # Should contain at least one common alternative
        assert any("inductive" in a or "contactless" in a for a in alts)


class TestQueryVariantGeneration:
    def test_generates_broad_variant(self, planner):
        intent = planner.plan("robotic fruit picking")
        types = [v.variant_type for v in intent.query_variants]
        assert "broad" in types

    def test_broad_variant_is_raw_description(self, planner):
        desc = "wireless power transfer through conductive barriers"
        intent = planner.plan(desc)
        broad = [v for v in intent.query_variants if v.variant_type == "broad"]
        assert broad[0].query == desc

    def test_generates_synonym_expanded(self, planner):
        intent = planner.plan("wireless charging through metal")
        types = [v.variant_type for v in intent.query_variants]
        assert "synonym_expanded" in types
        # Synonym variant should contain OR groups
        syn = [v for v in intent.query_variants if v.variant_type == "synonym_expanded"][0]
        assert " OR " in syn.query

    def test_generates_title_focused(self, planner):
        intent = planner.plan("solar cell efficiency improvement")
        types = [v.variant_type for v in intent.query_variants]
        assert "title_focused" in types

    def test_generates_concepts_and(self, planner):
        intent = planner.plan("robotic arm with sensor feedback control")
        types = [v.variant_type for v in intent.query_variants]
        assert "concepts_and" in types
        and_v = [v for v in intent.query_variants if v.variant_type == "concepts_and"][0]
        assert " AND " in and_v.query

    def test_generates_quoted_phrase_for_multi_word(self, planner):
        intent = planner.plan("wireless charging through metal wall")
        types = [v.variant_type for v in intent.query_variants]
        assert "quoted_phrase" in types
        qp = [v for v in intent.query_variants if v.variant_type == "quoted_phrase"][0]
        assert '"wireless charging"' in qp.query

    def test_variant_count_reasonable(self, planner):
        intent = planner.plan("method for autonomous vehicle navigation using lidar")
        assert 3 <= len(intent.query_variants) <= 6

    def test_no_synonym_variant_when_no_synonyms(self, planner):
        intent = planner.plan("xyzzy frobnicator gribble")
        types = [v.variant_type for v in intent.query_variants]
        assert "synonym_expanded" not in types


class TestSearchIntentFields:
    def test_date_cutoff_propagated(self, planner):
        intent = planner.plan("battery", date_cutoff="2020-01-01")
        assert intent.date_cutoff == "2020-01-01"

    def test_jurisdictions_propagated(self, planner):
        intent = planner.plan("battery", jurisdictions=["US", "EP"])
        assert intent.jurisdictions == ["US", "EP"]

    def test_rationale_non_empty(self, planner):
        intent = planner.plan("wireless charging")
        assert intent.rationale
        assert "concepts" in intent.rationale.lower() or "extracted" in intent.rationale.lower()

    def test_raw_description_preserved(self, planner):
        desc = "A novel method for underwater wireless communication"
        intent = planner.plan(desc)
        assert intent.raw_description == desc
