"""Cross-impl parity: NL search planner.

For each test description, runs both the Python and Rust implementations
and verifies they produce identical concepts, synonyms, and query variant types.
"""
from __future__ import annotations

import json
import subprocess

import pytest

from patent_mcp.search.planner import NaturalLanguagePlanner

TEST_DESCRIPTIONS = [
    "wireless charging through metal barriers",
    "machine learning for computer vision tasks",
    "robotic fruit picking using soft grippers",
    "novel heat exchanger design for turbine cooling",
    "method for autonomous vehicle navigation using lidar",
    "battery management system for electric vehicles",
    "3d printing of composite materials",
    "drug delivery using nanoparticle carriers",
    "blockchain based encryption for database security",
    "",  # empty description
]

# Fields that must agree between implementations
COMPARE_FIELDS = ["concepts", "query_variant_types"]


def _python_result(description: str, date_cutoff: str | None = None) -> dict:
    planner = NaturalLanguagePlanner()
    intent = planner.plan(description, date_cutoff=date_cutoff)
    return {
        "concepts": intent.concepts,
        "synonyms_keys": sorted(intent.synonyms.keys()),
        "query_variant_types": [v.variant_type for v in intent.query_variants],
        "query_variants_count": len(intent.query_variants),
        "raw_description": intent.raw_description,
    }


def _rust_result(rust_bin: str, description: str, date_cutoff: str | None = None) -> dict:
    cmd = [rust_bin, "plan", description]
    if date_cutoff:
        cmd.extend(["--date-cutoff", date_cutoff])
    proc = subprocess.run(
        cmd,
        capture_output=True,
        text=True,
        timeout=10,
    )
    assert proc.returncode == 0, (
        f"Rust plan failed for {description!r}: {proc.stderr}"
    )
    data = json.loads(proc.stdout)
    return {
        "concepts": data.get("concepts", []),
        "synonyms_keys": sorted(data.get("synonyms", {}).keys()),
        "query_variant_types": [v["variant_type"] for v in data.get("query_variants", [])],
        "query_variants_count": len(data.get("query_variants", [])),
        "raw_description": data.get("raw_description", ""),
    }


@pytest.mark.parametrize("description", TEST_DESCRIPTIONS)
def test_planner_concepts_parity(description: str, rust_binary: str) -> None:
    py = _python_result(description)
    rs = _rust_result(rust_binary, description)

    assert py["concepts"] == rs["concepts"], (
        f"Concepts mismatch for {description!r}:\n"
        f"  Python: {py['concepts']}\n"
        f"  Rust:   {rs['concepts']}"
    )


@pytest.mark.parametrize("description", TEST_DESCRIPTIONS)
def test_planner_synonyms_parity(description: str, rust_binary: str) -> None:
    py = _python_result(description)
    rs = _rust_result(rust_binary, description)

    assert py["synonyms_keys"] == rs["synonyms_keys"], (
        f"Synonym keys mismatch for {description!r}:\n"
        f"  Python: {py['synonyms_keys']}\n"
        f"  Rust:   {rs['synonyms_keys']}"
    )


@pytest.mark.parametrize("description", TEST_DESCRIPTIONS)
def test_planner_variant_types_parity(description: str, rust_binary: str) -> None:
    py = _python_result(description)
    rs = _rust_result(rust_binary, description)

    assert py["query_variant_types"] == rs["query_variant_types"], (
        f"Variant types mismatch for {description!r}:\n"
        f"  Python: {py['query_variant_types']}\n"
        f"  Rust:   {rs['query_variant_types']}"
    )


def test_planner_with_date_cutoff(rust_binary: str) -> None:
    desc = "wireless charging"
    cutoff = "2020-01-01"
    py = _python_result(desc, date_cutoff=cutoff)
    rs = _rust_result(rust_binary, desc, date_cutoff=cutoff)

    assert py["concepts"] == rs["concepts"]
    assert py["query_variant_types"] == rs["query_variant_types"]
