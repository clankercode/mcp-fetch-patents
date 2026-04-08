"""Configuration system: env vars + TOML config file, XDG paths."""
from __future__ import annotations

import os
import sys
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

if sys.version_info >= (3, 11):
    import tomllib
else:
    try:
        import tomllib
    except ImportError:
        import tomli as tomllib  # type: ignore[no-redef]


# ---------------------------------------------------------------------------
# XDG helpers
# ---------------------------------------------------------------------------

def xdg_data_home() -> Path:
    """Return $XDG_DATA_HOME or ~/.local/share."""
    xdg = os.environ.get("XDG_DATA_HOME", "")
    if xdg:
        return Path(xdg)
    return Path.home() / ".local" / "share"


def default_global_db() -> Path:
    return xdg_data_home() / "patent-cache" / "index.db"


def default_local_cache() -> Path:
    return Path(".patents")


# ---------------------------------------------------------------------------
# Config dataclass
# ---------------------------------------------------------------------------

@dataclass
class PatentConfig:
    # Cache paths
    cache_local_dir: Path = field(default_factory=default_local_cache)
    cache_global_db: Path = field(default_factory=default_global_db)

    # Source behavior
    source_priority: list[str] = field(default_factory=lambda: [
        "USPTO", "EPO_OPS", "BigQuery", "Espacenet",
        "WIPO_Scrape",
        "IP_Australia", "CIPO",
        "Google_Patents", "web_search",
    ])
    concurrency: int = 10
    fetch_all_sources: bool = True
    timeout: float = 30.0

    # Converter chain
    converters_order: list[str] = field(default_factory=lambda: [
        "pymupdf4llm", "pdfplumber", "pdftotext", "marker"
    ])
    converters_disabled: list[str] = field(default_factory=lambda: ["marker"])

    # API keys (None = not configured)
    epo_client_id: str | None = None
    epo_client_secret: str | None = None
    lens_api_key: str | None = None
    serpapi_key: str | None = None
    bing_key: str | None = None

    # Agent for postprocess_query
    agent_command: str = "claude"

    # Logging
    log_level: str = "info"

    # Per-source base URL overrides (for testing)
    source_base_urls: dict[str, str] = field(default_factory=dict)

    # Misc test flags
    disable_marker: bool = True  # True = marker in converters_disabled


# ---------------------------------------------------------------------------
# Loader
# ---------------------------------------------------------------------------

_BOOL_TRUE = {"1", "true", "yes", "on"}


def _parse_bool(v: str) -> bool:
    return v.strip().lower() in _BOOL_TRUE


def _find_toml_paths() -> list[Path]:
    """Auto-discover config files: cwd first, then HOME."""
    paths = []
    cwd_config = Path(".patents.toml")
    if cwd_config.exists():
        paths.append(cwd_config)
    home_config = Path.home() / ".patents.toml"
    if home_config.exists():
        paths.append(home_config)
    return paths


def _apply_toml(cfg: PatentConfig, data: dict[str, Any]) -> None:
    cache = data.get("cache", {})
    if "local_dir" in cache:
        cfg.cache_local_dir = Path(cache["local_dir"])
    if "global_db" in cache:
        cfg.cache_global_db = Path(cache["global_db"])

    sources = data.get("sources", {})
    if "priority" in sources:
        cfg.source_priority = list(sources["priority"])
    if "concurrency" in sources:
        cfg.concurrency = int(sources["concurrency"])
    if "fetch_all_sources" in sources:
        cfg.fetch_all_sources = bool(sources["fetch_all_sources"])
    if "timeout_seconds" in sources:
        cfg.timeout = float(sources["timeout_seconds"])

    epo = sources.get("epo_ops", {})
    if "client_id" in epo:
        cfg.epo_client_id = epo["client_id"] or None
    if "client_secret" in epo:
        cfg.epo_client_secret = epo["client_secret"] or None

    lens = sources.get("lens", {})
    if "api_key" in lens:
        cfg.lens_api_key = lens["api_key"] or None

    serpapi = sources.get("serpapi", {})
    if "api_key" in serpapi:
        cfg.serpapi_key = serpapi["api_key"] or None

    bing = sources.get("bing", {})
    if "api_key" in bing:
        cfg.bing_key = bing["api_key"] or None

    converters = data.get("converters", {})
    if "pdf_to_markdown_order" in converters:
        cfg.converters_order = list(converters["pdf_to_markdown_order"])
    if "disable" in converters:
        cfg.converters_disabled = list(converters["disable"])

    agent = data.get("agent", {})
    if "command" in agent:
        cfg.agent_command = agent["command"]

    logging_cfg = data.get("logging", {})
    if "level" in logging_cfg:
        cfg.log_level = logging_cfg["level"]


def _apply_env(cfg: PatentConfig, env: dict[str, str]) -> None:
    if v := env.get("PATENT_CACHE_DIR"):
        cfg.cache_local_dir = Path(v)
    if v := env.get("PATENT_GLOBAL_DB"):
        cfg.cache_global_db = Path(v)
    if v := env.get("PATENT_CONCURRENCY"):
        try:
            cfg.concurrency = int(v)
        except (ValueError, TypeError):
            pass  # ignore invalid integer; keep default
    if v := env.get("PATENT_TIMEOUT"):
        try:
            cfg.timeout = float(v)
        except (ValueError, TypeError):
            pass  # ignore invalid float; keep default
    if v := env.get("PATENT_FETCH_ALL_SOURCES"):
        cfg.fetch_all_sources = _parse_bool(v)
    if v := env.get("PATENT_DISABLE_MARKER"):
        if _parse_bool(v) and "marker" not in cfg.converters_disabled:
            cfg.converters_disabled = list(cfg.converters_disabled) + ["marker"]
    if v := env.get("PATENT_EPO_KEY"):
        parts = v.split(":", 1)
        cfg.epo_client_id = parts[0] or None
        cfg.epo_client_secret = (parts[1] if len(parts) > 1 else None) or None
    if v := env.get("PATENT_LENS_KEY"):
        cfg.lens_api_key = v or None
    if v := env.get("PATENT_SERPAPI_KEY"):
        cfg.serpapi_key = v or None
    if v := env.get("PATENT_BING_KEY"):
        cfg.bing_key = v or None
    if v := env.get("PATENT_AGENT_CMD"):
        cfg.agent_command = v
    if v := env.get("PATENT_LOG_LEVEL"):
        cfg.log_level = v


try:
    from dotenv import load_dotenv
except ImportError:  # pragma: no cover
    def load_dotenv(*args, **kwargs): pass  # type: ignore[misc]


def load_config(
    env: dict[str, str] | None = None,
    toml_path: Path | None = None,
    overrides: dict[str, Any] | None = None,
) -> PatentConfig:
    """Load config: defaults → TOML files → env vars → overrides."""
    load_dotenv()  # loads .env from cwd or parent dirs, no-op if not found
    cfg = PatentConfig()

    # 1. TOML files (lowest priority after defaults)
    toml_paths = [toml_path] if toml_path is not None else _find_toml_paths()
    for p in toml_paths:
        if p and p.exists():
            with open(p, "rb") as f:
                data = tomllib.load(f)
            _apply_toml(cfg, data)

    # 2. Environment variables
    _apply_env(cfg, env if env is not None else dict(os.environ))

    # 3. Programmatic overrides (tests)
    if overrides:
        for k, v in overrides.items():
            if hasattr(cfg, k):
                setattr(cfg, k, v)

    # Sync disable_marker flag
    cfg.disable_marker = "marker" in cfg.converters_disabled

    return cfg


# Singleton for production use (loaded once)
_config: PatentConfig | None = None


def get_config() -> PatentConfig:
    global _config
    if _config is None:
        _config = load_config()
    return _config
