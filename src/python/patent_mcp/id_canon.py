"""Patent ID canonicalization: parse any patent ID format into a canonical form."""
from __future__ import annotations

import re
from dataclasses import dataclass, field
from typing import NamedTuple


# ---------------------------------------------------------------------------
# Output type
# ---------------------------------------------------------------------------

@dataclass
class CanonicalPatentId:
    raw: str
    canonical: str           # e.g. "US7654321"
    jurisdiction: str        # e.g. "US", "EP", "WO"
    number: str              # numeric/alphanumeric serial (no jurisdiction prefix)
    kind_code: str | None    # e.g. "B1", "A2", "B2"; None if not present
    doc_type: str            # "patent" | "application" | "unknown"
    filing_year: int | None  # if determinable from ID
    errors: list[str] = field(default_factory=list)


# ---------------------------------------------------------------------------
# Regex patterns (compiled once)
# ---------------------------------------------------------------------------

# Kind codes: letter optionally followed by digit, e.g. A, B, B1, B2, A1, E
_KIND_RE = re.compile(r"([A-Z][0-9]?)$")

# --- US ---
# Granted patents: US + 7-8 digits (may have commas, spaces)
_US_PATENT_RE = re.compile(
    r"^US\s*([0-9]{1,3}(?:,[0-9]{3})*|[0-9]{4,8})$", re.IGNORECASE
)
# Applications: US + 4-digit year + 7 digits (with optional slash) + optional kind code
_US_APP_RE = re.compile(
    r"^US\s*(20[0-9]{2})[/\-]?([0-9]{6,7})([A-Z][0-9]?)?$", re.IGNORECASE
)
# Bare 7-digit number (assumed US granted)
_US_BARE_RE = re.compile(r"^([0-9]{7})$")
# Bare 8-digit number
_US_BARE8_RE = re.compile(r"^([0-9]{8})$")
# US applications without prefix: 10-11 digit number starting with year
_US_APP_BARE_RE = re.compile(r"^(20[0-9]{2})([0-9]{7})$")

# --- EP ---
_EP_RE = re.compile(r"^EP\s*([0-9\s]{6,10})$", re.IGNORECASE)

# --- WO/PCT ---
_WO_RE = re.compile(r"^WO\s*((?:19|20)[0-9]{2})[/\-]?([0-9]{4,8})$", re.IGNORECASE)

# --- JP ---
_JP_MODERN_RE = re.compile(r"^JP\s*(20[0-9]{2})[/\-]?([0-9]{5,7})$", re.IGNORECASE)
_JP_BARE_RE = re.compile(r"^JP\s*([0-9]{6,9})$", re.IGNORECASE)

# --- CN ---
_CN_RE = re.compile(r"^CN\s*([0-9]{9,13})[.\-]?([A-Z]?)$", re.IGNORECASE)

# --- KR ---
_KR_REG_RE = re.compile(r"^KR\s*10\s*-?\s*([0-9]{7})$", re.IGNORECASE)
_KR_APP_RE = re.compile(r"^KR\s*10\s*-?\s*(20[0-9]{2})\s*-?\s*([0-9]{6,7})$", re.IGNORECASE)

# --- AU ---
_AU_RE = re.compile(r"^AU\s*([0-9]{6,12})$", re.IGNORECASE)

# --- CA ---
_CA_RE = re.compile(r"^CA\s*([0-9\s]{6,10})$", re.IGNORECASE)

# --- NZ ---
_NZ_RE = re.compile(r"^NZ\s*([0-9]{5,8})$", re.IGNORECASE)

# --- BR ---
_BR_RE = re.compile(r"^BR\s*(10|11|20|PI)?\s*([0-9]{6,12})[.\-]?([0-9]?)$", re.IGNORECASE)

# --- IN ---
_IN_RE = re.compile(r"^IN\s*([0-9]{6,12})\s*([A-Z]*)\s*$", re.IGNORECASE)

# --- Generic ISO 2-letter prefix (Tier 2/3) ---
_ISO_PREFIX_RE = re.compile(r"^([A-Z]{2})\s*([0-9][0-9A-Z/\-\s]{4,20})$", re.IGNORECASE)

# --- URL (Google Patents, Espacenet, etc.) ---
_GOOGLE_URL_RE = re.compile(
    r"https?://patents\.google\.com/patent/([A-Z]{2}[0-9A-Z]+?)(?:/[a-z]{2})?$",
    re.IGNORECASE,
)
_ESPACENET_URL_RE = re.compile(
    r"https?://.*espacenet\.com.*?/([A-Z]{2}[0-9][0-9A-Z]*)(?:/|\?|$)",
    re.IGNORECASE,
)


# ---------------------------------------------------------------------------
# Core parsing
# ---------------------------------------------------------------------------

def _strip_kind(s: str) -> tuple[str, str | None]:
    """Strip trailing kind code from a number string. Return (number, kind_code)."""
    m = _KIND_RE.search(s)
    if m:
        kind = m.group(1)
        number = s[: m.start()]
        if number:  # don't strip if the whole string is the "kind code"
            return number, kind
    return s, None


def _remove_separators(s: str) -> str:
    """Remove spaces, commas, dashes from a number string."""
    return re.sub(r"[\s,\-]", "", s)


def canonicalize(raw_id: str) -> CanonicalPatentId:
    """Parse a patent ID in any format and return the canonical representation."""
    s = raw_id.strip()
    errors: list[str] = []

    # --- URL extraction ---
    for url_re in (_GOOGLE_URL_RE, _ESPACENET_URL_RE):
        m = url_re.match(s)
        if m:
            s = m.group(1)
            break

    upper = s.upper()

    # --- US applications (US + year/serial) ---
    m = _US_APP_RE.match(upper)
    if m:
        year, serial = m.group(1), m.group(2)
        kind = m.group(3) or None  # kind code e.g. "A1", may be None
        canonical = f"US{year}{serial.zfill(7)}"
        return CanonicalPatentId(
            raw=raw_id, canonical=canonical, jurisdiction="US",
            number=f"{year}{serial.zfill(7)}", kind_code=kind,
            doc_type="application", filing_year=int(year),
        )

    # --- US granted patents ---
    m = _US_PATENT_RE.match(upper)
    if m:
        num_raw = m.group(1)
        num_clean = _remove_separators(num_raw)
        num, kind = _strip_kind(num_clean)
        canonical = f"US{num}"
        return CanonicalPatentId(
            raw=raw_id, canonical=canonical, jurisdiction="US",
            number=num, kind_code=kind, doc_type="patent", filing_year=None,
        )

    # --- EP ---
    m = _EP_RE.match(upper)
    if m:
        num_raw = m.group(1)
        num_clean = _remove_separators(num_raw)
        num, kind = _strip_kind(num_clean)
        canonical = f"EP{num}"
        return CanonicalPatentId(
            raw=raw_id, canonical=canonical, jurisdiction="EP",
            number=num, kind_code=kind, doc_type="patent", filing_year=None,
        )

    # --- WO/PCT ---
    m = _WO_RE.match(upper)
    if m:
        year, serial = m.group(1), m.group(2)
        num, kind = _strip_kind(serial)
        canonical = f"WO{year}{num}"
        return CanonicalPatentId(
            raw=raw_id, canonical=canonical, jurisdiction="WO",
            number=f"{year}{num}", kind_code=kind,
            doc_type="application", filing_year=int(year),
        )

    # --- JP modern ---
    m = _JP_MODERN_RE.match(upper)
    if m:
        year, serial = m.group(1), m.group(2)
        canonical = f"JP{year}{serial}"
        return CanonicalPatentId(
            raw=raw_id, canonical=canonical, jurisdiction="JP",
            number=f"{year}{serial}", kind_code=None,
            doc_type="application", filing_year=int(year),
        )

    # --- JP bare ---
    m = _JP_BARE_RE.match(upper)
    if m:
        num = m.group(1)
        canonical = f"JP{num}"
        return CanonicalPatentId(
            raw=raw_id, canonical=canonical, jurisdiction="JP",
            number=num, kind_code=None, doc_type="patent", filing_year=None,
        )

    # --- CN ---
    m = _CN_RE.match(upper)
    if m:
        num, kind_suffix = m.group(1), m.group(2)
        # Remove dots from CN numbers (e.g. CN201910123456.X → CN201910123456X)
        num = num.replace(".", "")
        kind = kind_suffix if kind_suffix else None
        canonical = f"CN{num}{kind or ''}"
        return CanonicalPatentId(
            raw=raw_id, canonical=canonical, jurisdiction="CN",
            number=num, kind_code=kind, doc_type="patent", filing_year=None,
        )

    # --- KR application ---
    m = _KR_APP_RE.match(upper)
    if m:
        year, serial = m.group(1), m.group(2)
        canonical = f"KR10{year}{serial.zfill(7)}"
        return CanonicalPatentId(
            raw=raw_id, canonical=canonical, jurisdiction="KR",
            number=f"10{year}{serial.zfill(7)}", kind_code=None,
            doc_type="application", filing_year=int(year),
        )

    # --- KR registered ---
    m = _KR_REG_RE.match(upper)
    if m:
        num = m.group(1)
        canonical = f"KR10{num}"
        return CanonicalPatentId(
            raw=raw_id, canonical=canonical, jurisdiction="KR",
            number=f"10{num}", kind_code=None, doc_type="patent", filing_year=None,
        )

    # --- AU ---
    m = _AU_RE.match(upper)
    if m:
        num = _remove_separators(m.group(1))
        return CanonicalPatentId(
            raw=raw_id, canonical=f"AU{num}", jurisdiction="AU",
            number=num, kind_code=None, doc_type="patent", filing_year=None,
        )

    # --- CA ---
    m = _CA_RE.match(upper)
    if m:
        num = _remove_separators(m.group(1))
        return CanonicalPatentId(
            raw=raw_id, canonical=f"CA{num}", jurisdiction="CA",
            number=num, kind_code=None, doc_type="patent", filing_year=None,
        )

    # --- NZ ---
    m = _NZ_RE.match(upper)
    if m:
        num = m.group(1)
        return CanonicalPatentId(
            raw=raw_id, canonical=f"NZ{num}", jurisdiction="NZ",
            number=num, kind_code=None, doc_type="patent", filing_year=None,
        )

    # --- BR ---
    m = _BR_RE.match(upper)
    if m:
        prefix = (m.group(1) or "").strip()
        num = m.group(2)
        canonical = f"BR{prefix}{num}"
        return CanonicalPatentId(
            raw=raw_id, canonical=canonical, jurisdiction="BR",
            number=f"{prefix}{num}", kind_code=None, doc_type="patent", filing_year=None,
        )

    # --- IN ---
    m = _IN_RE.match(upper)
    if m:
        num, suffix = m.group(1), m.group(2).strip()
        canonical = f"IN{num}{suffix}"
        return CanonicalPatentId(
            raw=raw_id, canonical=canonical, jurisdiction="IN",
            number=num, kind_code=None, doc_type="patent", filing_year=None,
        )

    # --- Bare US numbers (7-digit → assume US granted) ---
    m = _US_BARE_RE.match(s)
    if m:
        num = m.group(1)
        return CanonicalPatentId(
            raw=raw_id, canonical=f"US{num}", jurisdiction="US",
            number=num, kind_code=None, doc_type="patent", filing_year=None,
        )

    # --- Bare 11-digit US application (20XXXXXXXXX) ---
    m = _US_APP_BARE_RE.match(s)
    if m:
        year, serial = m.group(1), m.group(2)
        canonical = f"US{year}{serial}"
        return CanonicalPatentId(
            raw=raw_id, canonical=canonical, jurisdiction="US",
            number=f"{year}{serial}", kind_code=None,
            doc_type="application", filing_year=int(year),
        )

    # --- Generic ISO 2-letter prefix (Tier 2/3 passthrough) ---
    m = _ISO_PREFIX_RE.match(upper)
    if m:
        jx = m.group(1)
        num_raw = _remove_separators(m.group(2))
        num, kind = _strip_kind(num_raw)
        # Only treat as valid if the jurisdiction is a known 2-letter code
        canonical = f"{jx}{num}"
        return CanonicalPatentId(
            raw=raw_id, canonical=canonical, jurisdiction=jx,
            number=num, kind_code=kind, doc_type="unknown", filing_year=None,
        )

    # --- Fallback: can't parse ---
    errors.append(f"Could not parse patent ID: {raw_id!r}")
    canonical_fallback = raw_id.upper().replace(" ", "")
    if not canonical_fallback:
        canonical_fallback = "UNKNOWN"
    return CanonicalPatentId(
        raw=raw_id, canonical=canonical_fallback,
        jurisdiction="UNKNOWN", number=raw_id,
        kind_code=None, doc_type="unknown", filing_year=None,
        errors=errors,
    )


def canonicalize_batch(raw_ids: list[str]) -> list[CanonicalPatentId]:
    """Canonicalize a list of IDs, preserving order."""
    return [canonicalize(rid) for rid in raw_ids]


def is_valid(raw_id: str) -> bool:
    """Return True if the ID can be parsed without errors."""
    result = canonicalize(raw_id)
    return not result.errors and result.jurisdiction != "UNKNOWN"
