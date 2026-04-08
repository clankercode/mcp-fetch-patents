"""Shared test fixtures and configuration."""
from __future__ import annotations

import os
import sys
from pathlib import Path

# Ensure the package is importable without installation (in-process)
_src_python = str(Path(__file__).parent.parent.parent / "src" / "python")
sys.path.insert(0, _src_python)

# Propagate to subprocesses spawned during tests
_existing = os.environ.get("PYTHONPATH", "")
os.environ["PYTHONPATH"] = _src_python + (os.pathsep + _existing if _existing else "")

# ---------------------------------------------------------------------------
# Avoid collecting slow test files when running the fast suite.
#
# pytest collects (imports) ALL test files before applying -m filters, which
# pays the module import cost even for deselected tests. The files below import
# heavy libraries (FastMCP ~430ms, httpx+respx ~250ms, hypothesis ~50ms) that
# violate the <1s fast-suite budget.
#
# We skip collection when the user explicitly opts out of slow tests (the
# standard fast-suite invocation: -m "not slow").
# ---------------------------------------------------------------------------
def _running_fast_suite() -> bool:
    """Return True if only non-slow tests were requested."""
    import _pytest.config as _pc
    try:
        argv = sys.argv
    except Exception:
        return False
    return any("not slow" in a for a in argv)

# Files that import heavy dependencies and are fully marked as slow
_HEAVY_TEST_FILES = [
    "test_server.py",       # FastMCP ~430ms
    "test_fetchers_http.py",  # respx + httpx ~250ms
    "test_integration.py",  # FastMCP + respx + httpx at module level ~600ms
    "test_fuzz_id_canon.py",  # hypothesis (already slow-marked)
    "test_fuzz_config.py",    # hypothesis (already slow-marked)
]

if _running_fast_suite():
    _here = Path(__file__).parent
    collect_ignore = [str(_here / f) for f in _HEAVY_TEST_FILES]
