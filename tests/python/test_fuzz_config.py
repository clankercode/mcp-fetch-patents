"""Hypothesis fuzzing for config — ensure load_config() never raises."""
import pytest
from hypothesis import given, settings
import hypothesis.strategies as st
from patent_mcp.config import load_config

pytestmark = pytest.mark.slow


@given(st.dictionaries(
    keys=st.sampled_from(["PATENT_CACHE_DIR", "PATENT_CONCURRENCY", "PATENT_EPO_KEY",
                           "PATENT_LENS_KEY", "PATENT_TIMEOUT_SECS"]),
    values=st.text(max_size=50)
))
@settings(max_examples=200)
def test_load_config_never_raises(env_overrides):
    try:
        cfg = load_config(env=env_overrides)
    except Exception as e:
        raise AssertionError(f"load_config raised with env={env_overrides}: {e}")
