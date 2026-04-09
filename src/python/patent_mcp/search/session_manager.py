"""Patent search session manager.

Manages patent research sessions on disk with atomic JSON persistence.
Session files live in a configurable directory (default: .patent-sessions/).
"""

from __future__ import annotations

import json
import os
import re
import threading
import time
from dataclasses import dataclass, field, asdict
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from patent_mcp.utils import now_iso

# Per-directory lock registry to serialise index reads+writes within a process
_index_locks: dict[str, threading.Lock] = {}
_index_locks_mutex = threading.Lock()


def _get_index_lock(directory: Path) -> threading.Lock:
    key = str(directory.resolve())
    with _index_locks_mutex:
        if key not in _index_locks:
            _index_locks[key] = threading.Lock()
        return _index_locks[key]


# ---------------------------------------------------------------------------
# Data model
# ---------------------------------------------------------------------------


@dataclass
class PatentHit:
    patent_id: str
    title: str | None = None
    date: str | None = None  # ISO date string (publication or priority)
    assignee: str | None = None
    inventors: list[str] = field(default_factory=list)
    abstract: str | None = None
    source: str = ""  # which database it came from
    relevance: str = "unknown"  # "high" | "medium" | "low" | "unknown"
    note: str = ""  # researcher annotation
    prior_art: bool | None = None  # None = unknown, True/False = determined
    url: str | None = None  # direct link to patent page


@dataclass
class QueryRecord:
    query_id: str  # "q001", "q002", etc.
    timestamp: str  # ISO8601
    source: str  # "USPTO" | "EPO_OPS" | "Google_Patents" | etc.
    query_text: str
    result_count: int
    results: list[PatentHit]
    metadata: dict[str, Any] | None = None  # planner output, backend info, etc.


@dataclass
class Session:
    session_id: str
    topic: str
    created_at: str  # ISO8601
    modified_at: str  # ISO8601
    prior_art_cutoff: str | None  # ISO date, e.g. "2020-01-01"
    notes: str
    queries: list[QueryRecord]
    classifications_explored: list[str]  # IPC/CPC codes like ["H02J50", "H01F38"]
    citation_chains: dict[str, Any]  # patent_id -> {forward: [...], backward: [...]}
    patent_families: dict[str, list[str]]  # patent_id -> [family_members]


@dataclass
class SessionSummary:
    session_id: str
    topic: str
    created_at: str
    modified_at: str
    query_count: int
    patent_count: int


# ---------------------------------------------------------------------------
# Serialisation helpers
# ---------------------------------------------------------------------------


def _session_to_dict(session: Session) -> dict[str, Any]:
    """Convert Session (and nested dataclasses) to a plain dict."""
    d = asdict(session)
    return d


def _patent_hit_from_dict(d: dict[str, Any]) -> PatentHit:
    return PatentHit(
        patent_id=d["patent_id"],
        title=d.get("title"),
        date=d.get("date"),
        assignee=d.get("assignee"),
        inventors=d.get("inventors", []),
        abstract=d.get("abstract"),
        source=d.get("source", ""),
        relevance=d.get("relevance", "unknown"),
        note=d.get("note", ""),
        prior_art=d.get("prior_art"),
        url=d.get("url"),
    )


def _query_record_from_dict(d: dict[str, Any]) -> QueryRecord:
    return QueryRecord(
        query_id=d["query_id"],
        timestamp=d["timestamp"],
        source=d.get("source", ""),
        query_text=d.get("query_text", ""),
        result_count=d.get("result_count", 0),
        results=[_patent_hit_from_dict(r) for r in d.get("results", [])],
        metadata=d.get("metadata"),
    )


def _session_from_dict(d: dict[str, Any]) -> Session:
    return Session(
        session_id=d["session_id"],
        topic=d.get("topic", ""),
        created_at=d["created_at"],
        modified_at=d["modified_at"],
        prior_art_cutoff=d.get("prior_art_cutoff"),
        notes=d.get("notes", ""),
        queries=[_query_record_from_dict(q) for q in d.get("queries", [])],
        classifications_explored=d.get("classifications_explored", []),
        citation_chains=d.get("citation_chains", {}),
        patent_families=d.get("patent_families", {}),
    )


def _make_slug(topic: str) -> str:
    """Lower, spaces→hyphens, strip non-alphanumeric-except-hyphens, first 30 chars."""
    slug = topic.lower().replace(" ", "-")
    slug = re.sub(r"[^a-z0-9\-]", "", slug)
    return slug[:30]


def _count_unique_patents(session: Session) -> int:
    seen: set[str] = set()
    for q in session.queries:
        for r in q.results:
            seen.add(r.patent_id)
    return len(seen)


def _validate_session_id(session_id: str) -> None:
    if not session_id:
        raise ValueError("Session ID cannot be empty")
    if (
        "/" in session_id
        or "\\" in session_id
        or ".." in session_id
        or "\0" in session_id
    ):
        raise ValueError(f"Invalid session ID: {session_id!r}")


# ---------------------------------------------------------------------------
# SessionManager
# ---------------------------------------------------------------------------


class SessionManager:
    def __init__(self, sessions_dir: Path | str | None = None):
        if sessions_dir is not None:
            self._dir = Path(sessions_dir)
        else:
            env_dir = os.environ.get("PATENT_SESSIONS_DIR")
            if env_dir:
                self._dir = Path(env_dir)
            else:
                self._dir = Path(".patent-sessions")
        self._dir.mkdir(parents=True, exist_ok=True)

    @property
    def sessions_dir(self) -> Path:
        return self._dir

    # ------------------------------------------------------------------
    # Public API
    # ------------------------------------------------------------------

    def create_session(
        self,
        topic: str,
        prior_art_cutoff: str | None = None,
        notes: str = "",
    ) -> Session:
        """Create a new session and persist it to disk."""
        now = now_iso()
        slug = _make_slug(topic)
        ts = datetime.now(tz=timezone.utc).strftime("%Y%m%d-%H%M%S")
        session_id = f"{ts}-{slug}"
        session = Session(
            session_id=session_id,
            topic=topic,
            created_at=now,
            modified_at=now,
            prior_art_cutoff=prior_art_cutoff,
            notes=notes,
            queries=[],
            classifications_explored=[],
            citation_chains={},
            patent_families={},
        )
        self.save_session(session)
        return session

    def _resolve_and_check(self, session_id: str) -> Path:
        _validate_session_id(session_id)
        path = self._dir / f"{session_id}.json"
        resolved_path = path.resolve()
        resolved_dir = self._dir.resolve()
        if not str(resolved_path).startswith(str(resolved_dir)):
            raise ValueError(f"Session path escapes directory: {session_id}")
        return path

    def load_session(self, session_id: str) -> Session:
        """Load a session by ID. Raises FileNotFoundError if not found."""
        path = self._resolve_and_check(session_id)
        if not path.exists():
            raise FileNotFoundError(f"Session not found: {session_id}")
        data = json.loads(path.read_text(encoding="utf-8"))
        return _session_from_dict(data)

    def save_session(self, session: Session) -> None:
        """Atomically write session to disk and update the index."""
        session.modified_at = now_iso()
        path = self._resolve_and_check(session.session_id)
        tmp_path = path.with_suffix(".json.tmp")
        content = json.dumps(_session_to_dict(session), indent=2, ensure_ascii=False)
        tmp_path.write_text(content, encoding="utf-8")
        tmp_path.rename(path)
        self._update_index(session)

    def list_sessions(self, limit: int | None = None) -> list[SessionSummary]:
        """Return sessions sorted by modified_at descending.

        Reads from .index.json when available; falls back to scanning .json files.
        """
        index_path = self._dir / ".index.json"
        if index_path.exists():
            try:
                data = json.loads(index_path.read_text(encoding="utf-8"))
                summaries = [
                    SessionSummary(
                        session_id=s["session_id"],
                        topic=s["topic"],
                        created_at=s["created_at"],
                        modified_at=s["modified_at"],
                        query_count=s["query_count"],
                        patent_count=s["patent_count"],
                    )
                    for s in data.get("sessions", [])
                ]
                summaries.sort(key=lambda s: s.modified_at, reverse=True)
                if limit is not None:
                    summaries = summaries[:limit]
                return summaries
            except (json.JSONDecodeError, KeyError):
                pass  # fall through to scan

        # Fallback: scan individual .json files
        summaries: list[SessionSummary] = []
        for p in self._dir.glob("*.json"):
            if p.name.startswith("."):
                continue
            try:
                data = json.loads(p.read_text(encoding="utf-8"))
                session = _session_from_dict(data)
                summaries.append(
                    SessionSummary(
                        session_id=session.session_id,
                        topic=session.topic,
                        created_at=session.created_at,
                        modified_at=session.modified_at,
                        query_count=len(session.queries),
                        patent_count=_count_unique_patents(session),
                    )
                )
            except (json.JSONDecodeError, KeyError):
                continue

        summaries.sort(key=lambda s: s.modified_at, reverse=True)
        if limit is not None:
            summaries = summaries[:limit]
        return summaries

    def append_query_result(self, session_id: str, query: QueryRecord) -> None:
        """Append a QueryRecord to an existing session."""
        _validate_session_id(session_id)
        session = self.load_session(session_id)
        session.queries.append(query)
        self.save_session(session)

    def add_note(self, session_id: str, note: str) -> None:
        """Append a note to a session (double newline separator)."""
        _validate_session_id(session_id)
        session = self.load_session(session_id)
        if session.notes:
            session.notes = session.notes + "\n\n" + note
        else:
            session.notes = note
        self.save_session(session)

    def annotate_patent(
        self,
        session_id: str,
        patent_id: str,
        annotation: str,
        relevance: str,
    ) -> None:
        """Find patent across all queries in session and update note + relevance."""
        _validate_session_id(session_id)
        session = self.load_session(session_id)
        updated = False
        for query in session.queries:
            for hit in query.results:
                if hit.patent_id == patent_id:
                    hit.note = annotation
                    hit.relevance = relevance
                    updated = True
        if updated:
            self.save_session(session)

    def export_markdown(
        self,
        session_id: str,
        output_path: Path | None = None,
    ) -> Path:
        """Generate a Markdown report for the session.

        Returns the path to the written file.
        """
        _validate_session_id(session_id)
        session = self.load_session(session_id)

        if output_path is None:
            output_path = self._resolve_and_check(session_id).with_name(
                f"{session_id}-report.md"
            )

        lines: list[str] = []

        # Header
        lines.append(f"# Patent Search Report: {session.topic}")
        lines.append("")
        lines.append(f"**Session ID:** {session.session_id}  ")
        lines.append(f"**Created:** {session.created_at}  ")
        lines.append(f"**Last Modified:** {session.modified_at}  ")
        if session.prior_art_cutoff:
            lines.append(f"**Prior Art Cutoff:** {session.prior_art_cutoff}  ")
        lines.append("")

        # Summary stats
        total_queries = len(session.queries)
        unique_patents: dict[str, PatentHit] = {}
        for query in session.queries:
            for hit in query.results:
                if hit.patent_id not in unique_patents:
                    unique_patents[hit.patent_id] = hit
        total_patents = len(unique_patents)

        lines.append("## Summary")
        lines.append("")
        lines.append(f"- **Queries run:** {total_queries}")
        lines.append(f"- **Unique patents found:** {total_patents}")
        if session.classifications_explored:
            lines.append(
                f"- **Classifications explored:** {', '.join(session.classifications_explored)}"
            )
        lines.append("")

        # Researcher notes
        if session.notes:
            lines.append("## Researcher Notes")
            lines.append("")
            lines.append(session.notes)
            lines.append("")

        # All patents sorted by relevance then date
        if unique_patents:
            _relevance_order = {"high": 0, "medium": 1, "low": 2, "unknown": 3}

            def _sort_key(hit: PatentHit) -> tuple[int, str]:
                return (_relevance_order.get(hit.relevance, 3), hit.date or "")

            sorted_hits = sorted(unique_patents.values(), key=_sort_key)

            lines.append("## Patents Found")
            lines.append("")
            lines.append("| Patent ID | Title | Date | Relevance | Assignee | Note |")
            lines.append("|-----------|-------|------|-----------|----------|------|")
            for hit in sorted_hits:
                title = (hit.title or "").replace("|", "\\|")
                note = (hit.note or "").replace("|", "\\|")
                assignee = (hit.assignee or "").replace("|", "\\|")
                lines.append(
                    f"| {hit.patent_id} | {title} | {hit.date or ''} "
                    f"| {hit.relevance} | {assignee} | {note} |"
                )
            lines.append("")

        # Per-query detail
        if session.queries:
            lines.append("## Query History")
            lines.append("")
            for query in session.queries:
                lines.append(f"### {query.query_id} — {query.source}")
                lines.append("")
                lines.append(f"**Timestamp:** {query.timestamp}  ")
                lines.append(f"**Query:** `{query.query_text}`  ")
                lines.append(f"**Results:** {query.result_count}  ")
                lines.append("")
                if query.results:
                    lines.append("| Patent ID | Title | Date | Relevance |")
                    lines.append("|-----------|-------|------|-----------|")
                    for hit in query.results:
                        title = (hit.title or "").replace("|", "\\|")
                        lines.append(
                            f"| {hit.patent_id} | {title} | {hit.date or ''} "
                            f"| {hit.relevance} |"
                        )
                    lines.append("")

        report = "\n".join(lines)
        output_path = Path(output_path)
        output_path.write_text(report, encoding="utf-8")
        return output_path

    # ------------------------------------------------------------------
    # Internal helpers
    # ------------------------------------------------------------------

    def _update_index(self, session: Session) -> None:
        """Rebuild the index entry for this session in .index.json (atomic, thread-safe)."""
        index_path = self._dir / ".index.json"
        lock = _get_index_lock(self._dir)

        with lock:
            # Load existing index while holding the lock
            existing: list[dict[str, Any]] = []
            if index_path.exists():
                try:
                    data = json.loads(index_path.read_text(encoding="utf-8"))
                    existing = [
                        s
                        for s in data.get("sessions", [])
                        if s.get("session_id") != session.session_id
                    ]
                except (json.JSONDecodeError, KeyError):
                    existing = []

            # Build summary entry
            entry: dict[str, Any] = {
                "session_id": session.session_id,
                "topic": session.topic,
                "created_at": session.created_at,
                "modified_at": session.modified_at,
                "query_count": len(session.queries),
                "patent_count": _count_unique_patents(session),
            }
            existing.append(entry)

            content = json.dumps({"sessions": existing}, indent=2, ensure_ascii=False)
            # Use a unique tmp file name per-thread to avoid collisions
            tmp_path = index_path.with_name(f".index.json.{threading.get_ident()}.tmp")
            tmp_path.write_text(content, encoding="utf-8")
            tmp_path.rename(index_path)
