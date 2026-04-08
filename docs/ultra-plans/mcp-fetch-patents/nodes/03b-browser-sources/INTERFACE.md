# INTERFACE — 03b-browser-sources

## Exposes
```python
from patent_mcp.fetchers.browser import GooglePatentsSource

# GooglePatentsSource is a BasePatentSource subclass
# Internally spawns playwright_runner subprocess

# Subprocess JSON protocol (for Rust bridge)
# Stdin to subprocess:
@dataclass
class PlaywrightRequest:
    canonical_id: str
    output_dir: str          # absolute path
    source: str              # "google_patents" | "jplatpat" | etc.
    config: dict             # serialized relevant config fields

# Stdout from subprocess (same SourceAttempt schema):
# { "source_name": "Google_Patents", "success": true/false, ... }

# Test-mode interface:
# When PATENT_PLAYWRIGHT_MOCK_DIR is set, reads HTML from that directory
# instead of launching a real browser. Mock dir structure:
#   {MOCK_DIR}/google_patents/{canonical_id}.html
```

## Rust Equivalents
```rust
pub struct BrowserSource {
    python_cmd: String,   // default: "python"
}
// Implements PatentSource trait by spawning Python subprocess
// Returns SourceAttempt with success=false if Python unavailable
```

## Depends On
01-id-canon, 06-config
(Does NOT depend on 02-cache-db — file writing done by orchestrator, not browser source)

## Consumed By
03-source-fetchers (orchestrator)
