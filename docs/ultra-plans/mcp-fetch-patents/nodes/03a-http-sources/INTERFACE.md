# INTERFACE — 03a-http-sources

## Exposes
```python
from patent_mcp.fetchers.http import (
    PpubsSource,
    EpoOpsSource,
    BigQuerySource,       # optional; degrades gracefully if GCP not configured
    EspacenetSource,
    WipoScrapeSource,
    IpAustraliaSource,
    CipoScrapeSource,
    PatentsViewStubSource,  # returns deprecation message only
    # Country-specific (all follow same BasePatentSource interface)
)

# All HTTP sources are subclasses of BasePatentSource (defined in 03-source-fetchers)
# Each is instantiable with just config; no required constructor args beyond config

# Auth helpers (used internally + by 02-cache-db SessionCache)
class EpoOpsTokenManager:
    """OAuth2 client credentials flow for EPO OPS."""
    def __init__(self, config: PatentConfig): ...
    async def get_token(self) -> str | None:
        """Returns bearer token or None if not configured / auth failed."""

class PpubsSessionManager:
    """PPUBS session establishment and caching."""
    def __init__(self, config: PatentConfig, session_cache: SessionCache): ...
    async def get_session_token(self) -> str | None:
```

## Rust Equivalents
```rust
// All implement trait PatentSource (defined in fetchers/mod.rs)
pub struct PpubsSource { ... }
pub struct EpoOpsSource { ... }
pub struct BigQuerySource { ... }  // optional; returns Err if unconfigured
pub struct EspacenetSource { ... }
// etc.
```

## Depends On
01-id-canon, 02-cache-db (SessionCache), 06-config

## Consumed By
03-source-fetchers (orchestrator)
