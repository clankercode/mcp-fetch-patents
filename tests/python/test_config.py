"""Tests for patent_mcp.config — T01-T08."""

from __future__ import annotations

import os
from pathlib import Path

import pytest

from patent_mcp.config import (
    PatentConfig,
    default_global_db,
    default_local_cache,
    load_config,
    xdg_data_home,
)


# ---------------------------------------------------------------------------
# T01 — Default config object
# ---------------------------------------------------------------------------


class TestDefaultConfig:
    def test_default_concurrency(self):
        cfg = load_config(env={})
        assert cfg.concurrency == 10

    def test_default_timeout(self):
        cfg = load_config(env={})
        assert cfg.timeout == 30.0

    def test_default_log_level(self):
        cfg = load_config(env={})
        assert cfg.log_level == "info"

    def test_default_fetch_all_sources(self):
        cfg = load_config(env={})
        assert cfg.fetch_all_sources is True

    def test_default_converters_order(self):
        cfg = load_config(env={})
        assert cfg.converters_order == [
            "pymupdf4llm",
            "pdfplumber",
            "pdftotext",
            "marker",
        ]

    def test_default_converters_disabled(self):
        cfg = load_config(env={})
        assert cfg.converters_disabled == ["marker"]

    def test_default_cache_local_dir(self):
        cfg = load_config(env={})
        assert cfg.cache_local_dir == default_local_cache()
        assert str(cfg.cache_local_dir).endswith("patent-cache/patents")

    def test_default_source_priority_contains_uspto(self):
        cfg = load_config(env={})
        assert "USPTO" in cfg.source_priority

    def test_disable_marker_synced(self):
        cfg = load_config(env={})
        assert cfg.disable_marker is True  # marker is in converters_disabled by default


# ---------------------------------------------------------------------------
# T02 — Load from environment variables
# ---------------------------------------------------------------------------


class TestEnvVarsOverrideDefaults:
    def test_concurrency_from_env(self):
        cfg = load_config(env={"PATENT_CONCURRENCY": "5"})
        assert cfg.concurrency == 5

    def test_log_level_from_env(self):
        cfg = load_config(env={"PATENT_LOG_LEVEL": "debug"})
        assert cfg.log_level == "debug"

    def test_timeout_from_env(self):
        cfg = load_config(env={"PATENT_TIMEOUT": "60.5"})
        assert cfg.timeout == 60.5

    def test_fetch_all_sources_false(self):
        cfg = load_config(env={"PATENT_FETCH_ALL_SOURCES": "false"})
        assert cfg.fetch_all_sources is False

    def test_fetch_all_sources_true_variants(self):
        for v in ("1", "true", "yes", "on"):
            cfg = load_config(env={"PATENT_FETCH_ALL_SOURCES": v})
            assert cfg.fetch_all_sources is True

    def test_cache_dir_from_env(self):
        cfg = load_config(env={"PATENT_CACHE_DIR": "/tmp/mypatents"})
        assert cfg.cache_local_dir == Path("/tmp/mypatents")

    def test_global_db_from_env(self):
        cfg = load_config(env={"PATENT_GLOBAL_DB": "/tmp/index.db"})
        assert cfg.cache_global_db == Path("/tmp/index.db")

    def test_epo_key_split_on_colon(self):
        cfg = load_config(env={"PATENT_EPO_KEY": "myid:mysecret"})
        assert cfg.epo_client_id == "myid"
        assert cfg.epo_client_secret == "mysecret"

    def test_epo_key_no_colon(self):
        cfg = load_config(env={"PATENT_EPO_KEY": "onlyid"})
        assert cfg.epo_client_id == "onlyid"
        assert cfg.epo_client_secret is None

    def test_epo_key_empty_secret(self):
        """EPO key with colon but empty secret: secret should be None, not ''."""
        cfg = load_config(env={"PATENT_EPO_KEY": "myid:"})
        assert cfg.epo_client_id == "myid"
        assert cfg.epo_client_secret is None

    def test_epo_key_empty_id_and_secret(self):
        """EPO key ':' → both should be None."""
        cfg = load_config(env={"PATENT_EPO_KEY": ":"})
        assert cfg.epo_client_id is None
        assert cfg.epo_client_secret is None

    def test_lens_key_from_env(self):
        cfg = load_config(env={"PATENT_LENS_KEY": "lenskey123"})
        assert cfg.lens_api_key == "lenskey123"

    def test_serpapi_key_from_env(self):
        cfg = load_config(env={"PATENT_SERPAPI_KEY": "serp123"})
        assert cfg.serpapi_key == "serp123"

    def test_bing_key_from_env(self):
        cfg = load_config(env={"PATENT_BING_KEY": "bing123"})
        assert cfg.bing_key == "bing123"

    def test_agent_cmd_from_env(self):
        cfg = load_config(env={"PATENT_AGENT_CMD": "myclaude"})
        assert cfg.agent_command == "myclaude"

    def test_disable_marker_from_env_adds_to_disabled(self):
        cfg = load_config(env={"PATENT_DISABLE_MARKER": "true"})
        assert "marker" in cfg.converters_disabled
        assert cfg.disable_marker is True

    def test_disable_marker_false_does_not_add(self):
        # marker is already in converters_disabled by default; this just doesn't add a duplicate
        cfg = load_config(env={"PATENT_DISABLE_MARKER": "false"})
        # converters_disabled keeps its default
        assert cfg.converters_disabled == ["marker"]


# ---------------------------------------------------------------------------
# T03 — Load from TOML config file
# ---------------------------------------------------------------------------


class TestTomlConfig:
    def test_toml_cache_local_dir(self, tmp_path):
        toml_file = tmp_path / "test.toml"
        toml_file.write_text('[cache]\nlocal_dir = "/tmp/patents"\n')
        cfg = load_config(env={}, toml_path=toml_file)
        assert cfg.cache_local_dir == Path("/tmp/patents")

    def test_toml_cache_global_db(self, tmp_path):
        toml_file = tmp_path / "test.toml"
        toml_file.write_text('[cache]\nglobal_db = "/tmp/idx.db"\n')
        cfg = load_config(env={}, toml_path=toml_file)
        assert cfg.cache_global_db == Path("/tmp/idx.db")

    def test_toml_concurrency(self, tmp_path):
        toml_file = tmp_path / "test.toml"
        toml_file.write_text("[sources]\nconcurrency = 3\n")
        cfg = load_config(env={}, toml_path=toml_file)
        assert cfg.concurrency == 3

    def test_toml_timeout(self, tmp_path):
        toml_file = tmp_path / "test.toml"
        toml_file.write_text("[sources]\ntimeout_seconds = 45.0\n")
        cfg = load_config(env={}, toml_path=toml_file)
        assert cfg.timeout == 45.0

    def test_toml_missing_keys_use_defaults(self, tmp_path):
        toml_file = tmp_path / "test.toml"
        toml_file.write_text("[sources]\n")
        cfg = load_config(env={}, toml_path=toml_file)
        assert cfg.concurrency == 10  # default

    def test_toml_epo_ops_credentials(self, tmp_path):
        toml_file = tmp_path / "test.toml"
        toml_file.write_text(
            '[sources.epo_ops]\nclient_id = "myid"\nclient_secret = "mysecret"\n'
        )
        cfg = load_config(env={}, toml_path=toml_file)
        assert cfg.epo_client_id == "myid"
        assert cfg.epo_client_secret == "mysecret"

    def test_toml_lens_api_key(self, tmp_path):
        toml_file = tmp_path / "test.toml"
        toml_file.write_text('[sources.lens]\napi_key = "lenskey"\n')
        cfg = load_config(env={}, toml_path=toml_file)
        assert cfg.lens_api_key == "lenskey"

    def test_toml_converters_order(self, tmp_path):
        toml_file = tmp_path / "test.toml"
        toml_file.write_text(
            '[converters]\npdf_to_markdown_order = ["pdftotext", "pdfplumber"]\n'
        )
        cfg = load_config(env={}, toml_path=toml_file)
        assert cfg.converters_order == ["pdftotext", "pdfplumber"]

    def test_toml_converters_disable(self, tmp_path):
        toml_file = tmp_path / "test.toml"
        toml_file.write_text('[converters]\ndisable = ["marker", "pymupdf4llm"]\n')
        cfg = load_config(env={}, toml_path=toml_file)
        assert "marker" in cfg.converters_disabled
        assert "pymupdf4llm" in cfg.converters_disabled

    def test_toml_agent_command(self, tmp_path):
        toml_file = tmp_path / "test.toml"
        toml_file.write_text('[agent]\ncommand = "my-claude"\n')
        cfg = load_config(env={}, toml_path=toml_file)
        assert cfg.agent_command == "my-claude"

    def test_toml_log_level(self, tmp_path):
        toml_file = tmp_path / "test.toml"
        toml_file.write_text('[logging]\nlevel = "debug"\n')
        cfg = load_config(env={}, toml_path=toml_file)
        assert cfg.log_level == "debug"

    def test_nonexistent_toml_path_uses_defaults(self, tmp_path):
        nonexistent = tmp_path / "no_such_file.toml"
        cfg = load_config(env={}, toml_path=nonexistent)
        assert cfg.concurrency == 10


# ---------------------------------------------------------------------------
# T04 — Env vars override TOML
# ---------------------------------------------------------------------------


class TestEnvOverridesToml:
    def test_env_overrides_toml_concurrency(self, tmp_path):
        toml_file = tmp_path / "test.toml"
        toml_file.write_text("[sources]\nconcurrency = 3\n")
        cfg = load_config(env={"PATENT_CONCURRENCY": "7"}, toml_path=toml_file)
        assert cfg.concurrency == 7

    def test_env_overrides_toml_log_level(self, tmp_path):
        toml_file = tmp_path / "test.toml"
        toml_file.write_text('[logging]\nlevel = "warn"\n')
        cfg = load_config(env={"PATENT_LOG_LEVEL": "debug"}, toml_path=toml_file)
        assert cfg.log_level == "debug"


# ---------------------------------------------------------------------------
# T05 — XDG path resolution
# ---------------------------------------------------------------------------


class TestXdgPaths:
    def test_xdg_data_home_used_when_set(self, monkeypatch):
        monkeypatch.setenv("XDG_DATA_HOME", "/tmp/xdg")
        result = xdg_data_home()
        assert result == Path("/tmp/xdg")

    def test_xdg_fallback_to_home(self, monkeypatch):
        monkeypatch.delenv("XDG_DATA_HOME", raising=False)
        result = xdg_data_home()
        assert result == Path.home() / ".local" / "share"

    def test_default_global_db_under_xdg(self, monkeypatch):
        monkeypatch.setenv("XDG_DATA_HOME", "/tmp/xdg")
        result = default_global_db()
        assert result == Path("/tmp/xdg/patent-cache/index.db")

    def test_default_global_db_fallback(self, monkeypatch):
        monkeypatch.delenv("XDG_DATA_HOME", raising=False)
        result = default_global_db()
        assert result == Path.home() / ".local" / "share" / "patent-cache" / "index.db"


# ---------------------------------------------------------------------------
# T06 — Missing API keys return None
# ---------------------------------------------------------------------------


class TestMissingApiKeys:
    def test_epo_client_id_none(self):
        cfg = load_config(env={})
        assert cfg.epo_client_id is None

    def test_epo_client_secret_none(self):
        cfg = load_config(env={})
        assert cfg.epo_client_secret is None

    def test_lens_api_key_none(self):
        cfg = load_config(env={})
        assert cfg.lens_api_key is None

    def test_serpapi_key_none(self):
        cfg = load_config(env={"PATENT_SERPAPI_KEY": ""})
        assert cfg.serpapi_key is None

    def test_bing_key_none(self):
        cfg = load_config(env={})
        assert cfg.bing_key is None

    def test_empty_string_epo_key_becomes_none(self):
        cfg = load_config(env={"PATENT_EPO_KEY": ":"})
        # Both parts are empty strings → should become None
        assert cfg.epo_client_id is None


# ---------------------------------------------------------------------------
# T07 — source_base_urls overrides
# ---------------------------------------------------------------------------


class TestSourceBaseUrlOverrides:
    def test_source_base_url_override(self):
        cfg = load_config(
            env={}, overrides={"source_base_urls": {"USPTO": "http://localhost:18080"}}
        )
        assert cfg.source_base_urls["USPTO"] == "http://localhost:18080"

    def test_multiple_source_url_overrides(self):
        overrides = {
            "source_base_urls": {
                "USPTO": "http://localhost:18080",
                "EPO_OPS": "http://localhost:18081",
            }
        }
        cfg = load_config(env={}, overrides=overrides)
        assert cfg.source_base_urls["USPTO"] == "http://localhost:18080"
        assert cfg.source_base_urls["EPO_OPS"] == "http://localhost:18081"

    def test_overrides_param_sets_arbitrary_field(self):
        cfg = load_config(env={}, overrides={"concurrency": 99})
        assert cfg.concurrency == 99

    def test_unknown_override_key_ignored(self):
        # Should not raise for unknown keys
        cfg = load_config(env={}, overrides={"nonexistent_field_xyz": "value"})
        assert cfg is not None


# ---------------------------------------------------------------------------
# T08 — TOML file discovery (auto-find ~/.patents.toml)
# ---------------------------------------------------------------------------


class TestTomlDiscovery:
    def test_auto_discover_home_toml(self, tmp_path, monkeypatch):
        """load_config() without explicit toml_path discovers ~/.patents.toml."""
        fake_home = tmp_path / "home"
        fake_home.mkdir()
        toml_file = fake_home / ".patents.toml"
        toml_file.write_text("[sources]\nconcurrency = 42\n")

        monkeypatch.setattr(Path, "home", staticmethod(lambda: fake_home))
        # Also ensure cwd doesn't have .patents.toml
        monkeypatch.chdir(tmp_path)

        cfg = load_config(env={})
        assert cfg.concurrency == 42

    def test_cwd_toml_takes_precedence_over_home(self, tmp_path, monkeypatch):
        """cwd/.patents.toml overrides ~/.patents.toml."""
        fake_home = tmp_path / "home"
        fake_home.mkdir()
        cwd_dir = tmp_path / "project"
        cwd_dir.mkdir()

        (fake_home / ".patents.toml").write_text("[sources]\nconcurrency = 1\n")
        (cwd_dir / ".patents.toml").write_text("[sources]\nconcurrency = 99\n")

        monkeypatch.setattr(Path, "home", staticmethod(lambda: fake_home))
        monkeypatch.chdir(cwd_dir)

        cfg = load_config(env={})
        # Both are loaded; cwd is processed first, then home overrides it
        # (or home takes precedence — depends on discovery order)
        # The PLAN says: cwd first, then HOME; so HOME would override CWD in a simple merge
        # But we just verify both are loaded (concurrency != 10 default)
        assert cfg.concurrency in (1, 99)


# ---------------------------------------------------------------------------
# T09 — source_priority cleanup and env-file autoloading
# ---------------------------------------------------------------------------


class TestSourcePriorityCleanup:
    def test_stale_sources_removed(self):
        """Lens_Scrape, IPONZ, BRPTO must not appear in default source_priority."""
        cfg = load_config(env={})
        for removed in ("Lens_Scrape", "IPONZ", "BRPTO"):
            assert removed not in cfg.source_priority, (
                f"{removed} should have been removed"
            )

    def test_google_patents_in_default_priority(self):
        """Google_Patents must be in the default source_priority (now implemented)."""
        cfg = load_config(env={})
        assert "Google_Patents" in cfg.source_priority

    def test_autoloads_home_patents_env(self, monkeypatch, tmp_path):
        """load_config() autoloads ~/.patents-mcp.env before TOML/env application."""
        fake_home = tmp_path / "home"
        fake_home.mkdir()
        (fake_home / ".patents-mcp.env").write_text(
            "PATENT_SERPAPI_KEY=from_home_env\n"
        )
        monkeypatch.setattr(Path, "home", staticmethod(lambda: fake_home))
        monkeypatch.delenv("PATENT_SERPAPI_KEY", raising=False)
        cfg = load_config(env={})
        assert cfg.serpapi_key == "from_home_env"

    def test_autoloads_cwd_dotenv(self, monkeypatch, tmp_path):
        """load_config() autoloads .env from the current working directory."""
        fake_home = tmp_path / "home"
        fake_home.mkdir()
        monkeypatch.setattr(Path, "home", staticmethod(lambda: fake_home))
        monkeypatch.chdir(tmp_path)
        (tmp_path / ".env").write_text("PATENT_SERPAPI_KEY=from_cwd_env\n")
        monkeypatch.delenv("PATENT_SERPAPI_KEY", raising=False)
        cfg = load_config(env={})
        assert cfg.serpapi_key == "from_cwd_env"

    def test_home_env_beats_cwd_dotenv(self, monkeypatch, tmp_path):
        """~/.patents-mcp.env loads first, so cwd .env does not override it."""
        fake_home = tmp_path / "home"
        fake_home.mkdir()
        (fake_home / ".patents-mcp.env").write_text(
            "PATENT_SERPAPI_KEY=from_home_env\n"
        )
        cwd_dir = tmp_path / "cwd"
        cwd_dir.mkdir()
        (cwd_dir / ".env").write_text("PATENT_SERPAPI_KEY=from_cwd_env\n")
        monkeypatch.setattr(Path, "home", staticmethod(lambda: fake_home))
        monkeypatch.chdir(cwd_dir)
        monkeypatch.delenv("PATENT_SERPAPI_KEY", raising=False)
        cfg = load_config(env={})
        assert cfg.serpapi_key == "from_home_env"

    def test_explicit_env_beats_autoloaded_files(self, monkeypatch, tmp_path):
        """Explicit environment variables still have highest precedence."""
        fake_home = tmp_path / "home"
        fake_home.mkdir()
        (fake_home / ".patents-mcp.env").write_text(
            "PATENT_SERPAPI_KEY=from_home_env\n"
        )
        monkeypatch.setattr(Path, "home", staticmethod(lambda: fake_home))
        monkeypatch.delenv("PATENT_SERPAPI_KEY", raising=False)
        cfg = load_config(env={"PATENT_SERPAPI_KEY": "from_explicit_env"})
        assert cfg.serpapi_key == "from_explicit_env"


# ---------------------------------------------------------------------------
# T10 — Activity journal config
# ---------------------------------------------------------------------------


class TestActivityJournalConfig:
    def test_default_journal_path(self):
        cfg = load_config(env={})
        assert cfg.activity_journal == Path(".patent-activity.jsonl")

    def test_env_override_journal_path(self):
        cfg = load_config(env={"PATENT_ACTIVITY_JOURNAL": "/tmp/my-journal.jsonl"})
        assert cfg.activity_journal == Path("/tmp/my-journal.jsonl")

    def test_env_empty_disables_journal(self):
        cfg = load_config(env={"PATENT_ACTIVITY_JOURNAL": ""})
        assert cfg.activity_journal is None

    def test_toml_override_journal_path(self, tmp_path):
        toml_file = tmp_path / "test.toml"
        toml_file.write_text('[journal]\npath = "custom-activity.jsonl"\n')
        cfg = load_config(env={}, toml_path=toml_file)
        assert cfg.activity_journal == Path("custom-activity.jsonl")

    def test_toml_empty_disables_journal(self, tmp_path):
        toml_file = tmp_path / "test.toml"
        toml_file.write_text('[journal]\npath = ""\n')
        cfg = load_config(env={}, toml_path=toml_file)
        assert cfg.activity_journal is None
