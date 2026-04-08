"""T17 — Cache schema parity: Python and Rust must use identical SQLite schema.

Verifies that both implementations create the same tables, columns, and indexes
so patent data written by one can be read by the other.
"""
from __future__ import annotations

import sqlite3
import subprocess
import tempfile
from pathlib import Path

import pytest

RUST_DIR = Path(__file__).parent.parent.parent / "src" / "rust"
RUST_BIN = RUST_DIR / "target" / "debug" / "patent-mcp-server"


def _extract_schema(db_path: Path) -> dict[str, str]:
    """Return {table_name: create_sql} from sqlite_master, normalized."""
    con = sqlite3.connect(str(db_path))
    rows = con.execute(
        "SELECT name, sql FROM sqlite_master WHERE type='table' ORDER BY name"
    ).fetchall()
    con.close()
    return {
        name: " ".join(sql.split())  # normalize whitespace
        for name, sql in rows
        if name and not name.startswith("sqlite_")
    }


def _extract_index_names(db_path: Path) -> set[str]:
    con = sqlite3.connect(str(db_path))
    rows = con.execute(
        "SELECT name FROM sqlite_master WHERE type='index' ORDER BY name"
    ).fetchall()
    con.close()
    return {r[0] for r in rows if not r[0].startswith("sqlite_")}


@pytest.fixture(scope="module")
def python_cache_db(tmp_path_factory):
    """Initialize a Python cache DB and return its path."""
    d = tmp_path_factory.mktemp("py_cache")
    cache_dir = d / ".patents"
    cache_dir.mkdir()

    import sys
    import os
    src_python = str(Path(__file__).parent.parent.parent / "src" / "python")
    env = dict(os.environ, PYTHONPATH=src_python)

    # Run Python to initialize the cache DB
    code = (
        f"import sys; sys.path.insert(0, {repr(src_python)}); "
        f"from patent_mcp.config import load_config; "
        f"from patent_mcp.cache import PatentCache; "
        f"from pathlib import Path; "
        f"cfg = load_config(overrides={{'cache_local_dir': Path({repr(str(cache_dir))})}}); "
        f"PatentCache(cfg)"
    )
    result = subprocess.run(
        [sys.executable, "-c", code],
        capture_output=True, text=True, env=env
    )
    assert result.returncode == 0, f"Python cache init failed: {result.stderr}"
    return cache_dir / "index.db"


@pytest.fixture(scope="module")
def rust_cache_db(tmp_path_factory, rust_binary):
    """Initialize a Rust cache DB by running a dummy canonicalize call."""
    d = tmp_path_factory.mktemp("rs_cache")
    cache_dir = d / ".patents"
    cache_dir.mkdir()

    # Run the Rust binary to just initialize the server config + cache
    # Use the "canonicalize" subcommand (lightweight, no network) with --cache-dir
    result = subprocess.run(
        [rust_binary, "--cache-dir", str(cache_dir), "canonicalize", "US7654321"],
        capture_output=True, text=True, timeout=10
    )
    assert result.returncode == 0, f"Rust canonicalize failed: {result.stderr}"

    # Cache DB is created at cache_dir/index.db by PatentCache::new() in run_server,
    # but canonicalize subcommand bypasses that. We need to init the cache directly.
    # Run the server for just a split second to init the DB via initialize message.
    import json
    init_msg = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}})
    proc = subprocess.Popen(
        [rust_binary, "--cache-dir", str(cache_dir)],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    stdout, _ = proc.communicate(input=init_msg + "\n", timeout=10)
    assert '"protocolVersion"' in stdout, f"Rust server bad response: {stdout}"

    db = cache_dir / "index.db"
    assert db.exists(), f"Rust cache DB not created at {db}"
    return db


def test_cache_tables_match(python_cache_db, rust_cache_db):
    """Both implementations must create the same tables."""
    py_tables = set(_extract_schema(python_cache_db).keys())
    rs_tables = set(_extract_schema(rust_cache_db).keys())
    assert py_tables == rs_tables, (
        f"Table mismatch:\n  Python only: {py_tables - rs_tables}\n"
        f"  Rust only: {rs_tables - py_tables}"
    )


def test_cache_patents_table_columns_match(python_cache_db, rust_cache_db):
    """The patents table must have the same columns in both implementations."""
    def get_columns(db_path):
        con = sqlite3.connect(str(db_path))
        cols = {row[1] for row in con.execute("PRAGMA table_info(patents)").fetchall()}
        con.close()
        return cols

    py_cols = get_columns(python_cache_db)
    rs_cols = get_columns(rust_cache_db)
    assert py_cols == rs_cols, (
        f"Column mismatch in patents table:\n  Python only: {py_cols - rs_cols}\n"
        f"  Rust only: {rs_cols - py_cols}"
    )


def test_cache_indexes_match(python_cache_db, rust_cache_db):
    """Both implementations must create the same indexes."""
    py_idx = _extract_index_names(python_cache_db)
    rs_idx = _extract_index_names(rust_cache_db)
    assert py_idx == rs_idx, (
        f"Index mismatch:\n  Python only: {py_idx - rs_idx}\n"
        f"  Rust only: {rs_idx - py_idx}"
    )
