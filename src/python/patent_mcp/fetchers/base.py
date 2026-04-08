"""Base patent source interface — ABC that all sources implement."""
from __future__ import annotations

from abc import ABC, abstractmethod
from dataclasses import dataclass, field
from pathlib import Path
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from patent_mcp.cache import PatentMetadata, SourceAttempt
    from patent_mcp.config import PatentConfig
    from patent_mcp.id_canon import CanonicalPatentId


@dataclass
class FetchResult:
    """Result of a single source fetch attempt."""
    source_attempt: "SourceAttempt"
    # Files written to output_dir (or None if fetch failed)
    pdf_path: Path | None = None
    txt_path: Path | None = None
    image_urls: list[str] = field(default_factory=list)
    metadata: "PatentMetadata | None" = None


class BasePatentSource(ABC):
    """Abstract base class for all patent data sources."""

    def __init__(self, config: "PatentConfig") -> None:
        self._config = config

    @property
    @abstractmethod
    def source_name(self) -> str:
        """Unique identifier for this source (e.g. 'USPTO', 'EPO_OPS')."""

    @property
    @abstractmethod
    def supported_jurisdictions(self) -> frozenset[str]:
        """Set of jurisdiction codes this source can handle; empty = all."""

    def can_fetch(self, patent: "CanonicalPatentId") -> bool:
        """Return True if this source supports the given patent jurisdiction."""
        jx = self.supported_jurisdictions
        return not jx or patent.jurisdiction in jx

    @abstractmethod
    async def fetch(
        self,
        patent: "CanonicalPatentId",
        output_dir: Path,
    ) -> FetchResult:
        """Fetch patent data and write files to output_dir."""

    def _base_url(self, key: str, default: str) -> str:
        """Return base URL, allowing per-source override for testing."""
        return self._config.source_base_urls.get(key, default)
