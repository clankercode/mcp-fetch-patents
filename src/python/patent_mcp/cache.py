"""Cache layer: in-memory session token cache + on-disk patent artifact cache backed by SQLite."""
from __future__ import annotations

import json
import logging
import shutil
import sqlite3
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from patent_mcp.config import PatentConfig

log = logging.getLogger(__name__)


# ---------------------------------------------------------------------------
# Data types
# ---------------------------------------------------------------------------

@dataclass
class SessionToken:
    token: str
    expires_at: datetime  # UTC


@dataclass
class ArtifactSet:
    pdf: Path | None = None
    txt: Path | None = None
    md: Path | None = None
    images: list[Path] = field(default_factory=list)
    raw: list[Path] = field(default_factory=list)


@dataclass
class PatentMetadata:
    canonical_id: str
    jurisdiction: str
    doc_type: str
    title: str | None = None
    abstract: str | None = None
    inventors: list[str] = field(default_factory=list)
    assignee: str | None = None
    filing_date: str | None = None
    publication_date: str | None = None
    grant_date: str | None = None
    fetched_at: str = field(default_factory=lambda: _now_utc())
    legal_status: str | None = None       # always null in v1
    status_fetched_at: str | None = None  # always null in v1


@dataclass
class CacheResult:
    canonical_id: str
    cache_dir: Path
    files: dict[str, Path]   # format → absolute path
    metadata: PatentMetadata
    is_complete: bool        # all files in DB actually exist on disk


@dataclass
class CacheEntry:
    canonical_id: str
    cache_dir: Path


@dataclass
class SourceAttempt:
    source: str
    success: bool
    elapsed_ms: float
    error: str | None = None
    metadata: dict[str, Any] | None = None


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _now_utc() -> str:
    return datetime.now(timezone.utc).isoformat()


def _utcnow() -> datetime:
    return datetime.now(timezone.utc)


def _safe_json_list(value: str | None) -> list:
    """Parse a JSON list string, returning [] on parse error or null."""
    if not value:
        return []
    try:
        result = json.loads(value)
        return result if isinstance(result, list) else []
    except (json.JSONDecodeError, ValueError):
        log.warning("Corrupted JSON list in cache DB: %r — returning []", value)
        return []


# ---------------------------------------------------------------------------
# SQLite schema
# ---------------------------------------------------------------------------

SCHEMA_SQL = """
PRAGMA journal_mode=WAL;
PRAGMA foreign_keys=ON;

CREATE TABLE IF NOT EXISTS patents (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    canonical_id     TEXT NOT NULL UNIQUE,
    jurisdiction     TEXT NOT NULL,
    doc_type         TEXT NOT NULL,
    title            TEXT,
    abstract         TEXT,
    inventors        TEXT,   -- JSON array
    assignee         TEXT,
    filing_date      TEXT,
    publication_date TEXT,
    grant_date       TEXT,
    fetched_at       TEXT NOT NULL,
    legal_status     TEXT,
    status_fetched_at TEXT,
    cache_dir        TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS patent_locations (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    patent_id    TEXT NOT NULL REFERENCES patents(canonical_id) ON DELETE CASCADE,
    format       TEXT NOT NULL,  -- 'pdf', 'txt', 'md', 'image_0', ...
    path         TEXT NOT NULL,
    UNIQUE(patent_id, format)
);

CREATE TABLE IF NOT EXISTS fetch_sources (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    patent_id  TEXT NOT NULL REFERENCES patents(canonical_id) ON DELETE CASCADE,
    source     TEXT NOT NULL,
    success    INTEGER NOT NULL,  -- 0 or 1
    elapsed_ms REAL NOT NULL,
    error      TEXT,
    extra_json TEXT              -- JSON blob for extra metadata
);

CREATE TABLE IF NOT EXISTS cache_registrations (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    cache_dir TEXT NOT NULL UNIQUE,
    registered_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_patent_locations_patent_id ON patent_locations(patent_id);
CREATE INDEX IF NOT EXISTS idx_patents_canonical_id ON patents(canonical_id);
"""


# ---------------------------------------------------------------------------
# SessionCache (in-memory, per-process)
# ---------------------------------------------------------------------------

class SessionCache:
    """In-memory session token cache. Per-process, not persisted to disk."""

    def __init__(self) -> None:
        self._tokens: dict[str, SessionToken] = {}

    def get(self, source: str) -> str | None:
        """Return valid token or None if missing/expired."""
        entry = self._tokens.get(source)
        if entry is None:
            return None
        if _utcnow() >= entry.expires_at:
            del self._tokens[source]
            return None
        return entry.token

    def set(self, source: str, token: str, ttl_minutes: int = 30) -> None:
        from datetime import timedelta
        expires_at = _utcnow() + timedelta(minutes=ttl_minutes)
        self._tokens[source] = SessionToken(token=token, expires_at=expires_at)

    def set_with_expiry(self, source: str, token: str, expires_at: datetime) -> None:
        """Use when source provides explicit expiry (e.g. EPO OPS OAuth response)."""
        self._tokens[source] = SessionToken(token=token, expires_at=expires_at)

    def invalidate(self, source: str) -> None:
        self._tokens.pop(source, None)


# ---------------------------------------------------------------------------
# PatentCache
# ---------------------------------------------------------------------------

class PatentCache:
    """On-disk patent cache backed by a single global SQLite DB."""

    def __init__(self, config: PatentConfig) -> None:
        self._config = config
        self._local_dir = Path(config.cache_local_dir)
        self._global_db_path = Path(config.cache_global_db)
        self._global_db_path.parent.mkdir(parents=True, exist_ok=True)
        self._init_db(self._global_db_path)

        # Suggest migration if old .patents/index.db exists in CWD
        old_local_db = Path(".patents") / "index.db"
        if old_local_db.exists():
            log.info(
                "Found old .patents/index.db in CWD. Patent cache now uses %s. "
                "The old .patents/ directory can be safely deleted.",
                self._global_db_path,
            )

        # Register cache dir in global index
        with self._connect() as conn:
            conn.execute(
                "INSERT OR IGNORE INTO cache_registrations(cache_dir, registered_at) VALUES (?,?)",
                (str(self._local_dir.resolve()), _now_utc()),
            )

    # ------------------------------------------------------------------
    # Private helpers
    # ------------------------------------------------------------------

    def _init_db(self, db_path: Path) -> None:
        db_path.parent.mkdir(parents=True, exist_ok=True)
        with sqlite3.connect(str(db_path)) as conn:
            conn.executescript(SCHEMA_SQL)

    def _connect(self, db_path: Path | None = None) -> sqlite3.Connection:
        path = db_path or self._global_db_path
        conn = sqlite3.connect(str(path))
        conn.row_factory = sqlite3.Row
        conn.execute("PRAGMA journal_mode=WAL")
        conn.execute("PRAGMA foreign_keys=ON")
        return conn

    def _patent_dir(self, canonical_id: str) -> Path:
        return self._local_dir / canonical_id

    # ------------------------------------------------------------------
    # Public API
    # ------------------------------------------------------------------

    def lookup(self, canonical_id: str) -> CacheResult | None:
        """Return cached artifacts for a patent, or None on miss/stale."""
        with self._connect() as conn:
            row = conn.execute(
                "SELECT * FROM patents WHERE canonical_id=?", (canonical_id,)
            ).fetchone()
            if row is None:
                return None

            loc_rows = conn.execute(
                "SELECT format, path FROM patent_locations WHERE patent_id=?",
                (canonical_id,),
            ).fetchall()

        files: dict[str, Path] = {}
        for loc in loc_rows:
            p = Path(loc["path"])
            if p.exists():
                files[loc["format"]] = p

        # If none of the expected files exist, treat as stale
        if loc_rows and not files:
            return None

        metadata = PatentMetadata(
            canonical_id=row["canonical_id"],
            jurisdiction=row["jurisdiction"],
            doc_type=row["doc_type"],
            title=row["title"],
            abstract=row["abstract"],
            inventors=_safe_json_list(row["inventors"]),
            assignee=row["assignee"],
            filing_date=row["filing_date"],
            publication_date=row["publication_date"],
            grant_date=row["grant_date"],
            fetched_at=row["fetched_at"],
            legal_status=row["legal_status"],
            status_fetched_at=row["status_fetched_at"],
        )
        is_complete = len(files) == len(loc_rows)
        return CacheResult(
            canonical_id=canonical_id,
            cache_dir=Path(row["cache_dir"]),
            files=files,
            metadata=metadata,
            is_complete=is_complete,
        )

    def store(
        self,
        canonical_id: str,
        artifacts: ArtifactSet,
        metadata: PatentMetadata,
        fetch_sources: list[SourceAttempt] | None = None,
    ) -> None:
        """Copy artifacts to cache dir and record in DB atomically."""
        dest_dir = self._patent_dir(canonical_id)
        dest_dir.mkdir(parents=True, exist_ok=True)

        def _copy_if_needed(src: Path, dst: Path) -> None:
            """Copy src → dst unless they resolve to the same path."""
            if src.resolve() != dst.resolve():
                shutil.copy2(src, dst)

        # Build file list to copy
        file_entries: list[tuple[str, Path]] = []  # (format, dest_path)
        if artifacts.pdf:
            dst = dest_dir / artifacts.pdf.name
            _copy_if_needed(artifacts.pdf, dst)
            file_entries.append(("pdf", dst))
        if artifacts.txt:
            dst = dest_dir / artifacts.txt.name
            _copy_if_needed(artifacts.txt, dst)
            file_entries.append(("txt", dst))
        if artifacts.md:
            dst = dest_dir / artifacts.md.name
            _copy_if_needed(artifacts.md, dst)
            file_entries.append(("md", dst))
        for i, img in enumerate(artifacts.images):
            dst = dest_dir / img.name
            _copy_if_needed(img, dst)
            file_entries.append((f"image_{i}", dst))
        for i, raw in enumerate(artifacts.raw):
            dst = dest_dir / raw.name
            _copy_if_needed(raw, dst)
            file_entries.append((f"raw_{i}", dst))

        # Write metadata.json
        meta_json = dest_dir / "metadata.json"
        meta_json.write_text(
            json.dumps(
                {
                    "canonical_id": metadata.canonical_id,
                    "jurisdiction": metadata.jurisdiction,
                    "doc_type": metadata.doc_type,
                    "title": metadata.title,
                    "abstract": metadata.abstract,
                    "inventors": metadata.inventors,
                    "assignee": metadata.assignee,
                    "filing_date": metadata.filing_date,
                    "publication_date": metadata.publication_date,
                    "grant_date": metadata.grant_date,
                    "fetched_at": metadata.fetched_at,
                    "legal_status": metadata.legal_status,
                    "status_fetched_at": metadata.status_fetched_at,
                },
                indent=2,
            )
        )

        # Write sources.json
        if fetch_sources:
            sources_json = dest_dir / "sources.json"
            sources_json.write_text(
                json.dumps(
                    [
                        {
                            "source": s.source,
                            "success": s.success,
                            "elapsed_ms": s.elapsed_ms,
                            "error": s.error,
                        }
                        for s in fetch_sources
                    ],
                    indent=2,
                )
            )

        # Persist to DB in one transaction
        with self._connect() as conn:
            conn.execute(
                """
                INSERT OR REPLACE INTO patents
                (canonical_id, jurisdiction, doc_type, title, abstract, inventors,
                 assignee, filing_date, publication_date, grant_date, fetched_at,
                 legal_status, status_fetched_at, cache_dir)
                VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?)
                """,
                (
                    metadata.canonical_id,
                    metadata.jurisdiction,
                    metadata.doc_type,
                    metadata.title,
                    metadata.abstract,
                    json.dumps(metadata.inventors),
                    metadata.assignee,
                    metadata.filing_date,
                    metadata.publication_date,
                    metadata.grant_date,
                    metadata.fetched_at,
                    metadata.legal_status,
                    metadata.status_fetched_at,
                    str(dest_dir),
                ),
            )
            # Remove old location rows then re-insert
            conn.execute("DELETE FROM patent_locations WHERE patent_id=?", (canonical_id,))
            conn.executemany(
                "INSERT INTO patent_locations(patent_id, format, path) VALUES (?,?,?)",
                [(canonical_id, fmt, str(p)) for fmt, p in file_entries],
            )
            if fetch_sources:
                conn.executemany(
                    """
                    INSERT INTO fetch_sources(patent_id, source, success, elapsed_ms, error, extra_json)
                    VALUES (?,?,?,?,?,?)
                    """,
                    [
                        (
                            canonical_id,
                            s.source,
                            1 if s.success else 0,
                            s.elapsed_ms,
                            s.error,
                            json.dumps(s.metadata) if s.metadata else None,
                        )
                        for s in fetch_sources
                    ],
                )

    def register_cache_dir(self, cache_dir: Path) -> None:
        """Register an external cache directory in the global index."""
        self._global_db_path.parent.mkdir(parents=True, exist_ok=True)
        with self._connect(self._global_db_path) as conn:
            conn.execute(
                "INSERT OR IGNORE INTO cache_registrations(cache_dir, registered_at) VALUES (?,?)",
                (str(cache_dir.resolve()), _now_utc()),
            )

    def list_all(self) -> list[CacheEntry]:
        """Return all cached patent IDs."""
        with self._connect() as conn:
            rows = conn.execute(
                "SELECT DISTINCT canonical_id, cache_dir FROM patents ORDER BY canonical_id"
            ).fetchall()
        return [CacheEntry(canonical_id=r["canonical_id"], cache_dir=Path(r["cache_dir"])) for r in rows]
