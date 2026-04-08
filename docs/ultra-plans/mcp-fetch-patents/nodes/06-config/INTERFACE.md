# INTERFACE — 06-config

## Exposes
```python
@dataclass
class PatentConfig:
    cache_local_dir: Path
    cache_global_db: Path
    source_priority: list[str]
    concurrency: int
    fetch_all_sources: bool
    timeout: float
    converters_order: list[str]
    converters_disabled: list[str]
    epo_client_id: str | None
    epo_client_secret: str | None
    lens_api_key: str | None
    serpapi_key: str | None
    bing_key: str | None
    agent_command: str
    log_level: str
    # Per-source base URL overrides (for testing)
    source_base_urls: dict[str, str]

def load_config(
    env: dict[str, str] | None = None,        # defaults to os.environ
    toml_path: Path | None = None,             # auto-discovers ~/.patents.toml
    overrides: dict | None = None              # programmatic overrides (tests)
) -> PatentConfig: ...

DEFAULT_CONFIG: PatentConfig
```

## Depends On
Nothing.

## Consumed By
All other nodes.
