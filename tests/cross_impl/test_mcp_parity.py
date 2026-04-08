"""T19 (partial) — Cross-impl MCP protocol parity.

Starts both Python and Rust servers as subprocesses, sends JSON-RPC messages,
and compares protocol version, tool names, and empty-batch response structure.
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

# Proper MCP initialize params (required by FastMCP-based Python server)
MCP_INIT_PARAMS = {
    "protocolVersion": "2024-11-05",
    "capabilities": {},
    "clientInfo": {"name": "parity-test", "version": "0.1"},
}


def send_rpc(proc: subprocess.Popen, method: str, params: Any = None) -> dict:
    """Write a JSON-RPC request to proc.stdin and read one response line from stdout."""
    rpc_id = int(time.monotonic() * 1e6) & 0xFFFFFF
    req: dict = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
    if params is not None:
        req["params"] = params
    line = json.dumps(req) + "\n"
    proc.stdin.write(line)  # type: ignore[union-attr]
    proc.stdin.flush()  # type: ignore[union-attr]
    raw = proc.stdout.readline()  # type: ignore[union-attr]
    assert raw, f"No response from server for method={method!r}"
    return json.loads(raw)


def start_server(cmd: list[str], cache_dir: str) -> subprocess.Popen:
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


@pytest.fixture(scope="module")
def rust_server(rust_binary: str, tmp_path_factory: pytest.TempPathFactory):
    """Start the Rust MCP server subprocess."""
    tmp = tmp_path_factory.mktemp("rust_cache")
    proc = start_server([rust_binary], str(tmp / ".patents"))
    yield proc
    try:
        proc.stdin.close()  # type: ignore[union-attr]
        proc.wait(timeout=5)
    except Exception:
        proc.kill()


@pytest.fixture(scope="module")
def python_server(tmp_path_factory: pytest.TempPathFactory):
    """Start the Python MCP server subprocess."""
    tmp = tmp_path_factory.mktemp("py_cache")
    proc = start_server(
        [sys.executable, "-m", "patent_mcp"],
        str(tmp / ".patents"),
    )
    yield proc
    try:
        proc.stdin.close()  # type: ignore[union-attr]
        proc.wait(timeout=5)
    except Exception:
        proc.kill()


@pytest.mark.slow
def test_protocol_version_matches(rust_server, python_server) -> None:
    """Both servers must report the same MCP protocol version."""
    rust_resp = send_rpc(rust_server, "initialize", MCP_INIT_PARAMS)
    py_resp = send_rpc(python_server, "initialize", MCP_INIT_PARAMS)

    rust_ver = rust_resp["result"]["protocolVersion"]
    py_ver = py_resp["result"]["protocolVersion"]
    assert rust_ver == py_ver, (
        f"Protocol version mismatch: Rust={rust_ver!r} Python={py_ver!r}"
    )


@pytest.mark.slow
def test_tools_list_names_match(rust_server, python_server) -> None:
    """Both servers must expose the same set of tool names."""
    rust_resp = send_rpc(rust_server, "tools/list", {})
    py_resp = send_rpc(python_server, "tools/list", {})

    rust_tools = {t["name"] for t in rust_resp["result"]["tools"]}
    py_tools = {t["name"] for t in py_resp["result"]["tools"]}
    assert rust_tools == py_tools, (
        f"Tool name mismatch:\n  Rust: {sorted(rust_tools)}\n  Python: {sorted(py_tools)}"
    )


@pytest.mark.slow
def test_fetch_patents_empty_structure(rust_server, python_server) -> None:
    """Empty fetch_patents should return matching response structure."""
    rust_resp = send_rpc(
        rust_server,
        "tools/call",
        {"name": "fetch_patents", "arguments": {"patent_ids": []}},
    )
    py_resp = send_rpc(
        python_server,
        "tools/call",
        {"name": "fetch_patents", "arguments": {"patent_ids": []}},
    )

    def extract_payload(resp: dict) -> dict:
        content = resp["result"]["content"]
        assert content, "Empty content list"
        text = content[0]["text"]
        return json.loads(text)

    rust_payload = extract_payload(rust_resp)
    py_payload = extract_payload(py_resp)

    # Both must have results and summary keys
    assert "results" in rust_payload, "Rust missing 'results'"
    assert "results" in py_payload, "Python missing 'results'"
    assert "summary" in rust_payload, "Rust missing 'summary'"
    assert "summary" in py_payload, "Python missing 'summary'"

    # Both results lists must be empty
    assert rust_payload["results"] == [], "Rust results not empty"
    assert py_payload["results"] == [], "Python results not empty"

    # Summary keys must exist in both
    summary_keys = {"total", "success", "cached", "errors", "total_duration_ms"}
    assert summary_keys <= rust_payload["summary"].keys(), (
        f"Rust summary missing keys: {summary_keys - rust_payload['summary'].keys()}"
    )
    assert summary_keys <= py_payload["summary"].keys(), (
        f"Python summary missing keys: {summary_keys - py_payload['summary'].keys()}"
    )
