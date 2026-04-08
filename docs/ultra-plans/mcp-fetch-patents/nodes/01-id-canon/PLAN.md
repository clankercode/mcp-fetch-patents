# PLAN — 01-id-canon: Patent ID Canonicalization

*TDD task list. Pure functions — no I/O, no mocks needed.*
*All tasks: RED → GREEN → REFACTOR*

---

## Python Implementation

### T01 — US utility patent (granted)
- **RED**: `test_us_patent_bare_number` — `canonicalize("7654321")` → `canonical=="US7654321"`, `jurisdiction=="US"`, `doc_type=="patent"`
- **RED**: `test_us_patent_prefixed` — `canonicalize("US7654321")` → same result
- **RED**: `test_us_patent_with_commas` — `canonicalize("US 7,654,321")` → `canonical=="US7654321"`
- **GREEN**: implement US regex pattern; strip spaces + commas; 7-digit bare number → assume US
- **REFACTOR**: extract `_normalize_us_number()` helper

### T02 — US utility patent with kind code
- **RED**: `test_us_kind_code_extracted` — `canonicalize("US7654321B2")` → `kind_code=="B2"`, `number=="7654321"`
- **RED**: `test_us_a1_kind_code` — `canonicalize("US20240123456A1")` → `kind_code=="A1"`, `doc_type=="application"`
- **GREEN**: regex to separate number from kind code `[A-Z][0-9]?`

### T03 — US patent applications
- **RED**: `test_us_application_slash_format` — `canonicalize("US2024/0123456")` → `canonical=="US20240123456"`, `doc_type=="application"`, `filing_year==2024`
- **RED**: `test_us_application_no_slash` — `canonicalize("US20240123456")` → same
- **GREEN**: detect application format (starts with year 19xx/20xx after US prefix, 11 digits total)

### T04 — EP patents
- **RED**: `test_ep_bare` — `canonicalize("EP1234567")` → `jurisdiction=="EP"`, `canonical=="EP1234567"`
- **RED**: `test_ep_with_spaces` — `canonicalize("EP 1 234 567 A1")` → `canonical=="EP1234567"`, `kind_code=="A1"`
- **RED**: `test_ep_b1` — `canonicalize("EP1234567B1")` → `kind_code=="B1"`
- **GREEN**: EP regex

### T05 — WO/PCT applications
- **RED**: `test_wo_slash` — `canonicalize("WO2024/123456")` → `jurisdiction=="WO"`, `canonical=="WO2024123456"`, `filing_year==2024`
- **RED**: `test_wo_no_slash` — `canonicalize("WO2024123456")` → same
- **GREEN**: WO regex

### T06 — JP patents
- **RED**: `test_jp_h_era` — `canonicalize("JP2023-123456")` → `jurisdiction=="JP"`, `canonical=="JP2023123456"`
- **RED**: `test_jp_bare` — `canonicalize("JP4567890")` → `jurisdiction=="JP"`
- **GREEN**: JP regex; handle dash separator

### T07 — CN patents
- **RED**: `test_cn_invention` — `canonicalize("CN112345678A")` → `jurisdiction=="CN"`, `kind_code=="A"`
- **RED**: `test_cn_application_with_dot` — `canonicalize("CN201910123456.X")` → `jurisdiction=="CN"`, `canonical=="CN201910123456X"` (normalize dot)
- **GREEN**: CN regex

### T08 — KR patents
- **RED**: `test_kr_registered` — `canonicalize("KR102345678")` → `jurisdiction=="KR"`
- **RED**: `test_kr_application` — `canonicalize("KR10-2023-0012345")` → `jurisdiction=="KR"`, `canonical=="KR1020230012345"`
- **GREEN**: KR regex

### T09 — AU, CA, NZ
- **RED**: `test_au_patent` — `canonicalize("AU2023123456")` → `jurisdiction=="AU"`
- **RED**: `test_ca_patent` — `canonicalize("CA3012345")` → `jurisdiction=="CA"`
- **RED**: `test_nz_patent` — `canonicalize("NZ123456")` → `jurisdiction=="NZ"`
- **GREEN**: AU/CA/NZ regexes

### T10 — BR, IN
- **RED**: `test_br_patent` — `canonicalize("BR102023012345-0")` → `jurisdiction=="BR"`, canonical has hyphen stripped
- **RED**: `test_in_patent` — `canonicalize("IN202317001234")` → `jurisdiction=="IN"`
- **GREEN**: BR/IN regexes

### T11 — Tier 2 jurisdictions (ISO prefix passthrough)
- **RED**: `test_de_passthrough` — `canonicalize("DE102023001234")` → `jurisdiction=="DE"`, `canonical=="DE102023001234"`
- **RED**: `test_unknown_jurisdiction` — `canonicalize("XX9999999")` → `jurisdiction=="XX"`, `errors` list non-empty (ambiguous)
- **GREEN**: generic ISO-2-prefix + digits pattern for Tier 2/3

### T12 — URL input (Google Patents URL)
- **RED**: `test_google_patents_url` — `canonicalize("https://patents.google.com/patent/US7654321B2/en")` → `canonical=="US7654321"`, `kind_code=="B2"`
- **GREEN**: detect `https?://` prefix; extract patent ID from URL path

### T13 — Batch canonicalization
- **RED**: `test_batch_same_length` — `canonicalize_batch(["US7654321", "EP1234567"])` returns list of 2
- **RED**: `test_batch_preserves_order` — input order preserved in output
- **GREEN**: `canonicalize_batch` as thin wrapper over `canonicalize`

### T14 — Invalid / malformed input
- **RED**: `test_empty_string` — `canonicalize("")` → `errors` non-empty, no exception
- **RED**: `test_random_garbage` — `canonicalize("ABCDEFGH!@#$")` → no exception, errors populated
- **RED**: `test_is_valid_false` — `is_valid("notapatent")` → `False`
- **RED**: `test_is_valid_true` — `is_valid("US7654321")` → `True`
- **GREEN**: catch-all fallback in canonicalize; never raise

### T15 — Round-trip property
- **RED**: `test_roundtrip` — for all fixture patent IDs: `canonicalize(canonicalize(id).canonical).canonical == canonicalize(id).canonical`
- **GREEN**: should pass if T01–T14 all pass

---

## Rust Implementation

### T16 — Rust: all T01–T15 mirrored in `id_canon/tests.rs`
- **RED**: write all test cases using Rust `canonicalize()` function
- **GREEN**: implement same regex patterns; use `regex` crate
- **REFACTOR**: share regex definitions at module level (compiled once)

### T17 — Parity test: Python canonical == Rust canonical for all fixtures
- **RED**: `test_parity_all_fixtures` in `cross_impl/test_id_canon_parity.py` — for each fixture ID, compare Python output JSON == Rust binary output JSON
- **GREEN**: should pass if T16 is correct

---

## Acceptance Criteria
- All `canonicalize()` tests pass in <50ms total (pure computation, no I/O)
- No panics or exceptions on any string input
- Python and Rust produce byte-identical canonical strings for all fixture IDs
- `is_valid()` correctly rejects at least common non-patent strings

## Dependencies
None
