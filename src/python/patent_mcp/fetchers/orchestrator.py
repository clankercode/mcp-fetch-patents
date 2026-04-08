"""Source fetcher orchestrator — coordinates all sources, cache, and conversion."""
from __future__ import annotations

import asyncio
import logging
from dataclasses import dataclass, field
from pathlib import Path
from typing import TYPE_CHECKING

from patent_mcp.cache import ArtifactSet, PatentCache, PatentMetadata, SourceAttempt
from patent_mcp.fetchers.base import BasePatentSource, FetchResult

if TYPE_CHECKING:
    from patent_mcp.config import PatentConfig
    from patent_mcp.id_canon import CanonicalPatentId

log = logging.getLogger(__name__)


@dataclass
class OrchestratorResult:
    canonical_id: str
    success: bool
    cache_dir: Path | None = None
    files: dict[str, Path] = field(default_factory=dict)
    metadata: PatentMetadata | None = None
    sources: list[SourceAttempt] = field(default_factory=list)
    error: str | None = None
    from_cache: bool = False


class FetcherOrchestrator:
    """Coordinates patent fetching across all sources with caching."""

    def __init__(
        self,
        config: "PatentConfig",
        cache: PatentCache | None = None,
    ) -> None:
        self._config = config
        self._cache = cache or PatentCache(config)
        self._sources: list[BasePatentSource] = self._build_sources()

    # ------------------------------------------------------------------
    # Source registry
    # ------------------------------------------------------------------

    def _build_sources(self) -> list[BasePatentSource]:
        """Build all available sources in config priority order."""
        from patent_mcp.cache import SessionCache
        from patent_mcp.fetchers.http import (
            BigQuerySource,
            CipoScrapeSource,
            EpoOpsSource,
            GooglePatentsSource,
            IpAustraliaSource,
            PatentsViewStubSource,
            PpubsSource,
            WipoScrapeSource,
            EspacenetSource,
        )
        from patent_mcp.fetchers.web_search import WebSearchFallbackSource

        session_cache = SessionCache()
        all_sources: dict[str, BasePatentSource] = {
            "USPTO": PpubsSource(self._config, session_cache),
            "EPO_OPS": EpoOpsSource(self._config, session_cache),
            "BigQuery": BigQuerySource(self._config),
            "Espacenet": EspacenetSource(self._config),
            "WIPO_Scrape": WipoScrapeSource(self._config),
            "IP_Australia": IpAustraliaSource(self._config),
            "CIPO": CipoScrapeSource(self._config),
            "Google_Patents": GooglePatentsSource(self._config),
            "web_search": WebSearchFallbackSource(self._config),
        }

        ordered: list[BasePatentSource] = []
        for name in self._config.source_priority:
            src = all_sources.get(name)
            if src:
                ordered.append(src)
        # Add any sources not in priority list at end
        priority_names = set(self._config.source_priority)
        for name, src in all_sources.items():
            if name not in priority_names:
                ordered.append(src)
        return ordered

    def get_sources_for(self, patent: "CanonicalPatentId") -> list[BasePatentSource]:
        """Return sources that support this patent's jurisdiction, in priority order."""
        return [s for s in self._sources if s.can_fetch(patent)]

    # ------------------------------------------------------------------
    # Fetch single patent
    # ------------------------------------------------------------------

    async def fetch(
        self,
        patent: "CanonicalPatentId",
        output_dir: Path,
    ) -> OrchestratorResult:
        """Fetch a single patent, using cache if available."""
        # Cache hit
        cached = self._cache.lookup(patent.canonical)
        if cached and cached.is_complete:
            return OrchestratorResult(
                canonical_id=patent.canonical,
                success=True,
                cache_dir=cached.cache_dir,
                files=cached.files,
                metadata=cached.metadata,
                from_cache=True,
            )

        output_dir.mkdir(parents=True, exist_ok=True)
        sources = self.get_sources_for(patent)

        # Filter out web_search; it's only used as last resort
        structured = [s for s in sources if s.source_name != "web_search"]
        web_search = next((s for s in sources if s.source_name == "web_search"), None)

        all_attempts: list[SourceAttempt] = []
        all_pdfs: list[Path] = []
        all_txts: list[Path] = []
        all_images: list[str] = []
        best_metadata: PatentMetadata | None = None

        if self._config.fetch_all_sources:
            # Concurrent fetch from all structured sources
            tasks = [s.fetch(patent, output_dir) for s in structured]
            results: list[FetchResult] = await asyncio.gather(*tasks, return_exceptions=True)
            for r in results:
                if isinstance(r, Exception):
                    log.warning("Source raised exception: %s", r)
                    continue
                all_attempts.append(r.source_attempt)
                if r.source_attempt.success:
                    if r.pdf_path:
                        all_pdfs.append(r.pdf_path)
                    if r.txt_path:
                        all_txts.append(r.txt_path)
                    all_images.extend(r.image_urls)
                    if best_metadata is None and r.metadata:
                        best_metadata = r.metadata
        else:
            # Sequential: stop after first success
            for src in structured:
                r = await src.fetch(patent, output_dir)
                all_attempts.append(r.source_attempt)
                if r.source_attempt.success:
                    if r.pdf_path:
                        all_pdfs.append(r.pdf_path)
                    if r.txt_path:
                        all_txts.append(r.txt_path)
                    all_images.extend(r.image_urls)
                    if best_metadata is None and r.metadata:
                        best_metadata = r.metadata
                    break

        any_success = any(a.success for a in all_attempts)

        # Web search fallback
        if not any_success and web_search:
            r = await web_search.fetch(patent, output_dir)
            all_attempts.append(r.source_attempt)

        # Convert PDF to markdown if we got a PDF
        md_path: Path | None = None
        if all_pdfs and best_metadata:
            from patent_mcp.converters.pipeline import ConverterPipeline
            pipeline = ConverterPipeline(self._config)
            md_out = output_dir / f"{patent.canonical}.md"
            conv = pipeline.pdf_to_markdown(all_pdfs[0], md_out, best_metadata)
            if conv.success:
                md_path = conv.output_path

        # Build files dict
        files: dict[str, Path] = {}
        if all_pdfs:
            files["pdf"] = all_pdfs[0]
        if all_txts:
            files["txt"] = all_txts[0]
        if md_path:
            files["md"] = md_path

        # Store in cache
        if any_success and best_metadata:
            artifacts = ArtifactSet(
                pdf=all_pdfs[0] if all_pdfs else None,
                txt=all_txts[0] if all_txts else None,
                md=md_path,
                images=[],
            )
            try:
                self._cache.store(patent.canonical, artifacts, best_metadata, all_attempts)
            except Exception as e:
                log.warning("Cache store failed: %s", e)

        return OrchestratorResult(
            canonical_id=patent.canonical,
            success=any_success or bool(files),
            cache_dir=output_dir if files else None,
            files=files,
            metadata=best_metadata,
            sources=all_attempts,
        )

    # ------------------------------------------------------------------
    # Batch fetch
    # ------------------------------------------------------------------

    async def fetch_batch(
        self,
        patents: "list[CanonicalPatentId]",
        output_base_dir: Path,
        concurrency: int | None = None,
    ) -> list[OrchestratorResult]:
        """Fetch multiple patents concurrently."""
        limit = concurrency or self._config.concurrency
        semaphore = asyncio.Semaphore(limit)

        async def _bounded_fetch(patent: "CanonicalPatentId") -> OrchestratorResult:
            async with semaphore:
                out_dir = output_base_dir / patent.canonical
                try:
                    return await self.fetch(patent, out_dir)
                except Exception as e:
                    log.error("Batch fetch failed for %s: %s", patent.canonical, e)
                    return OrchestratorResult(
                        canonical_id=patent.canonical,
                        success=False,
                        error=str(e),
                    )

        results = await asyncio.gather(*[_bounded_fetch(p) for p in patents])
        return list(results)
