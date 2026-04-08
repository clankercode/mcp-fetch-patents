# INTERFACE — 02-cache-db

## Exposes
```python
class PatentCache:
    def __init__(self, config: PatentConfig): ...
    def lookup(self, canonical_id: str) -> CacheResult | None: ...
    def store(self, canonical_id: str, artifacts: ArtifactSet, metadata: PatentMetadata) -> None: ...
    def register_cache_dir(self, cache_dir: Path) -> None: ...
    def list_all(self) -> list[CacheEntry]: ...

@dataclass
class CacheResult:
    canonical_id: str
    cache_dir: Path
    files: dict[str, Path]   # format -> absolute path
    metadata: PatentMetadata
    is_complete: bool        # all requested formats present

@dataclass
class ArtifactSet:
    pdf: Path | None
    txt: Path | None
    md: Path | None
    images: list[Path]
    raw: list[Path]

@dataclass
class PatentMetadata:
    canonical_id: str
    jurisdiction: str
    doc_type: str
    title: str | None
    abstract: str | None
    inventors: list[str]
    assignee: str | None
    filing_date: str | None
    publication_date: str | None
    grant_date: str | None
    fetched_at: str
    legal_status: str | None      # always null in v1
    status_fetched_at: str | None # always null in v1
```

## Also Exposes
```python
# Session token cache (used by 03a HTTP sources for PPUBS, EPO OPS, etc.)
from patent_mcp.cache import SessionCache, SessionToken

@dataclass
class SessionToken:
    token: str
    expires_at: datetime   # UTC

class SessionCache:
    """In-memory session token cache. Per-process, not persisted to disk."""
    def get(self, source: str) -> str | None:
        """Return valid token or None if missing/expired."""
    def set(self, source: str, token: str, ttl_minutes: int) -> None: ...
    def invalidate(self, source: str) -> None: ...
    def set_with_expiry(self, source: str, token: str, expires_at: datetime) -> None:
        """Use when source provides explicit expiry (e.g. EPO OPS OAuth response)."""
```

## Depends On
01-id-canon, 06-config

## Consumed By
03-source-fetchers, 05-mcp-protocol, 07-test-infra
