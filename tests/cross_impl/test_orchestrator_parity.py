"""T18 — Orchestrator parity: cache written by Python must be readable by Rust.

The primary parity test for the fetcher orchestrator is cross-cache interop:
both implementations use identical SQLite schema, so a cache entry written by
Python should be returned by the Rust server as a cache hit with identical data.

Full T18 (fetching from mock sources) requires the mock HTTP server (T08-T11)
which is not yet implemented. This test covers the cache-hit path which
validates the most critical parity property: once fetched, data is identical.
"""
from __future__ import annotations

import json
import os
import sqlite3
import subprocess
import sys
import tempfile
from pathlib import Path

import pytest

FIXTURES_DIR = Path(__file__).parent.parent / "fixtures"
RUST_DIR = Path(__file__).parent.parent.parent / "src" / "rust"
RUST_BIN = RUST_DIR / "target" / "debug" / "patent-mcp-server"
SRC_PYTHON = Path(__file__).parent.parent.parent / "src" / "python"


def _normalize_metadata(d: dict | None) -> dict | None:
    """Strip volatile fields (timestamps, paths) for comparison."""
    if d is None:
        return None
    drop = {"fetched_at", "status_fetched_at", "cache_dir"}
    return {k: v for k, v in sorted(d.items()) if k not in drop}


@pytest.fixture(scope="module")
def rust_binary(tmp_path_factory):
    result = subprocess.run(
        ["cargo", "build", "--manifest-path", str(RUST_DIR / "Cargo.toml")],
        capture_output=True, text=True
    )
    assert result.returncode == 0, f"Rust build failed: {result.stderr}"
    return str(RUST_BIN)


@pytest.fixture
def shared_cache_dir(tmp_path):
    """Temp dir used as the shared cache by both Python and Rust."""
    d = tmp_path / ".patents"
    d.mkdir()
    return d


def _python_populate_cache(cache_dir: Path, canonical_id: str) -> dict:
    """Use Python PatentCache to store a synthetic patent; return stored metadata."""
    env = dict(os.environ, PYTHONPATH=str(SRC_PYTHON))
    code = f"""
import sys, json
sys.path.insert(0, {repr(str(SRC_PYTHON))})
from pathlib import Path
from patent_mcp.config import load_config
from patent_mcp.cache import PatentCache, PatentMetadata, ArtifactSet

cfg = load_config(overrides={{"cache_local_dir": Path({repr(str(cache_dir))})}})
cache = PatentCache(cfg)

pdf = Path({repr(str(cache_dir))}) / "stub.pdf"
pdf.write_bytes(b"%PDF-1.4 stub")

meta = PatentMetadata(
    canonical_id={repr(canonical_id)},
    jurisdiction={repr(canonical_id[:2])},
    doc_type="patent",
    title="Cross-Impl Test Patent",
    abstract="Synthetic patent for cross-impl cache parity testing.",
    inventors=["Alice Test", "Bob Test"],
    assignee="Test Corp",
    filing_date="2020-01-15",
    publication_date="2022-06-01",
    grant_date=None,
)

artifacts = ArtifactSet(pdf=pdf)
cache.store({repr(canonical_id)}, artifacts, meta)

import dataclasses
print(json.dumps(dataclasses.asdict(meta)))
"""
    result = subprocess.run(
        [sys.executable, "-c", code],
        capture_output=True, text=True, env=env
    )
    assert result.returncode == 0, f"Python populate failed: {result.stderr}"
    # Get the last JSON line
    lines = [l for l in result.stdout.splitlines() if l.strip().startswith('{')]
    assert lines, f"No JSON output: {result.stdout!r}"
    return json.loads(lines[-1])


def _rust_mcp_fetch(rust_binary: str, cache_dir: Path, canonical_id: str) -> dict:
    """Send fetch_patents MCP call to Rust server; return parsed result."""
    msg = json.dumps({
        "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}
    }) + "\n"
    msg += json.dumps({
        "jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": {
            "name": "fetch_patents",
            "arguments": {"patent_ids": [canonical_id]}
        }
    }) + "\n"

    proc = subprocess.Popen(
        [rust_binary, "--cache-dir", str(cache_dir)],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        text=True
    )
    stdout, _ = proc.communicate(input=msg, timeout=15)

    responses = [json.loads(l) for l in stdout.splitlines() if l.strip()]
    # Find the tools/call response (id=2)
    fetch_resp = next((r for r in responses if r.get("id") == 2), None)
    assert fetch_resp is not None, f"No tools/call response in: {stdout}"
    assert "result" in fetch_resp, f"Error in fetch response: {fetch_resp}"

    content = fetch_resp["result"]["content"][0]["text"]
    return json.loads(content)


class TestCacheInterop:
    def test_rust_reads_python_written_cache(self, rust_binary, shared_cache_dir):
        """Python writes a patent; Rust server returns it as a cache hit."""
        canonical_id = "US7654321"
        py_meta = _python_populate_cache(shared_cache_dir, canonical_id)

        rust_result = _rust_mcp_fetch(rust_binary, shared_cache_dir, canonical_id)

        assert rust_result["summary"]["total"] == 1
        assert rust_result["summary"]["success"] == 1
        assert rust_result["summary"]["cached"] == 1, "Expected cache hit"

        results = rust_result["results"]
        assert len(results) == 1
        r = results[0]
        assert r["canonical_id"] == canonical_id
        assert r["success"] is True
        assert r["from_cache"] is True

    def test_rust_metadata_matches_python_written(self, rust_binary, shared_cache_dir):
        """Metadata returned by Rust must match what Python stored."""
        canonical_id = "EP1234567"
        py_meta = _python_populate_cache(shared_cache_dir, canonical_id)

        rust_result = _rust_mcp_fetch(rust_binary, shared_cache_dir, canonical_id)
        results = rust_result["results"]
        assert results, "No results from Rust"

        rust_meta = results[0].get("metadata")
        assert rust_meta is not None, "Rust returned no metadata"

        py_norm = _normalize_metadata(py_meta)
        rs_norm = _normalize_metadata(rust_meta)

        # Key fields must match
        for field in ["canonical_id", "jurisdiction", "doc_type", "title",
                      "inventors", "assignee", "filing_date", "publication_date"]:
            assert py_norm.get(field) == rs_norm.get(field), (
                f"Field '{field}' mismatch:\n  Python: {py_norm.get(field)}\n"
                f"  Rust:   {rs_norm.get(field)}"
            )

    def test_rust_miss_for_unknown_id(self, rust_binary, shared_cache_dir):
        """Rust server returns error (not a crash) for unknown patent ID."""
        rust_result = _rust_mcp_fetch(rust_binary, shared_cache_dir, "US9999998")

        assert rust_result["summary"]["total"] == 1
        # May fail (no Python subprocess) but should not crash
        results = rust_result["results"]
        assert len(results) == 1
        # success=False is acceptable for a cache miss without network
        r = results[0]
        assert "canonical_id" in r
