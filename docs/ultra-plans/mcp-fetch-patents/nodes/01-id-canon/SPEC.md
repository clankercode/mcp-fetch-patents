# SPEC — 01-id-canon: Patent ID Canonicalization

## Responsibility
Parse patent IDs in any format any user/agent might provide, normalize them to a canonical form, and expose metadata about the patent type and jurisdiction.

## Inputs
Raw patent ID strings. Examples of what users might provide:
- `US7654321` / `US 7,654,321` / `7654321` (implied US)
- `US2024/0123456` / `US20240123456` (US application)
- `EP1234567` / `EP 1 234 567 A1` / `EP1234567B1`
- `WO2024/123456` / `WO2024123456`
- `JP2023-123456` / `JP4567890`
- `CN201910123456.X` / `CN112345678A`
- `KR10-2023-0012345` / `KR102345678`
- `AU2023123456` / `AU2023100123`
- `CA3012345` / `CA 3 012 345`
- `NZ123456`
- `BR102023012345-0`
- `IN202317001234`
- `GCC/P/2023/12345` (Gulf Cooperation Council)
- Bulk lists / comma-separated / newline-separated

## Outputs
`CanonicalPatentId` struct / dataclass:
```
{
  "raw": "US 7,654,321",
  "canonical": "US7654321",
  "jurisdiction": "US",
  "number": "7654321",
  "kind_code": null,             # e.g. "B1", "A1", "B2"
  "doc_type": "patent",          # "patent" | "application" | "unknown"
  "filing_year": null,           # if determinable from ID
  "errors": []                   # parse warnings/ambiguities
}
```

## Canonicalization Rules
1. Strip whitespace and punctuation separators
2. Uppercase jurisdiction code
3. Remove comma separators from US numbers
4. Normalize application numbers to `{CC}{YEAR}/{SERIAL}` form
5. Separate kind codes (A1, B1, B2, etc.) from document number; store separately
6. If no jurisdiction prefix and number looks like a US patent (7 digits), assume US
7. If ambiguous, populate `errors` with explanation and best guess

## Jurisdiction Coverage (v1)
**Tier 1 (full regex + tests):** US, EP, WO, JP, CN, KR, AU, CA, NZ, BR, IN
**Tier 2 (best-effort regex):** DE, FR, GB, IT, ES, NL, SE, CH, DK, FI, NO, AT, BE, GCC
**Tier 3 (passthrough with jurisdiction tag):** All others — accept ISO 2-letter prefix + number

## Implementation Notes
- Pure functions, no I/O, no network
- Implemented identically in Python and Rust (same logic, different syntax)
- ~20-30 regex patterns for Tier 1 jurisdictions
- Should handle malformed input gracefully (return best guess + error fields)
- Batch canonicalization: accepts list of strings, returns list of results

## Test Surface
- Unit tests: one per jurisdiction per format variant
- Round-trip: `canonical → parse → canonical` is identity
- Fuzzing: random strings should not panic/throw, always return a result
- Cross-impl: Python canonical == Rust canonical for all fixture inputs

## Dependencies
None (pure parsing)

## Feasibility Notes
None — pure string processing, straightforward.
