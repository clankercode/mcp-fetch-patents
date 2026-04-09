"""Per-repo activity journal — appends JSONL records of tool invocations."""
from __future__ import annotations

import json
import logging
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

log = logging.getLogger(__name__)


class ActivityJournal:
    """Appends one JSONL line per tool invocation. Failures never propagate."""

    def __init__(self, path: Path | None) -> None:
        self._path = path

    def _append_record(self, record: dict[str, Any]) -> None:
        if self._path is None:
            return
        try:
            line = json.dumps(record, default=str)
            with open(self._path, "a") as f:
                f.write(line + "\n")
        except Exception:
            log.warning("Failed to write activity journal %s", self._path, exc_info=True)

    def log_fetch(self, patent_ids: list[str], summary: dict[str, Any]) -> None:
        self._append_record({
            "ts": datetime.now(timezone.utc).isoformat(),
            "action": "fetch",
            "patent_ids": patent_ids,
            "results": summary,
        })

    def log_list(self, count: int) -> None:
        self._append_record({
            "ts": datetime.now(timezone.utc).isoformat(),
            "action": "list",
            "count": count,
        })

    def log_metadata(self, patent_ids: list[str], found: int, missing: int) -> None:
        self._append_record({
            "ts": datetime.now(timezone.utc).isoformat(),
            "action": "metadata",
            "patent_ids": patent_ids,
            "found": found,
            "missing": missing,
        })
