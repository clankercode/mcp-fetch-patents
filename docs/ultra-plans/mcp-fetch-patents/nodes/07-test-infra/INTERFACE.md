# INTERFACE — 07-test-infra

## Exposes (for test suites, not production code)

### Mock Server
```python
# Start/stop in pytest fixtures
def start_mock_server(port: int, routes_file: Path) -> MockServer: ...
def stop_mock_server(server: MockServer) -> None: ...

# Config override for tests pointing to mock server
def test_config(mock_server_port: int, tmp_cache_dir: Path) -> PatentConfig: ...
```

### Fixture Helpers
```python
def load_fixture(jurisdiction: str, patent_id: str) -> PatentFixture: ...
def list_fixtures() -> list[str]: ...  # returns all fixture patent IDs

@dataclass
class PatentFixture:
    canonical_id: str
    expected_metadata: PatentMetadata
    mock_responses: dict[str, bytes]  # source_name -> response bytes
    expected_files: dict[str, bytes]  # format -> expected content
```

### Parity Assertion
```python
def assert_parity(python_result: dict, rust_result: dict) -> None:
    """Assert Python and Rust produced semantically identical results."""
    ...
```

## Depends On
All other nodes (as subjects of testing).

## Consumed By
All test files. Not consumed by production code.
