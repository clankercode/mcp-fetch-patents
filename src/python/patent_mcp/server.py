"""MCP server — fetch patents by ID, cache everything."""

from __future__ import annotations

import logging
import sys
import time
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Any

# Configure logging to stderr BEFORE any other imports that might log
logging.basicConfig(
    stream=sys.stderr,
    level=logging.INFO,
    format="%(asctime)s %(levelname)s %(name)s %(message)s",
)

log = logging.getLogger(__name__)

MAX_RESPONSE_TOKENS = 50_000  # ~200KB; truncate content exceeding this

# Module-level imports so tests can patch patent_mcp.server.PatentCache etc.
from patent_mcp.cache import PatentCache  # noqa: E402
from patent_mcp.config import get_config, load_config  # noqa: E402
from patent_mcp.fetchers.orchestrator import FetcherOrchestrator  # noqa: E402
from patent_mcp.id_canon import canonicalize  # noqa: E402
from patent_mcp.journal import ActivityJournal  # noqa: E402


# ---------------------------------------------------------------------------
# MCP Tool I/O types
# ---------------------------------------------------------------------------


@dataclass
class PatentFetchResult:
    patent_id: str  # raw input
    canonical_id: str  # normalized
    status: str  # "fetched" | "cached" | "partial" | "error"
    files: dict[str, str]  # format → absolute path string
    metadata: dict[str, Any]
    sources: list[dict]
    fetch_duration_ms: float
    error: str | None


@dataclass
class FetchSummary:
    total: int
    success: int
    cached: int
    errors: int
    total_duration_ms: float


# ---------------------------------------------------------------------------
# Token budget
# ---------------------------------------------------------------------------


def _estimate_tokens(text: str) -> int:
    """Rough token estimate: 4 chars ≈ 1 token."""
    return len(text) // 4


def _truncate_if_needed(
    result_dict: dict, max_tokens: int = MAX_RESPONSE_TOKENS
) -> dict:
    """Truncate large text fields (abstract, title) to fit token budget.
    File paths are NEVER truncated."""
    import json

    result_str = json.dumps(result_dict)
    if _estimate_tokens(result_str) <= max_tokens:
        return result_dict

    # Truncate abstracts in each result
    for r in result_dict.get("results", []):
        meta = r.get("metadata", {})
        abstract = meta.get("abstract") or ""
        if len(abstract) > 500:
            meta["abstract"] = abstract[:500] + "... [truncated]"
        title = meta.get("title") or ""
        if len(title) > 200:
            meta["title"] = title[:200] + "..."

    return result_dict


# ---------------------------------------------------------------------------
# Server construction
# ---------------------------------------------------------------------------


def _build_server(config=None):
    """Build and return the FastMCP server instance."""
    from mcp.server.fastmcp import FastMCP

    cfg = config or get_config()
    cache = PatentCache(cfg)
    orchestrator = FetcherOrchestrator(cfg, cache=cache)
    journal = ActivityJournal(cfg.activity_journal)

    mcp = FastMCP(
        "patent-mcp-server",
        instructions=(
            "Fetch patents by ID number. Supports US, EP, WO, JP, CN, KR, AU, CA, NZ, BR, IN, "
            "and other ISO jurisdictions. Batch requests are explicitly encouraged — pass multiple "
            "IDs in a single call. Returns file paths and metadata, not raw content."
        ),
    )

    # ------------------------------------------------------------------
    # Tool: fetch_patents
    # ------------------------------------------------------------------

    @mcp.tool()
    async def fetch_patents(
        patent_ids: list[str],
        formats: list[str] | None = None,
        force_refresh: bool = False,
        postprocess_query: str | None = None,
    ) -> dict:
        """Fetch one or more patents by ID. Returns file paths + metadata.

        Args:
            patent_ids: Patent IDs in any common format (e.g. "US7654321", "EP1234567B1",
                "WO2024/123456", "https://patents.google.com/patent/US7654321").
                Batch requests are encouraged — pass multiple IDs.
            formats: Formats to retrieve. Defaults to ["pdf", "txt", "md"].
            force_refresh: Skip cache and re-fetch from sources.
            postprocess_query: Query for post-processing (stored for v2; no-op in v1).
        """
        if postprocess_query:
            log.warning(
                "postprocess_query not yet implemented in v1; stored for future use. "
                "Value: %r",
                postprocess_query,
            )

        if not patent_ids:
            return {
                "results": [],
                "summary": asdict(FetchSummary(0, 0, 0, 0, 0.0)),
                "isError": False,
            }

        start_total = time.monotonic()
        output_base = Path(cfg.cache_local_dir)

        # Canonicalize all IDs
        canonicals = [canonicalize(pid) for pid in patent_ids]

        # Fetch (with optional force_refresh by clearing cache lookup)
        if force_refresh:
            # Temporarily override cache.lookup to always return None
            original_lookup = cache.lookup
            cache.lookup = lambda cid: None  # type: ignore[assignment]
            try:
                batch_results = await orchestrator.fetch_batch(canonicals, output_base)
            finally:
                cache.lookup = original_lookup
        else:
            batch_results = await orchestrator.fetch_batch(canonicals, output_base)

        results: list[PatentFetchResult] = []
        n_success = n_cached = n_errors = 0

        for raw_id, canon, orc_result in zip(patent_ids, canonicals, batch_results):
            meta_dict: dict[str, Any] = {}
            if orc_result.metadata:
                m = orc_result.metadata
                meta_dict = {
                    "title": m.title,
                    "abstract": m.abstract,
                    "inventors": m.inventors,
                    "assignee": m.assignee,
                    "filing_date": m.filing_date,
                    "publication_date": m.publication_date,
                    "grant_date": m.grant_date,
                    "fetched_at": m.fetched_at,
                    "legal_status": m.legal_status,
                    "jurisdiction": m.jurisdiction,
                    "doc_type": m.doc_type,
                }

            if postprocess_query:
                meta_dict["postprocess_query"] = postprocess_query
                meta_dict["postprocess_query_note"] = (
                    "postprocess_query not yet implemented in v1; stored for future use"
                )

            files_str: dict[str, str] = {
                fmt: str(path) for fmt, path in orc_result.files.items()
            }

            sources = [
                {
                    "source": a.source,
                    "success": a.success,
                    "elapsed_ms": a.elapsed_ms,
                    "error": a.error,
                }
                for a in orc_result.sources
            ]

            if orc_result.from_cache:
                status = "cached"
                n_cached += 1
                n_success += 1
            elif orc_result.success:
                status = "fetched"
                n_success += 1
            elif orc_result.files:
                status = "partial"
                n_success += 1
            else:
                status = "error"
                n_errors += 1

            results.append(
                PatentFetchResult(
                    patent_id=raw_id,
                    canonical_id=orc_result.canonical_id,
                    status=status,
                    files=files_str,
                    metadata=meta_dict,
                    sources=sources,
                    fetch_duration_ms=0.0,
                    error=orc_result.error,
                )
            )

        total_ms = (time.monotonic() - start_total) * 1000
        response = {
            "results": [asdict(r) for r in results],
            "summary": asdict(
                FetchSummary(
                    total=len(results),
                    success=n_success,
                    cached=n_cached,
                    errors=n_errors,
                    total_duration_ms=total_ms,
                )
            ),
        }
        journal.log_fetch(patent_ids, response["summary"])
        response["isError"] = False
        return _truncate_if_needed(response)

    # ------------------------------------------------------------------
    # Tool: list_cached_patents
    # ------------------------------------------------------------------

    @mcp.tool()
    def list_cached_patents() -> dict:
        """List all cached patents."""
        entries = cache.list_all()
        journal.log_list(len(entries))
        return {
            "patents": [
                {"canonical_id": e.canonical_id, "cache_dir": str(e.cache_dir)}
                for e in entries
            ],
            "count": len(entries),
            "isError": False,
        }

    # ------------------------------------------------------------------
    # Tool: get_patent_metadata
    # ------------------------------------------------------------------

    @mcp.tool()
    def get_patent_metadata(patent_ids: list[str]) -> dict:
        """Return cached metadata for patents (no network call).

        Args:
            patent_ids: One or more patent IDs to look up.
        """
        results = []
        for raw_id in patent_ids:
            canon = canonicalize(raw_id)
            hit = cache.lookup(canon.canonical)
            if hit:
                m = hit.metadata
                results.append(
                    {
                        "patent_id": raw_id,
                        "canonical_id": canon.canonical,
                        "metadata": {
                            "title": m.title,
                            "abstract": m.abstract,
                            "inventors": m.inventors,
                            "assignee": m.assignee,
                            "filing_date": m.filing_date,
                            "publication_date": m.publication_date,
                            "jurisdiction": m.jurisdiction,
                            "doc_type": m.doc_type,
                        },
                    }
                )
            else:
                results.append(
                    {
                        "patent_id": raw_id,
                        "canonical_id": canon.canonical,
                        "metadata": None,
                    }
                )

        found = sum(1 for r in results if r["metadata"] is not None)
        missing = len(results) - found
        journal.log_metadata(patent_ids, found, missing)
        return {"results": results, "isError": False}

    return mcp


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------


def run_server(cache_dir: str | None = None, log_level: str = "info") -> None:
    """Start the MCP server on stdin/stdout."""
    numeric_level = getattr(logging, log_level.upper(), logging.INFO)
    logging.getLogger().setLevel(numeric_level)

    overrides: dict = {}
    if cache_dir:
        overrides["cache_local_dir"] = Path(cache_dir)

    cfg = load_config(overrides=overrides)
    mcp = _build_server(config=cfg)
    mcp.run(transport="stdio")
