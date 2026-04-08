"""Tests for patent_mcp.cache — T01-T11."""
from __future__ import annotations

import asyncio
import json
import time
from datetime import datetime, timedelta, timezone
from pathlib import Path

import pytest

from patent_mcp.cache import (
    ArtifactSet,
    CacheResult,
    PatentCache,
    PatentMetadata,
    SessionCache,
    SourceAttempt,
)
from patent_mcp.config import PatentConfig, load_config


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

def _make_config(tmp_path: Path) -> PatentConfig:
    cfg = load_config(env={})
    cfg.cache_local_dir = tmp_path / "local" / ".patents"
    cfg.cache_global_db = tmp_path / "global" / "index.db"
    return cfg


def _make_meta(canonical_id: str = "US7654321") -> PatentMetadata:
    return PatentMetadata(
        canonical_id=canonical_id,
        jurisdiction="US",
        doc_type="patent",
        title="Widget assembly",
        inventors=["Alice", "Bob"],
        fetched_at="2026-01-01T00:00:00+00:00",
    )


def _write_file(path: Path, content: str = "data") -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content)
    return path


# ---------------------------------------------------------------------------
# T01 — Schema creation
# ---------------------------------------------------------------------------

class TestSchemaCreated:
    def test_tables_exist(self, tmp_path):
        import sqlite3
        cfg = _make_config(tmp_path)
        cache = PatentCache(cfg)
        with sqlite3.connect(str(cfg.cache_local_dir / "index.db")) as conn:
            tables = {
                row[0]
                for row in conn.execute(
                    "SELECT name FROM sqlite_master WHERE type='table'"
                ).fetchall()
            }
        assert "patents" in tables
        assert "patent_locations" in tables
        assert "cache_registrations" in tables
        assert "fetch_sources" in tables

    def test_wal_mode(self, tmp_path):
        import sqlite3
        cfg = _make_config(tmp_path)
        PatentCache(cfg)
        with sqlite3.connect(str(cfg.cache_local_dir / "index.db")) as conn:
            mode = conn.execute("PRAGMA journal_mode").fetchone()[0]
        assert mode == "wal"

    def test_global_db_also_created(self, tmp_path):
        cfg = _make_config(tmp_path)
        PatentCache(cfg)
        assert cfg.cache_global_db.exists()

    def test_self_registration_in_global_db(self, tmp_path):
        import sqlite3
        cfg = _make_config(tmp_path)
        PatentCache(cfg)
        with sqlite3.connect(str(cfg.cache_global_db)) as conn:
            rows = conn.execute("SELECT * FROM cache_registrations").fetchall()
        assert len(rows) >= 1


# ---------------------------------------------------------------------------
# T02 — Cache miss
# ---------------------------------------------------------------------------

class TestCacheMiss:
    def test_cache_miss_returns_none(self, tmp_path):
        cfg = _make_config(tmp_path)
        cache = PatentCache(cfg)
        assert cache.lookup("US7654321") is None

    def test_cache_miss_different_id(self, tmp_path):
        cfg = _make_config(tmp_path)
        cache = PatentCache(cfg)
        assert cache.lookup("EP1234567") is None


# ---------------------------------------------------------------------------
# T03 — Store and retrieve artifacts
# ---------------------------------------------------------------------------

class TestStoreAndLookup:
    def test_store_and_lookup_pdf(self, tmp_path):
        cfg = _make_config(tmp_path)
        cache = PatentCache(cfg)
        pdf = _write_file(tmp_path / "src" / "US7654321.pdf", "%PDF-1.4")
        artifacts = ArtifactSet(pdf=pdf)
        cache.store("US7654321", artifacts, _make_meta("US7654321"))
        result = cache.lookup("US7654321")
        assert result is not None
        assert "pdf" in result.files
        assert result.files["pdf"].exists()

    def test_store_and_lookup_all_formats(self, tmp_path):
        cfg = _make_config(tmp_path)
        cache = PatentCache(cfg)
        src = tmp_path / "src"
        artifacts = ArtifactSet(
            pdf=_write_file(src / "US7654321.pdf"),
            txt=_write_file(src / "US7654321.txt"),
            md=_write_file(src / "US7654321.md"),
            images=[
                _write_file(src / "fig1.png"),
                _write_file(src / "fig2.png"),
            ],
        )
        cache.store("US7654321", artifacts, _make_meta("US7654321"))
        result = cache.lookup("US7654321")
        assert result is not None
        assert "pdf" in result.files
        assert "txt" in result.files
        assert "md" in result.files
        assert "image_0" in result.files
        assert "image_1" in result.files

    def test_store_idempotent(self, tmp_path):
        """Storing the same patent twice overwrites; no duplicates."""
        cfg = _make_config(tmp_path)
        cache = PatentCache(cfg)
        pdf = _write_file(tmp_path / "src" / "US7654321.pdf")
        artifacts = ArtifactSet(pdf=pdf)
        cache.store("US7654321", artifacts, _make_meta())
        cache.store("US7654321", artifacts, _make_meta())
        result = cache.lookup("US7654321")
        assert result is not None
        assert len(result.files) == 1

    def test_store_copies_files_to_cache_dir(self, tmp_path):
        """Files should be in the cache dir, not the src dir."""
        cfg = _make_config(tmp_path)
        cache = PatentCache(cfg)
        pdf = _write_file(tmp_path / "src" / "US7654321.pdf")
        cache.store("US7654321", ArtifactSet(pdf=pdf), _make_meta())
        result = cache.lookup("US7654321")
        assert result is not None
        cached_pdf = result.files["pdf"]
        # Should be under the cache dir, not the original src path
        assert str(cached_pdf).startswith(str(cfg.cache_local_dir))


# ---------------------------------------------------------------------------
# T04 — Metadata stored and returned
# ---------------------------------------------------------------------------

class TestMetadata:
    def test_metadata_stored_and_returned(self, tmp_path):
        cfg = _make_config(tmp_path)
        cache = PatentCache(cfg)
        pdf = _write_file(tmp_path / "src" / "US7654321.pdf")
        meta = _make_meta()
        cache.store("US7654321", ArtifactSet(pdf=pdf), meta)
        result = cache.lookup("US7654321")
        assert result is not None
        assert result.metadata.title == "Widget assembly"
        assert result.metadata.inventors == ["Alice", "Bob"]
        assert result.metadata.jurisdiction == "US"
        assert result.metadata.doc_type == "patent"

    def test_metadata_canonical_id_correct(self, tmp_path):
        cfg = _make_config(tmp_path)
        cache = PatentCache(cfg)
        pdf = _write_file(tmp_path / "src" / "EP1234567.pdf")
        meta = _make_meta("EP1234567")
        meta.jurisdiction = "EP"
        cache.store("EP1234567", ArtifactSet(pdf=pdf), meta)
        result = cache.lookup("EP1234567")
        assert result is not None
        assert result.canonical_id == "EP1234567"
        assert result.metadata.canonical_id == "EP1234567"


# ---------------------------------------------------------------------------
# T05 — Stale file detection
# ---------------------------------------------------------------------------

class TestStaleFile:
    def test_stale_file_returns_incomplete(self, tmp_path):
        """If file was deleted, is_complete=False."""
        cfg = _make_config(tmp_path)
        cache = PatentCache(cfg)
        pdf = _write_file(tmp_path / "src" / "US7654321.pdf")
        cache.store("US7654321", ArtifactSet(pdf=pdf), _make_meta())

        # Delete the cached file
        result = cache.lookup("US7654321")
        assert result is not None
        result.files["pdf"].unlink()

        # Lookup again — is_complete should be False or result is None
        result2 = cache.lookup("US7654321")
        if result2 is not None:
            assert result2.is_complete is False
        # Returning None is also acceptable behavior (all files missing)

    def test_is_complete_true_when_all_exist(self, tmp_path):
        cfg = _make_config(tmp_path)
        cache = PatentCache(cfg)
        pdf = _write_file(tmp_path / "src" / "US7654321.pdf")
        cache.store("US7654321", ArtifactSet(pdf=pdf), _make_meta())
        result = cache.lookup("US7654321")
        assert result is not None
        assert result.is_complete is True


# ---------------------------------------------------------------------------
# T06 — Cache registration
# ---------------------------------------------------------------------------

class TestCacheRegistration:
    def test_register_cache_dir(self, tmp_path):
        import sqlite3
        cfg = _make_config(tmp_path)
        cache = PatentCache(cfg)
        external_dir = tmp_path / "other" / ".patents"
        cache.register_cache_dir(external_dir)
        with sqlite3.connect(str(cfg.cache_global_db)) as conn:
            rows = conn.execute(
                "SELECT cache_dir FROM cache_registrations WHERE cache_dir=?",
                (str(external_dir.resolve()),),
            ).fetchall()
        assert len(rows) == 1

    def test_register_idempotent(self, tmp_path):
        """Registering same dir twice should not raise or duplicate."""
        import sqlite3
        cfg = _make_config(tmp_path)
        cache = PatentCache(cfg)
        ext_dir = tmp_path / "ext" / ".patents"
        cache.register_cache_dir(ext_dir)
        cache.register_cache_dir(ext_dir)
        with sqlite3.connect(str(cfg.cache_global_db)) as conn:
            rows = conn.execute(
                "SELECT cache_dir FROM cache_registrations WHERE cache_dir=?",
                (str(ext_dir.resolve()),),
            ).fetchall()
        assert len(rows) == 1


# ---------------------------------------------------------------------------
# T07 — list_all
# ---------------------------------------------------------------------------

class TestListAll:
    def test_list_all_empty(self, tmp_path):
        cfg = _make_config(tmp_path)
        cache = PatentCache(cfg)
        assert cache.list_all() == []

    def test_list_all_two_patents(self, tmp_path):
        cfg = _make_config(tmp_path)
        cache = PatentCache(cfg)
        src = tmp_path / "src"
        cache.store("US7654321", ArtifactSet(pdf=_write_file(src / "a.pdf")), _make_meta("US7654321"))
        cache.store("EP1234567", ArtifactSet(pdf=_write_file(src / "b.pdf")), _make_meta("EP1234567"))
        entries = cache.list_all()
        ids = {e.canonical_id for e in entries}
        assert "US7654321" in ids
        assert "EP1234567" in ids
        assert len(entries) == 2


# ---------------------------------------------------------------------------
# T08 — SessionCache
# ---------------------------------------------------------------------------

class TestSessionCache:
    def test_cache_miss_returns_none(self):
        sc = SessionCache()
        assert sc.get("PPUBS") is None

    def test_store_and_get(self):
        sc = SessionCache()
        sc.set("PPUBS", "mytoken", ttl_minutes=30)
        assert sc.get("PPUBS") == "mytoken"

    def test_expired_returns_none(self):
        sc = SessionCache()
        # TTL=0 means expires immediately; use a negative timedelta trick via set_with_expiry
        past = datetime.now(timezone.utc) - timedelta(seconds=1)
        sc.set_with_expiry("PPUBS", "oldtoken", expires_at=past)
        assert sc.get("PPUBS") is None

    def test_set_with_expiry_future(self):
        sc = SessionCache()
        future = datetime.now(timezone.utc) + timedelta(hours=1)
        sc.set_with_expiry("EPO_OPS", "epotoken", expires_at=future)
        assert sc.get("EPO_OPS") == "epotoken"

    def test_set_with_expiry_past(self):
        sc = SessionCache()
        past = datetime.now(timezone.utc) - timedelta(seconds=1)
        sc.set_with_expiry("EPO_OPS", "oldtoken", expires_at=past)
        assert sc.get("EPO_OPS") is None

    def test_invalidate(self):
        sc = SessionCache()
        sc.set("PPUBS", "mytoken", ttl_minutes=30)
        sc.invalidate("PPUBS")
        assert sc.get("PPUBS") is None

    def test_invalidate_missing_key_no_error(self):
        sc = SessionCache()
        sc.invalidate("NONEXISTENT")  # Should not raise

    def test_multiple_sources_independent(self):
        sc = SessionCache()
        sc.set("PPUBS", "token1", ttl_minutes=30)
        sc.set("EPO_OPS", "token2", ttl_minutes=30)
        assert sc.get("PPUBS") == "token1"
        assert sc.get("EPO_OPS") == "token2"
        sc.invalidate("PPUBS")
        assert sc.get("PPUBS") is None
        assert sc.get("EPO_OPS") == "token2"


# ---------------------------------------------------------------------------
# T09 — Concurrent writes (WAL mode)
# ---------------------------------------------------------------------------

class TestConcurrentWrites:
    def test_concurrent_stores(self, tmp_path):
        """10 concurrent store operations should all succeed."""
        cfg = _make_config(tmp_path)
        cache = PatentCache(cfg)
        src = tmp_path / "src"
        src.mkdir()

        ids = [f"US{7000000 + i}" for i in range(10)]
        for pid in ids:
            _write_file(src / f"{pid}.pdf")

        async def store_one(pid: str) -> None:
            pdf = src / f"{pid}.pdf"
            meta = PatentMetadata(canonical_id=pid, jurisdiction="US", doc_type="patent",
                                  fetched_at="2026-01-01T00:00:00+00:00")
            cache.store(pid, ArtifactSet(pdf=pdf), meta)

        async def run_all():
            await asyncio.gather(*[store_one(pid) for pid in ids])

        asyncio.run(run_all())

        entries = cache.list_all()
        assert len(entries) == 10


# ---------------------------------------------------------------------------
# T10 — sources.json written
# ---------------------------------------------------------------------------

class TestSourcesJson:
    def test_sources_json_written(self, tmp_path):
        cfg = _make_config(tmp_path)
        cache = PatentCache(cfg)
        pdf = _write_file(tmp_path / "src" / "US7654321.pdf")
        sources = [
            SourceAttempt(source="USPTO", success=True, elapsed_ms=123.4),
            SourceAttempt(source="EPO_OPS", success=False, elapsed_ms=50.0, error="timeout"),
        ]
        cache.store("US7654321", ArtifactSet(pdf=pdf), _make_meta(), fetch_sources=sources)

        sources_file = cfg.cache_local_dir / "US7654321" / "sources.json"
        assert sources_file.exists()
        data = json.loads(sources_file.read_text())
        assert any(s["source"] == "USPTO" for s in data)

    def test_sources_json_not_written_when_no_sources(self, tmp_path):
        cfg = _make_config(tmp_path)
        cache = PatentCache(cfg)
        pdf = _write_file(tmp_path / "src" / "US7654321.pdf")
        cache.store("US7654321", ArtifactSet(pdf=pdf), _make_meta())
        sources_file = cfg.cache_local_dir / "US7654321" / "sources.json"
        assert not sources_file.exists()


# ---------------------------------------------------------------------------
# T11 — metadata.json written
# ---------------------------------------------------------------------------

class TestMetadataJson:
    def test_metadata_json_written(self, tmp_path):
        cfg = _make_config(tmp_path)
        cache = PatentCache(cfg)
        pdf = _write_file(tmp_path / "src" / "US7654321.pdf")
        cache.store("US7654321", ArtifactSet(pdf=pdf), _make_meta())

        meta_file = cfg.cache_local_dir / "US7654321" / "metadata.json"
        assert meta_file.exists()
        data = json.loads(meta_file.read_text())
        assert data["canonical_id"] == "US7654321"
        assert data["jurisdiction"] == "US"
        assert "title" in data

    def test_metadata_json_inventors_is_list(self, tmp_path):
        cfg = _make_config(tmp_path)
        cache = PatentCache(cfg)
        pdf = _write_file(tmp_path / "src" / "US7654321.pdf")
        meta = _make_meta()
        meta.inventors = ["Alice", "Bob"]
        cache.store("US7654321", ArtifactSet(pdf=pdf), meta)
        meta_file = cfg.cache_local_dir / "US7654321" / "metadata.json"
        data = json.loads(meta_file.read_text())
        assert isinstance(data["inventors"], list)
        assert "Alice" in data["inventors"]


# ---------------------------------------------------------------------------
# T12 — Robustness: corrupted data
# ---------------------------------------------------------------------------

class TestRobustness:
    def test_corrupted_inventors_json_returns_empty_list(self, tmp_path):
        """A corrupted inventors field in the DB should return [] not raise."""
        import sqlite3 as _sqlite3
        from patent_mcp.cache import _safe_json_list

        # Direct unit test of the helper
        assert _safe_json_list(None) == []
        assert _safe_json_list("") == []
        assert _safe_json_list("[]") == []
        assert _safe_json_list('["Alice","Bob"]') == ["Alice", "Bob"]
        assert _safe_json_list("INVALID{NOT JSON}") == []
        assert _safe_json_list('"just_a_string"') == []  # string not list

    def test_corrupted_inventors_in_db_survives_lookup(self, tmp_path):
        """Manually corrupt inventors in DB; lookup should still return result."""
        import sqlite3 as _sqlite3
        cfg = _make_config(tmp_path)
        cache = PatentCache(cfg)
        pdf = _write_file(tmp_path / "src" / "US7654321.pdf")
        cache.store("US7654321", ArtifactSet(pdf=pdf), _make_meta())

        # Corrupt the inventors column in the LOCAL db (which lookup() uses)
        local_db = cfg.cache_local_dir / "index.db"
        with _sqlite3.connect(str(local_db)) as conn:
            conn.execute(
                "UPDATE patents SET inventors = ? WHERE canonical_id = ?",
                ("NOT VALID JSON", "US7654321"),
            )

        # Lookup should return the patent with empty inventors (not crash)
        result = cache.lookup("US7654321")
        assert result is not None
        assert result.metadata is not None
        assert result.metadata.inventors == []

    def test_same_file_store_no_error(self, tmp_path):
        """Storing a file that's already in its destination should not crash."""
        cfg = _make_config(tmp_path)
        cache = PatentCache(cfg)
        # Write PDF directly into the cache dir (simulating source writing to cache dir)
        dest_dir = cfg.cache_local_dir / "US7654321"
        dest_dir.mkdir(parents=True, exist_ok=True)
        pdf = dest_dir / "US7654321.pdf"
        pdf.write_text("%PDF-1.4")

        # Store should not raise "same file" error
        cache.store("US7654321", ArtifactSet(pdf=pdf), _make_meta())
        result = cache.lookup("US7654321")
        assert result is not None
