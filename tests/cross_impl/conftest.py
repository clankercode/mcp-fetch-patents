import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

# Ensure the Python package is importable without installation
_src_python = str(Path(__file__).parent.parent.parent / "src" / "python")
if _src_python not in sys.path:
    sys.path.insert(0, _src_python)
_existing = os.environ.get("PYTHONPATH", "")
os.environ["PYTHONPATH"] = _src_python + (os.pathsep + _existing if _existing else "")

# Path to the compiled Rust binary (built during tests if needed)
RUST_DIR = Path(__file__).parent.parent.parent / "src" / "rust"
RUST_BIN = RUST_DIR / "target" / "debug" / "patent-mcp-server"


def pytest_configure(config):
    """Build the Rust binary before cross-impl tests run."""
    pass  # Build happens lazily in the rust_binary fixture


@pytest.fixture(scope="session")
def rust_binary():
    """Build and return path to Rust binary."""
    env = os.environ.copy()
    env["CC"] = "gcc"
    result = subprocess.run(
        ["cargo", "build", "--manifest-path", str(RUST_DIR / "Cargo.toml")],
        capture_output=True,
        text=True,
        env=env,
    )
    assert result.returncode == 0, f"Rust build failed: {result.stderr}"
    assert RUST_BIN.exists(), f"Binary not found: {RUST_BIN}"
    return str(RUST_BIN)
