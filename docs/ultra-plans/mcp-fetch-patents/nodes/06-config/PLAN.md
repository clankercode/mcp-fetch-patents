# PLAN — 06-config: Configuration System

*TDD task list. Implement Python first, then Rust mirror.*
*All tasks: RED (write failing test) → GREEN (implement) → REFACTOR*

---

## Python Implementation

### T01 — Default config object
- **RED**: `test_default_config_has_expected_values` — call `load_config()` with no args; assert `concurrency==10`, `timeout==30.0`, `log_level=="info"`, `fetch_all_sources==True`, `converters_order==["pymupdf4llm","pdfplumber","pdftotext","marker"]`, `converters_disabled==["marker"]`
- **GREEN**: implement `PatentConfig` dataclass + `DEFAULT_CONFIG` + `load_config()` returning defaults
- **REFACTOR**: ensure `PatentConfig` is fully typed; no mutable defaults

### T02 — Load from environment variables
- **RED**: `test_env_vars_override_defaults` — set `PATENT_CONCURRENCY=5`, `PATENT_LOG_LEVEL=debug`; call `load_config(env={"PATENT_CONCURRENCY": "5", "PATENT_LOG_LEVEL": "debug"})`; assert values
- **GREEN**: add env var parsing in `load_config()`; map each `PATENT_*` var to its field
- **REFACTOR**: centralize env var name → field mapping in a dict

### T03 — Load from TOML config file
- **RED**: `test_toml_config_file_loaded` — write a temp `~/.patents.toml` with `[cache] local_dir = "/tmp/patents"`; call `load_config(toml_path=tmp_path)`; assert `cache_local_dir == Path("/tmp/patents")`
- **GREEN**: add TOML parsing with `tomllib` (Python 3.11+) or `tomli` fallback
- **REFACTOR**: handle missing TOML keys gracefully (use defaults)

### T04 — Env vars override TOML
- **RED**: `test_env_overrides_toml` — TOML has `concurrency=3`; env has `PATENT_CONCURRENCY=7`; assert `concurrency==7`
- **GREEN**: apply merge order: defaults → TOML → env vars

### T05 — XDG path resolution
- **RED**: `test_xdg_data_home_used_when_set` — set `XDG_DATA_HOME=/tmp/xdg`; assert `default_global_db() == Path("/tmp/xdg/patent-cache/index.db")`
- **RED**: `test_xdg_fallback_to_home` — unset `XDG_DATA_HOME`; assert db path under `~/.local/share/`
- **GREEN**: implement `xdg_data_home()` and `default_global_db()`

### T06 — Missing API keys return None
- **RED**: `test_missing_api_key_is_none` — no env vars set; assert `config.epo_client_id is None`, `config.lens_api_key is None`, etc.
- **GREEN**: all API key fields default to None

### T07 — source_base_urls overrides (for testing)
- **RED**: `test_source_base_url_override` — pass `overrides={"source_base_urls": {"USPTO": "http://localhost:18080"}}`; assert `config.source_base_urls["USPTO"] == "http://localhost:18080"`
- **GREEN**: add `overrides` param to `load_config()`; merge over TOML + env

### T08 — TOML file discovery (auto-find ~/.patents.toml)
- **RED**: `test_auto_discover_home_toml` — create `tmp_home/.patents.toml`; call `load_config()` with patched HOME; verify it's loaded
- **GREEN**: implement `_find_toml_paths()` that checks `cwd/.patents.toml` then `~/.patents.toml`

---

## Rust Implementation

### T09 — Rust: config struct + env loading
- **RED**: `test_default_config` in `config/tests.rs` — assert same defaults as Python
- **GREEN**: `PatentConfig` struct in Rust; `load_config()` reads `PATENT_*` env vars; use `dirs` crate for XDG paths
- **REFACTOR**: ensure field names and types are identical to Python (same JSON serialization)

### T10 — Rust: TOML loading
- **RED**: `test_toml_config_loaded` — parse a temp TOML file; assert values
- **GREEN**: use `toml` crate for deserialization

### T11 — Rust: precedence matches Python
- **RED**: `test_env_overrides_toml_rust` — same scenario as T04
- **GREEN**: same merge logic

---

## Acceptance Criteria
- `load_config()` is deterministic and side-effect-free
- Python `PatentConfig` and Rust `PatentConfig` serialize to identical JSON for same inputs
- `source_base_urls` override enables complete test isolation
- No panics or crashes on any valid/invalid input

## Dependencies
None (this is the foundation)
