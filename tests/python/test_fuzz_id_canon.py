"""Hypothesis fuzzing for id_canon — ensure canonicalize() never raises."""
import pytest
from hypothesis import given, settings
import hypothesis.strategies as st
from patent_mcp.id_canon import canonicalize

pytestmark = pytest.mark.slow


@given(st.text())
@settings(max_examples=500)
def test_canonicalize_never_raises(s):
    result = canonicalize(s)
    assert result is not None
    assert result.canonical  # always non-empty
    assert result.jurisdiction  # always non-empty


@given(st.text(alphabet="US0123456789-/"))
@settings(max_examples=200)
def test_canonicalize_us_like_inputs(s):
    result = canonicalize(s)
    assert result is not None
