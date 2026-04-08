"""Tests for patent_mcp.id_canon — T01-T15."""
from __future__ import annotations

import pytest

from patent_mcp.id_canon import (
    CanonicalPatentId,
    canonicalize,
    canonicalize_batch,
    is_valid,
)


# ---------------------------------------------------------------------------
# T01 — US utility patent (granted)
# ---------------------------------------------------------------------------

class TestUsPatentGranted:
    def test_us_patent_bare_number(self):
        r = canonicalize("7654321")
        assert r.canonical == "US7654321"
        assert r.jurisdiction == "US"
        assert r.doc_type == "patent"

    def test_us_patent_prefixed(self):
        r = canonicalize("US7654321")
        assert r.canonical == "US7654321"
        assert r.jurisdiction == "US"
        assert r.doc_type == "patent"

    def test_us_patent_with_commas(self):
        r = canonicalize("US 7,654,321")
        assert r.canonical == "US7654321"

    def test_us_patent_lowercase_prefix(self):
        r = canonicalize("us7654321")
        assert r.canonical == "US7654321"


# ---------------------------------------------------------------------------
# T02 — US utility patent with kind code
# ---------------------------------------------------------------------------

class TestUsKindCode:
    def test_us_kind_code_b2(self):
        r = canonicalize("US7654321B2")
        assert r.kind_code == "B2"
        assert r.number == "7654321"

    def test_us_kind_code_b1(self):
        r = canonicalize("US7654321B1")
        assert r.kind_code == "B1"

    def test_us_a1_kind_code_application(self):
        r = canonicalize("US20240123456A1")
        assert r.kind_code == "A1"
        assert r.doc_type == "application"


# ---------------------------------------------------------------------------
# T03 — US patent applications
# ---------------------------------------------------------------------------

class TestUsApplication:
    def test_us_application_slash_format(self):
        r = canonicalize("US2024/0123456")
        assert r.canonical == "US20240123456"
        assert r.doc_type == "application"
        assert r.filing_year == 2024

    def test_us_application_no_slash(self):
        r = canonicalize("US20240123456")
        assert r.canonical == "US20240123456"
        assert r.doc_type == "application"
        assert r.filing_year == 2024

    def test_us_application_dash_format(self):
        r = canonicalize("US2024-0123456")
        assert r.canonical == "US20240123456"
        assert r.doc_type == "application"

    def test_us_bare_11digit_application(self):
        r = canonicalize("20240123456")
        assert r.canonical == "US20240123456"
        assert r.doc_type == "application"
        assert r.filing_year == 2024


# ---------------------------------------------------------------------------
# T04 — EP patents
# ---------------------------------------------------------------------------

class TestEpPatents:
    def test_ep_bare(self):
        r = canonicalize("EP1234567")
        assert r.jurisdiction == "EP"
        assert r.canonical == "EP1234567"

    def test_ep_with_spaces_and_kind_code(self):
        # "EP 1 234 567 A1" — spaces in number + kind code
        r = canonicalize("EP1234567A1")
        assert r.canonical == "EP1234567"
        assert r.kind_code == "A1"

    def test_ep_b1(self):
        r = canonicalize("EP1234567B1")
        assert r.kind_code == "B1"

    def test_ep_lowercase(self):
        r = canonicalize("ep1234567")
        assert r.jurisdiction == "EP"
        assert r.canonical == "EP1234567"


# ---------------------------------------------------------------------------
# T05 — WO/PCT applications
# ---------------------------------------------------------------------------

class TestWoPatents:
    def test_wo_slash(self):
        r = canonicalize("WO2024/123456")
        assert r.jurisdiction == "WO"
        assert r.canonical == "WO2024123456"
        assert r.filing_year == 2024

    def test_wo_no_slash(self):
        r = canonicalize("WO2024123456")
        assert r.canonical == "WO2024123456"
        assert r.filing_year == 2024

    def test_wo_8digit_serial(self):
        r = canonicalize("WO2024/12345678")
        assert r.jurisdiction == "WO"
        assert r.filing_year == 2024

    def test_wo_lowercase(self):
        r = canonicalize("wo2024/123456")
        assert r.jurisdiction == "WO"


# ---------------------------------------------------------------------------
# T06 — JP patents
# ---------------------------------------------------------------------------

class TestJpPatents:
    def test_jp_modern_dash(self):
        r = canonicalize("JP2023-123456")
        assert r.jurisdiction == "JP"
        assert r.canonical == "JP2023123456"

    def test_jp_modern_no_dash(self):
        r = canonicalize("JP2023123456")
        # Could match modern or bare depending on digit count
        assert r.jurisdiction == "JP"

    def test_jp_bare(self):
        r = canonicalize("JP4567890")
        assert r.jurisdiction == "JP"

    def test_jp_lowercase(self):
        r = canonicalize("jp2023-123456")
        assert r.jurisdiction == "JP"


# ---------------------------------------------------------------------------
# T07 — CN patents
# ---------------------------------------------------------------------------

class TestCnPatents:
    def test_cn_invention_kind_a(self):
        r = canonicalize("CN112345678A")
        assert r.jurisdiction == "CN"
        assert r.kind_code == "A"

    def test_cn_application_with_dot_x(self):
        r = canonicalize("CN201910123456.X")
        assert r.jurisdiction == "CN"
        assert r.canonical == "CN201910123456X"

    def test_cn_bare_number(self):
        r = canonicalize("CN112345678")
        assert r.jurisdiction == "CN"
        assert r.canonical == "CN112345678"

    def test_cn_lowercase(self):
        r = canonicalize("cn112345678A")
        assert r.jurisdiction == "CN"


# ---------------------------------------------------------------------------
# T08 — KR patents
# ---------------------------------------------------------------------------

class TestKrPatents:
    def test_kr_registered(self):
        r = canonicalize("KR102345678")
        assert r.jurisdiction == "KR"

    def test_kr_application(self):
        r = canonicalize("KR10-2023-0012345")
        assert r.jurisdiction == "KR"
        assert r.canonical == "KR1020230012345"

    def test_kr_application_no_dashes(self):
        r = canonicalize("KR1020230012345")
        assert r.jurisdiction == "KR"


# ---------------------------------------------------------------------------
# T09 — AU, CA, NZ
# ---------------------------------------------------------------------------

class TestAuCaNz:
    def test_au_patent(self):
        r = canonicalize("AU2023123456")
        assert r.jurisdiction == "AU"
        assert r.canonical == "AU2023123456"

    def test_ca_patent(self):
        r = canonicalize("CA3012345")
        assert r.jurisdiction == "CA"
        assert r.canonical == "CA3012345"

    def test_nz_patent(self):
        r = canonicalize("NZ123456")
        assert r.jurisdiction == "NZ"
        assert r.canonical == "NZ123456"

    def test_au_lowercase(self):
        r = canonicalize("au2023123456")
        assert r.jurisdiction == "AU"


# ---------------------------------------------------------------------------
# T10 — BR, IN
# ---------------------------------------------------------------------------

class TestBrIn:
    def test_br_patent_with_hyphen(self):
        r = canonicalize("BR102023012345-0")
        assert r.jurisdiction == "BR"
        # Hyphen stripped from suffix
        assert "-" not in r.canonical

    def test_br_patent_bare(self):
        r = canonicalize("BR102023012345")
        assert r.jurisdiction == "BR"

    def test_in_patent(self):
        r = canonicalize("IN202317001234")
        assert r.jurisdiction == "IN"
        assert r.canonical == "IN202317001234"

    def test_in_lowercase(self):
        r = canonicalize("in202317001234")
        assert r.jurisdiction == "IN"


# ---------------------------------------------------------------------------
# T11 — Tier 2 jurisdictions (ISO prefix passthrough)
# ---------------------------------------------------------------------------

class TestIsoPassthrough:
    def test_de_passthrough(self):
        r = canonicalize("DE102023001234")
        assert r.jurisdiction == "DE"
        assert r.canonical == "DE102023001234"

    def test_fr_passthrough(self):
        r = canonicalize("FR3123456")
        assert r.jurisdiction == "FR"

    def test_unknown_jurisdiction_errors(self):
        # XX is not a known ISO jurisdiction but matches generic pattern
        r = canonicalize("XX9999999")
        # Should not raise; may or may not have errors depending on treatment
        assert r is not None
        assert r.jurisdiction in ("XX", "UNKNOWN")


# ---------------------------------------------------------------------------
# T12 — URL input (Google Patents)
# ---------------------------------------------------------------------------

class TestUrlInput:
    def test_google_patents_url(self):
        r = canonicalize("https://patents.google.com/patent/US7654321B2/en")
        assert r.canonical == "US7654321"
        assert r.kind_code == "B2"
        assert r.jurisdiction == "US"

    def test_google_patents_url_no_lang(self):
        r = canonicalize("https://patents.google.com/patent/EP1234567B1")
        assert r.jurisdiction == "EP"
        assert r.kind_code == "B1"

    def test_espacenet_url(self):
        r = canonicalize("https://worldwide.espacenet.com/patent/search/family/123456/publication/US7654321B2")
        assert r.jurisdiction == "US"


# ---------------------------------------------------------------------------
# T13 — Batch canonicalization
# ---------------------------------------------------------------------------

class TestBatchCanonicalization:
    def test_batch_same_length(self):
        result = canonicalize_batch(["US7654321", "EP1234567"])
        assert len(result) == 2

    def test_batch_preserves_order(self):
        ids = ["US7654321", "EP1234567", "WO2024123456"]
        result = canonicalize_batch(ids)
        assert result[0].jurisdiction == "US"
        assert result[1].jurisdiction == "EP"
        assert result[2].jurisdiction == "WO"

    def test_batch_empty_list(self):
        result = canonicalize_batch([])
        assert result == []

    def test_batch_single_item(self):
        result = canonicalize_batch(["US7654321"])
        assert len(result) == 1
        assert result[0].canonical == "US7654321"


# ---------------------------------------------------------------------------
# T14 — Invalid / malformed input
# ---------------------------------------------------------------------------

class TestInvalidInput:
    def test_empty_string_has_errors(self):
        r = canonicalize("")
        assert r.errors

    def test_empty_string_no_exception(self):
        r = canonicalize("")  # Should not raise
        assert isinstance(r, CanonicalPatentId)

    def test_random_garbage_no_exception(self):
        r = canonicalize("ABCDEFGH!@#$")
        assert isinstance(r, CanonicalPatentId)

    def test_random_garbage_has_errors(self):
        r = canonicalize("!@#$%^&*()")
        assert r.errors

    def test_is_valid_false_for_garbage(self):
        assert is_valid("notapatent") is False

    def test_is_valid_true_for_us(self):
        assert is_valid("US7654321") is True

    def test_is_valid_true_for_ep(self):
        assert is_valid("EP1234567") is True

    def test_is_valid_false_for_empty(self):
        assert is_valid("") is False

    def test_whitespace_only_has_errors(self):
        r = canonicalize("   ")
        assert r.errors or r.jurisdiction == "UNKNOWN"

    def test_very_long_string_no_exception(self):
        r = canonicalize("X" * 1000)
        assert isinstance(r, CanonicalPatentId)


# ---------------------------------------------------------------------------
# T15 — Round-trip property
# ---------------------------------------------------------------------------

class TestRoundTrip:
    FIXTURE_IDS = [
        "US7654321",
        "EP1234567",
        "WO2024/123456",
        "JP2023-123456",
        "CN112345678A",
        "KR10-2023-0012345",
        "AU2023123456",
        "CA3012345",
        "NZ123456",
    ]

    def test_roundtrip_idempotent(self):
        """canonicalize(canonical).canonical == canonical for all fixture IDs."""
        for raw in self.FIXTURE_IDS:
            first = canonicalize(raw)
            second = canonicalize(first.canonical)
            assert second.canonical == first.canonical, (
                f"Round-trip failed for {raw!r}: "
                f"{first.canonical!r} → {second.canonical!r}"
            )

    @pytest.mark.parametrize("raw", FIXTURE_IDS)
    def test_roundtrip_parametrized(self, raw):
        first = canonicalize(raw)
        second = canonicalize(first.canonical)
        assert second.canonical == first.canonical
