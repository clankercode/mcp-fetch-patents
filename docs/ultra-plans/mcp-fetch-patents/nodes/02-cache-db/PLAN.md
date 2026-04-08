# PLAN — 02-cache-db: Cache Layer + Global Index

*Depends on: 06-config (T01-T08), 01-id-canon (T01-T15), 07-test-infra (T08-T10 for test_config fixture)*

---

## Python Implementation

### T01 — SQLite schema creation
- **RED**: `test_schema_created_on_init` — init `PatentCache(test_config)`; assert tables `patents`, `patent_locations`, `cache_registrations`, `fetch_sources` exist in SQLite
- **GREEN**: implement `PatentCache.__init__()` that creates DB + runs `CREATE TABLE IF NOT EXISTS` migrations
- **REFACTOR**: schema in a separate `SCHEMA_SQL` constant; use WAL mode pragma

### T02 — Cache miss returns None
- **RED**: `test_cache_miss_returns_none` — `cache.lookup("US7654321")` on empty DB → returns `None`
- **GREEN**: `SELECT` from `patent_locations` returns empty → return None

### T03 — Store and retrieve artifacts
- **RED**: `test_store_and_lookup_pdf` — store ArtifactSet with a PDF path; lookup; assert `result.files["pdf"]` matches stored path
- **RED**: `test_store_and_lookup_all_formats` — store PDF + txt + md + 2 images; lookup; all paths returned
- **GREEN**: implement `cache.store()` — write files to disk (copy from ArtifactSet paths), insert DB rows
- **REFACTOR**: wrap store in a single SQLite transaction

### T04 — Metadata stored and returned
- **RED**: `test_metadata_stored` — store with `PatentMetadata(title="Widget assembly", inventors=["Alice"])`; lookup; assert `result.metadata.title == "Widget assembly"`
- **GREEN**: insert into `patents` table; join on lookup

### T05 — Stale file detection
- **RED**: `test_stale_file_returns_none` — store artifact, delete the file, lookup; assert returns None (or `is_complete=False`)
- **GREEN**: after DB hit, `Path.exists()` check on each file; flag stale entries

### T06 — Cache registration
- **RED**: `test_register_cache_dir` — call `cache.register_cache_dir(Path("/tmp/test/.patents"))`; query `cache_registrations`; assert entry exists
- **GREEN**: `INSERT OR IGNORE INTO cache_registrations` 

### T07 — list_all returns all patents
- **RED**: `test_list_all_empty` → `[]`
- **RED**: `test_list_all_two_patents` — store 2; list_all returns 2 entries
- **GREEN**: `SELECT DISTINCT patent_id FROM patent_locations`

### T08 — PPUBS session token cache
- **RED**: `test_session_cache_miss` — `SessionCache().get("PPUBS")` → None
- **RED**: `test_session_cache_store_and_get` — store token with TTL=30min; get → returns token
- **RED**: `test_session_cache_expired` — store with TTL=0; sleep 0.01s; get → None
- **RED**: `test_session_cache_set_with_expiry` — store using `set_with_expiry(expires_at=now+1h)`; get before expiry → token; get after → None
- **RED**: `test_session_cache_invalidate` — store; invalidate; get → None
- **GREEN**: implement `SessionCache` with `dict[str, SessionToken]`, datetime comparison
- **NOTE on TTLs**: PPUBS TTL=30min (undocumented API timeout); EPO OPS: use `expires_in` from OAuth response via `set_with_expiry()`; all other sources: default 30min unless overridden
- **REFACTOR**: `SessionCache` is separate from `PatentCache` (in-memory, no DB)

### T09 — Concurrent writes (WAL mode)
- **RED**: `test_concurrent_stores` — 10 concurrent `cache.store()` calls with different patent IDs using asyncio; all succeed; DB has 10 entries
- **GREEN**: SQLite WAL mode + per-operation transactions prevent contention

### T10 — sources.json written
- **RED**: `test_sources_json_written` — after store with `fetch_sources=[SourceAttempt(...)]`; read `{cache_dir}/{patent_id}/sources.json`; assert JSON contains source name
- **GREEN**: write `sources.json` alongside artifact files

### T11 — metadata.json written
- **RED**: `test_metadata_json_written` — after store; read `{cache_dir}/{patent_id}/metadata.json`; assert valid JSON with `canonical_id` field
- **GREEN**: write `metadata.json` alongside artifact files

---

## Rust Implementation

### T12 — Rust: schema + CRUD in `cache/tests.rs`
- Mirror T01–T08 in Rust using `rusqlite` crate
- Same SQL schema; `serde_json` for JSON files
- **RED → GREEN → REFACTOR** cycle for each

### T13 — Rust: SessionCache
- Mirror T08 in Rust; use `std::time::SystemTime` for expiry

### T14 — Parity: same DB state after identical operations
- **RED**: `test_db_state_parity` in `cross_impl/test_cache_parity.py`:
  - Run same store sequence via Python and Rust (via subprocess + mock server)
  - Export both DBs with `sqlite3 {db_path} .dump` → text
  - `assert_db_dump_parity(py_dump, rust_dump)` — compare schema + data rows (strip timestamps)
- **GREEN**: Both implementations produce identical SQLite schemas and data rows for same operations
- **Note**: Log-based comparison replaced by SQLite dump comparison — more robust than parsing debug log lines

---

## Acceptance Criteria
- All DB operations wrapped in transactions
- `lookup()` is fast (<1ms) for cache hits (SQLite index on `patent_id`)
- `store()` is atomic — no partial writes on failure
- Python and Rust produce identical DB state and JSON files for same inputs

## Dependencies
- `06-config` (T01-T08)
- `01-id-canon` (T01-T15)
- `07-test-infra` (T10 — `test_config` fixture)
