# INTERFACE — 01-id-canon

## Exposes
```python
# Python
@dataclass
class CanonicalPatentId:
    raw: str
    canonical: str         # e.g. "US7654321"
    jurisdiction: str      # e.g. "US", "EP", "WO"
    number: str            # numeric/alphanumeric serial
    kind_code: str | None  # e.g. "B1", "A2"
    doc_type: str          # "patent" | "application" | "unknown"
    filing_year: int | None
    errors: list[str]

def canonicalize(raw_id: str) -> CanonicalPatentId: ...
def canonicalize_batch(raw_ids: list[str]) -> list[CanonicalPatentId]: ...
def is_valid(raw_id: str) -> bool: ...
```

```rust
// Rust
pub struct CanonicalPatentId { ... }
pub fn canonicalize(raw: &str) -> CanonicalPatentId;
pub fn canonicalize_batch(raw: &[&str]) -> Vec<CanonicalPatentId>;
pub fn is_valid(raw: &str) -> bool;
```

## Depends On
Nothing.

## Consumed By
02-cache-db, 03-source-fetchers, 03a, 03b, 03c, 05-mcp-protocol, 07-test-infra
