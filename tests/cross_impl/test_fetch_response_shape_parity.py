"""Cross-impl parity: fetch_patents response shape.

Starts both Python and Rust MCP servers, sends fetch_patents with an invalid
patent ID (no network required), and verifies both return the same field
structure in per-result entries and the summary object.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import time
from pathlib import Path
from typing import Any

import pytest

PYTHON_SRC = Path(__file__).parent.parent.parent / "src" / "python"

INVALID_PATENT_ID = "INVALID-XXXXX-NOTREAL"
EXPECTED_RESULT_FIELDS = {
    "patent_id",
    "canonical_id",
    "status",
    "files",
    "metadata",
    "fetch_duration_ms",
}
VALID_STATUSES = {"fetched", "cached", "partial", "error"}
SUMMARY_FIELDS = {"total", "success", "cached", "errors", "total_duration_ms"}

MCP_INIT_PARAMS = {
    "protocolVersion": "2024-11-05",
    "capabilities": {},
    "clientInfo": {"name": "parity-test", "version": "0.1"},
}


def _send_rpc(proc: subprocess.Popen, method: str, params: Any = None) -> dict:
    rpc_id = int(time.monotonic() * 1e6) & 0xFFFFFF
    req: dict = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
    if params is not None:
        req["params"] = params
    line = json.dumps(req) + "\n"
    proc.stdin.write(line)
    proc.stdin.flush()
    raw = proc.stdout.readline()
    assert raw, f"No response from server for method={method!r}"
    return json.loads(raw)


def _extract_payload(resp: dict) -> dict:
    content = resp["result"]["content"]
    assert content, "Empty content list"
    text = content[0]["text"]
    return json.loads(text)


def _start_server(cmd: list[str], cache_dir: str) -> subprocess.Popen:
    env = os.environ.copy()
    env["PATENT_CACHE_DIR"] = cache_dir
    env["PYTHONPATH"] = str(PYTHON_SRC) + os.pathsep + env.get("PYTHONPATH", "")
    return subprocess.Popen(
        cmd,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        text=True,
        env=env,
    )


def _fetch_invalid(proc: subprocess.Popen) -> dict:
    _send_rpc(proc, "initialize", MCP_INIT_PARAMS)
    resp = _send_rpc(
        proc,
        "tools/call",
        {"name": "fetch_patents", "arguments": {"patent_ids": [INVALID_PATENT_ID]}},
    )
    return _extract_payload(resp)


@pytest.fixture(scope="module")
def rust_fetch_server(rust_binary: str, tmp_path_factory: pytest.TempPathFactory):
    tmp = tmp_path_factory.mktemp("rust_fetch_shape")
    proc = _start_server([rust_binary], str(tmp / ".patents"))
    yield proc
    try:
        proc.stdin.close()
        proc.wait(timeout=5)
    except Exception:
        proc.kill()


@pytest.fixture(scope="module")
def python_fetch_server(tmp_path_factory: pytest.TempPathFactory):
    tmp = tmp_path_factory.mktemp("py_fetch_shape")
    proc = _start_server(
        [sys.executable, "-m", "patent_mcp"],
        str(tmp / ".patents"),
    )
    yield proc
    try:
        proc.stdin.close()
        proc.wait(timeout=5)
    except Exception:
        proc.kill()


def test_python_fetch_result_fields(python_fetch_server: subprocess.Popen) -> None:
    """Python fetch_patents result must include all expected fields."""
    payload = _fetch_invalid(python_fetch_server)
    assert payload["results"], "Expected at least one result"
    result = payload["results"][0]
    missing = EXPECTED_RESULT_FIELDS - set(result.keys())
    assert not missing, f"Python result missing fields: {missing}"
    assert result["status"] in VALID_STATUSES, f"Unexpected status: {result['status']}"
    assert result["patent_id"] == INVALID_PATENT_ID


def test_rust_fetch_result_fields(rust_fetch_server: subprocess.Popen) -> None:
    """Rust fetch_patents result must include all expected fields matching Python."""
    payload = _fetch_invalid(rust_fetch_server)
    assert payload["results"], "Expected at least one result"
    result = payload["results"][0]
    missing = EXPECTED_RESULT_FIELDS - set(result.keys())
    assert not missing, f"Rust result missing fields: {missing}"
    assert result["status"] in VALID_STATUSES, f"Unexpected status: {result['status']}"
    assert result["patent_id"] == INVALID_PATENT_ID


def test_fetch_result_field_overlap(
    rust_fetch_server: subprocess.Popen, python_fetch_server: subprocess.Popen
) -> None:
    """Both implementations must return the same set of per-result fields."""
    rust_payload = _fetch_invalid(rust_fetch_server)
    py_payload = _fetch_invalid(python_fetch_server)

    rust_fields = set(rust_payload["results"][0].keys())
    py_fields = set(py_payload["results"][0].keys())

    rust_only = rust_fields - py_fields
    py_only = py_fields - rust_fields
    assert not rust_only, f"Fields only in Rust: {rust_only}"
    assert not py_only, f"Fields only in Python: {py_only}"


def test_fetch_summary_fields_match(
    rust_fetch_server: subprocess.Popen, python_fetch_server: subprocess.Popen
) -> None:
    """Both implementations must return the same summary structure."""
    rust_payload = _fetch_invalid(rust_fetch_server)
    py_payload = _fetch_invalid(python_fetch_server)

    rust_summary_keys = set(rust_payload["summary"].keys())
    py_summary_keys = set(py_payload["summary"].keys())
    assert SUMMARY_FIELDS <= rust_summary_keys, (
        f"Rust summary missing keys: {SUMMARY_FIELDS - rust_summary_keys}"
    )
    assert SUMMARY_FIELDS <= py_summary_keys, (
        f"Python summary missing keys: {SUMMARY_FIELDS - py_summary_keys}"
    )

    assert rust_payload["summary"]["total"] == 1
    assert py_payload["summary"]["total"] == 1
    assert rust_payload["summary"]["errors"] == 1
    assert py_payload["summary"]["errors"] == 1


def test_rust_fetch_empty_batch_shape(rust_fetch_server: subprocess.Popen) -> None:
    """Rust empty-batch response shape must match Python's."""
    _send_rpc(rust_fetch_server, "initialize", MCP_INIT_PARAMS)
    resp = _send_rpc(
        rust_fetch_server,
        "tools/call",
        {"name": "fetch_patents", "arguments": {"patent_ids": []}},
    )
    payload = _extract_payload(resp)
    assert payload["results"] == []
    assert SUMMARY_FIELDS <= set(payload["summary"].keys())
    assert payload["summary"]["total"] == 0
