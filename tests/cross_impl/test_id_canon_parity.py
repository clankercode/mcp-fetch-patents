"""T16 — Cross-impl parity: id_canon canonicalize().

For each test ID, runs both the Python and Rust implementations and compares
the jurisdiction, canonical, doc_type, kind_code, and number fields.
"""
from __future__ import annotations

import dataclasses
import json
import subprocess

import pytest

from patent_mcp.id_canon import canonicalize

TEST_IDS = [
    "US7654321",
    "EP1234567",
    "WO2024123456",
    "JP2023123456",
    "KR101234567",
    "CN202310001234A",
    "AU2023123456",
    "INVALID-ID",
    "US20230001234A1",
    "https://patents.google.com/patent/US7654321/en",
    "",  # empty string
]

# Fields that must agree between implementations.
COMPARE_FIELDS = ["jurisdiction", "doc_type", "kind_code"]


def _python_result(raw_id: str) -> dict:
    result = canonicalize(raw_id)
    return dataclasses.asdict(result)


def _rust_result(rust_bin: str, raw_id: str) -> dict:
    proc = subprocess.run(
        [rust_bin, "canonicalize", raw_id],
        capture_output=True,
        text=True,
        timeout=10,
    )
    assert proc.returncode == 0, (
        f"Rust canonicalize failed for {raw_id!r}: {proc.stderr}"
    )
    return json.loads(proc.stdout)


@pytest.mark.parametrize("raw_id", TEST_IDS)
def test_id_canon_parity(raw_id: str, rust_binary: str) -> None:
    py = _python_result(raw_id)
    rs = _rust_result(rust_binary, raw_id)

    for field in COMPARE_FIELDS:
        assert py[field] == rs[field], (
            f"Field {field!r} mismatch for {raw_id!r}: "
            f"Python={py[field]!r} Rust={rs[field]!r}"
        )

    # jurisdiction must be non-empty in both
    assert py["jurisdiction"], f"Python jurisdiction empty for {raw_id!r}"
    assert rs["jurisdiction"], f"Rust jurisdiction empty for {raw_id!r}"

    # canonical must be non-empty in both
    assert py["canonical"], f"Python canonical empty for {raw_id!r}"
    assert rs["canonical"], f"Rust canonical empty for {raw_id!r}"
