# SPEC — 02-cache-db: Cache Layer + Global Index

## Responsibility
Manage the local per-repo patent cache and the system-wide global SQLite index. Provide cache hit/miss lookup, store downloaded patent artifacts, register caches, and discover existing caches across the system.

## Local Cache (per-repo)
Location: `.patents/` in the working directory of the MCP server process.

```
.patents/
  {canonical_id}/           # e.g. US7654321/
    metadata.json           # patent metadata + status stubs
    patent.pdf              # full patent PDF
    patent.txt              # full text (if available)
    patent.md               # markdown conversion
    images/
      fig001.png
      fig001_ocr.txt        # tesseract output for this figure
      fig002.png
      fig002_ocr.txt
    sources.json            # which sources were used, when, URLs
    raw/                    # raw downloads (before processing)
      *.html, *.xml, etc.
```

## Global Index (system-wide)
Location: `$XDG_DATA_HOME/patent-cache/index.db` (defaults to `~/.local/share/patent-cache/index.db`)

### SQLite Schema (v1)

```sql
CREATE TABLE patents (
  id TEXT PRIMARY KEY,               -- canonical patent ID
  jurisdiction TEXT NOT NULL,
  doc_type TEXT NOT NULL,            -- patent | application
  title TEXT,
  abstract TEXT,
  inventors TEXT,                    -- JSON array
  assignee TEXT,
  filing_date TEXT,
  publication_date TEXT,
  grant_date TEXT,
  fetched_at TEXT NOT NULL,          -- ISO8601
  last_verified_at TEXT,
  -- Legal status (v2 fields, nullable in v1)
  legal_status TEXT,                 -- live | dead | pending | null
  status_fetched_at TEXT,
  UNIQUE(id)
);

CREATE TABLE patent_locations (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  patent_id TEXT NOT NULL REFERENCES patents(id),
  cache_dir TEXT NOT NULL,           -- absolute path to .patents/ dir
  format TEXT NOT NULL,              -- pdf | txt | md | image | raw_html | raw_xml
  file_path TEXT NOT NULL,           -- absolute path to file
  registered_at TEXT NOT NULL,       -- ISO8601
  UNIQUE(patent_id, cache_dir, format)
);

CREATE TABLE cache_registrations (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  cache_dir TEXT NOT NULL UNIQUE,    -- absolute path to .patents/ dir
  registered_at TEXT NOT NULL,
  last_seen_at TEXT NOT NULL
);

CREATE TABLE fetch_sources (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  patent_id TEXT NOT NULL REFERENCES patents(id),
  source_name TEXT NOT NULL,         -- e.g. "USPTO", "EPO_OPS", "Google_Patents"
  url TEXT,
  fetched_at TEXT NOT NULL,
  success INTEGER NOT NULL,          -- 1 = success, 0 = failure
  error_msg TEXT,
  formats_retrieved TEXT             -- JSON array of format strings
);

CREATE INDEX idx_patents_jurisdiction ON patents(jurisdiction);
CREATE INDEX idx_locations_patent ON patent_locations(patent_id);
CREATE INDEX idx_locations_cache ON patent_locations(cache_dir);
```

## Cache Lookup Algorithm
1. Normalize input ID via `01-id-canon`
2. Query global index: `SELECT * FROM patent_locations WHERE patent_id = ?`
3. For each location found, verify file exists on disk (files may have moved)
4. If files exist → return paths (cache hit)
5. If global index has entry but files missing → mark stale, proceed to fetch
6. If no index entry → proceed to fetch (cache miss)

## Cache Registration
When a new `.patents/` directory is created, register it in the global index:
```
INSERT OR IGNORE INTO cache_registrations(cache_dir, registered_at, last_seen_at) VALUES (?, ?, ?)
```

## Cache Discovery (finding "other" caches)
The server discovers all known caches from the global index (`cache_registrations` table). No filesystem scanning required — caches self-register when first used.

## Artifact Storage
After successful fetch:
1. Create `.patents/{canonical_id}/` directory
2. Write each artifact to appropriate path
3. Write `metadata.json` and `sources.json`
4. Insert/update rows in global `patents`, `patent_locations`, `fetch_sources`
5. Update `cache_registrations.last_seen_at`

## metadata.json Schema
```json
{
  "canonical_id": "US7654321",
  "jurisdiction": "US",
  "doc_type": "patent",
  "title": "...",
  "abstract": "...",
  "inventors": ["Alice Smith", "Bob Jones"],
  "assignee": "ACME Corp",
  "filing_date": "2005-03-12",
  "publication_date": "2010-01-19",
  "grant_date": "2010-01-19",
  "fetched_at": "2026-04-07T12:00:00Z",
  "last_verified_at": "2026-04-07T12:00:00Z",
  "legal_status": null,
  "status_fetched_at": null,
  "formats_available": ["pdf", "txt", "md", "images"],
  "image_count": 5,
  "sources_used": ["USPTO", "Google_Patents"]
}
```

## API Session Token Cache (in-memory)
Some sources (notably PPUBS) require multi-step session establishment. Cache session tokens in memory with TTL to avoid re-establishing per request:
```python
@dataclass
class SessionToken:
    token: str
    expires_at: datetime

class SessionCache:
    _cache: dict[str, SessionToken]  # source_name -> token
    
    def get(self, source: str) -> str | None:
        """Return cached token if not expired."""
    
    def set(self, source: str, token: str, ttl_minutes: int = 30) -> None: ...
    
    def invalidate(self, source: str) -> None: ...
```
Session cache is per-process (in-memory only); not persisted to disk.

## Implementation Notes
- SQLite WAL mode for concurrent access
- All writes are atomic (transaction per patent download)
- Python: `sqlite3` stdlib + dataclasses
- Rust: `rusqlite` crate + serde structs
- Both implementations must produce identical SQL and JSON schemas

## Dependencies
- `01-id-canon` (for normalization)
- `06-config` (for path resolution)

## Test Surface
- Unit: insert + lookup roundtrip
- Unit: stale file detection
- Unit: cache miss → returns None (not an error)
- Unit: concurrent writes (WAL mode)
- Cross-impl: Python and Rust produce identical DB state for same operations
