# INTERFACE — 03-source-fetchers

## Exposes
```python
class FetcherOrchestrator:
    def __init__(self, config: PatentConfig): ...
    
    async def fetch(self, canonical_id: CanonicalPatentId, 
                    output_dir: Path) -> OrchestratorResult: ...
    
    async def fetch_batch(self, ids: list[CanonicalPatentId],
                          base_cache_dir: Path) -> list[OrchestratorResult]: ...

@dataclass
class OrchestratorResult:
    canonical_id: str
    success: bool           # true if at least one source succeeded
    artifacts: ArtifactSet
    metadata: PatentMetadata | None
    sources: list[SourceAttempt]
    total_duration_ms: int

@dataclass  
class SourceAttempt:
    source_name: str
    success: bool
    formats_retrieved: list[str]
    url: str | None
    error: str | None
    duration_ms: int
    metadata: dict | None = None  # source-specific extra data
    # 03c (web search fallback) stores: {"urls": [{"url":..,"title":..,"confidence":..}], "query": ".."}
    # Other sources may store supplemental data here

# Base class for all individual source fetchers
class BasePatentSource(ABC):
    @abstractmethod
    async def fetch(self, canonical_id: CanonicalPatentId, 
                    output_dir: Path, config: PatentConfig) -> SourceAttempt: ...
    
    @property
    @abstractmethod  
    def source_name(self) -> str: ...
    
    @property
    @abstractmethod
    def supported_jurisdictions(self) -> list[str]: ...  # ["US"] or ["*"] for all
```

## Depends On
01-id-canon, 02-cache-db, 03a-http-sources, 03b-browser-sources, 03c-web-search-fallback, 04-format-conversion, 06-config

## Consumed By
05-mcp-protocol, 07-test-infra
