"""Deep patent search MCP server — tools for the patent-searcher OpenCode agent."""

from __future__ import annotations

import logging
import sys
import threading
from pathlib import Path
from typing import Any

logging.basicConfig(
    stream=sys.stderr,
    level=logging.INFO,
    format="%(asctime)s %(levelname)s %(name)s %(message)s",
)
log = logging.getLogger(__name__)

from dataclasses import asdict

from mcp.server.fastmcp import FastMCP  # noqa: E402

from patent_mcp.utils import now_iso

mcp = FastMCP("patent-search")

# ---------------------------------------------------------------------------
# Lazy imports — avoid loading heavy deps at import time
# ---------------------------------------------------------------------------

_session_manager = None
_orchestrator = None


def _get_session_manager():
    global _session_manager
    if _session_manager is None:
        from patent_mcp.search.session_manager import SessionManager

        _session_manager = SessionManager()
    return _session_manager


def _get_orchestrator():
    global _orchestrator
    if _orchestrator is None:
        from patent_mcp.fetchers.orchestrator import FetcherOrchestrator

        _orchestrator = FetcherOrchestrator(_get_config())
    return _orchestrator


def _get_config():
    from patent_mcp.config import get_config

    return get_config()


# ---------------------------------------------------------------------------
# Singletons for browser infrastructure (lazy init)
# ---------------------------------------------------------------------------

_browser_manager_lock = threading.Lock()
_browser_managers: dict[str, object] = {}  # profile_name → BrowserManager


def _get_browser_manager(profile_name: str | None = None):
    """Return a shared BrowserManager for the given profile, creating it on first call."""
    cfg = _get_config()
    name = profile_name or cfg.search_browser_default_profile

    if name in _browser_managers:
        return _browser_managers[name]

    with _browser_manager_lock:
        if name in _browser_managers:
            return _browser_managers[name]

        from patent_mcp.search.profile_manager import ProfileManager
        from patent_mcp.search.browser_manager import BrowserManager

        pm = ProfileManager(cfg.search_browser_profiles_dir)
        bm = BrowserManager(
            profile_manager=pm,
            profile_name=name,
            headless=cfg.search_browser_headless,
            idle_timeout=cfg.search_browser_idle_timeout,
            timeout=cfg.search_browser_timeout * 1000,  # seconds → ms
        )
        _browser_managers[name] = bm
        return bm


# ---------------------------------------------------------------------------
# Helper: run async in a sync context (MCP tools are sync)
# ---------------------------------------------------------------------------


def _run(coro):
    import asyncio

    try:
        loop = asyncio.get_event_loop()
        if loop.is_running():
            import concurrent.futures

            with concurrent.futures.ThreadPoolExecutor() as pool:
                future = pool.submit(asyncio.run, coro)
                return future.result()
        return loop.run_until_complete(coro)
    except RuntimeError:
        return asyncio.run(coro)


# ---------------------------------------------------------------------------
# Session tools
# ---------------------------------------------------------------------------


@mcp.tool()
def patent_status() -> dict[str, Any]:
    """Check server health, configured API keys, cache stats, and tool availability.

    Use this to diagnose why searches return no results.

    Returns:
        API key status (set/not set), cache directory and count, browser availability,
        converter tools available, and session directory.
    """
    import shutil

    cfg = _get_config()

    api_keys = {
        "serpapi_key": "set" if cfg.serpapi_key else "not set",
        "epo_client_id": "set" if cfg.epo_client_id else "not set",
        "epo_client_secret": "set" if cfg.epo_client_secret else "not set",
        "lens_api_key": "set" if cfg.lens_api_key else "not set",
        "bing_key": "set" if cfg.bing_key else "not set",
    }

    cache_dir = Path(cfg.cache_local_dir)
    cached_patents = 0
    if cache_dir.exists():
        cached_patents = sum(1 for _ in cache_dir.iterdir() if _.is_dir())

    browser_available = (
        shutil.which("chromium") is not None or shutil.which("chrome") is not None
    )
    try:
        from playwright.sync_api import sync_playwright

        browser_available = True
    except ImportError:
        pass

    converters_available: list[str] = []
    for conv in cfg.converters_order:
        if conv in cfg.converters_disabled:
            continue
        if conv == "pymupdf4llm":
            try:
                import pymupdf4llm

                converters_available.append(conv)
            except ImportError:
                pass
        elif conv == "pdfplumber":
            try:
                import pdfplumber

                converters_available.append(conv)
            except ImportError:
                pass
        elif conv == "pdftotext":
            if shutil.which("pdftotext"):
                converters_available.append(conv)
        else:
            converters_available.append(conv)

    sm = _get_session_manager()
    sessions_count = len(sm.list_sessions(limit=9999))

    return {
        "status": "ok",
        "api_keys": api_keys,
        "cache": {
            "directory": str(cache_dir),
            "patent_count": cached_patents,
        },
        "browser_available": browser_available,
        "converters_available": converters_available,
        "sessions": {
            "directory": str(sm.sessions_dir),
            "count": sessions_count,
        },
        "search_backend_default": cfg.search_backend_default,
        "isError": False,
    }


@mcp.tool()
def patent_session_create(
    topic: str,
    prior_art_cutoff: str | None = None,
    notes: str = "",
) -> dict[str, Any]:
    """Create a new patent research session.

    Args:
        topic: Descriptive name for this research session (e.g. "prior-art-wireless-charging-2026")
        prior_art_cutoff: Optional date cutoff (ISO format YYYY-MM-DD). Patents before this date
                          may qualify as prior art for the topic you're researching.
        notes: Optional initial researcher notes.

    Returns:
        session_id, topic, and sessions_dir path.
    """
    sm = _get_session_manager()
    session = sm.create_session(
        topic=topic, prior_art_cutoff=prior_art_cutoff, notes=notes
    )
    sm.save_session(session)
    return {
        "session_id": session.session_id,
        "topic": session.topic,
        "created_at": session.created_at,
        "sessions_dir": str(sm.sessions_dir),
        "message": f"Session created. Use session_id='{session.session_id}' in search calls to auto-save results.",
        "isError": False,
    }


@mcp.tool()
def patent_session_load(session_id: str) -> dict[str, Any]:
    """Load a saved research session and return its full state.

    Args:
        session_id: The session ID to load (from patent_session_list or a prior session_create call).

    Returns:
        Full session data including all queries run, patents found, notes, etc.
    """
    sm = _get_session_manager()
    try:
        session = sm.load_session(session_id)
        from patent_mcp.search.session_manager import _session_to_dict

        return {**_session_to_dict(session), "isError": False}
    except FileNotFoundError:
        return {
            "error": f"Session '{session_id}' not found. Use patent_session_list to see available sessions.",
            "isError": True,
        }


@mcp.tool()
def patent_session_list(limit: int = 20) -> dict[str, Any]:
    """List all saved patent research sessions, most recent first.

    Args:
        limit: Maximum number of sessions to return (default 20).

    Returns:
        List of session summaries with id, topic, dates, query count, and patent count.
    """
    sm = _get_session_manager()
    summaries = sm.list_sessions(limit=limit)
    return {
        "sessions": [
            {
                "session_id": s.session_id,
                "topic": s.topic,
                "created_at": s.created_at,
                "modified_at": s.modified_at,
                "query_count": s.query_count,
                "patent_count": s.patent_count,
            }
            for s in summaries
        ],
        "total": len(summaries),
        "isError": False,
    }


@mcp.tool()
def patent_session_note(session_id: str, note: str) -> dict[str, Any]:
    """Add a researcher note to a session.

    Args:
        session_id: The session to update.
        note: Text note to append (e.g. observations, next steps, hypotheses).

    Returns:
        Confirmation with updated modified_at timestamp.
    """
    sm = _get_session_manager()
    try:
        sm.add_note(session_id, note)
        return {"session_id": session_id, "status": "note added", "isError": False}
    except FileNotFoundError:
        return {"error": f"Session '{session_id}' not found.", "isError": True}


@mcp.tool()
def patent_session_annotate(
    session_id: str,
    patent_id: str,
    annotation: str,
    relevance: str = "high",
) -> dict[str, Any]:
    """Annotate a patent result in a session with relevance and notes.

    Args:
        session_id: The session containing the patent.
        patent_id: The patent ID to annotate (e.g. "US10123456B2").
        annotation: Researcher note about why this patent is relevant/irrelevant.
        relevance: One of: "high", "medium", "low", "irrelevant".

    Returns:
        Confirmation.
    """
    sm = _get_session_manager()
    try:
        sm.annotate_patent(session_id, patent_id, annotation, relevance)
        return {
            "session_id": session_id,
            "patent_id": patent_id,
            "relevance": relevance,
            "status": "annotated",
            "isError": False,
        }
    except FileNotFoundError:
        return {"error": f"Session '{session_id}' not found.", "isError": True}


@mcp.tool()
def patent_session_export(
    session_id: str,
    output_path: str | None = None,
) -> dict[str, Any]:
    """Export a research session to a readable Markdown report.

    Args:
        session_id: The session to export.
        output_path: Optional output file path. Defaults to .patent-sessions/{session_id}-report.md

    Returns:
        Path to the generated report file.
    """
    sm = _get_session_manager()
    try:
        out_path = Path(output_path) if output_path else None
        report_path = sm.export_markdown(session_id, out_path)
        return {"report_path": str(report_path), "status": "exported", "isError": False}
    except FileNotFoundError:
        return {"error": f"Session '{session_id}' not found.", "isError": True}


@mcp.tool()
def patent_session_delete(session_id: str) -> dict[str, Any]:
    """Delete a patent research session and all its data.

    Args:
        session_id: ID of the session to delete.

    Returns:
        Confirmation of deletion or not-found status.
    """
    sm = _get_session_manager()
    try:
        deleted = sm.delete_session(session_id)
        if deleted:
            return {
                "status": "deleted",
                "session_id": session_id,
                "isError": False,
            }
        return {
            "status": "not_found",
            "message": f"Session '{session_id}' not found.",
            "session_id": session_id,
            "isError": False,
        }
    except ValueError as e:
        return {"error": str(e), "isError": True}


@mcp.tool()
def patent_quick_search(
    description: str,
    max_results: int = 10,
    prior_art_cutoff: str | None = None,
    backend: str = "auto",
) -> dict[str, Any]:
    """One-shot patent search: creates a session, runs natural-language search,
    and returns a summary. Combines patent_session_create + patent_search_natural
    into a single call.

    Args:
        description: Natural-language description of what you're looking for.
        max_results: Max results to return. Default: 10.
        prior_art_cutoff: ISO date (YYYY-MM-DD). If set, highlights patents before
                          this date as prior art.
        backend: Search backend: "browser", "serpapi", or "auto".

    Returns:
        Session ID, search results, and planner output.
    """
    if not description or not description.strip():
        return {
            "error": "description is required and must be non-empty",
            "isError": True,
        }

    sm = _get_session_manager()
    session = sm.create_session(description, prior_art_cutoff, "")
    session_id = session.session_id

    result = patent_search_natural(
        description=description,
        date_cutoff=prior_art_cutoff,
        max_results=max_results,
        session_id=session_id,
        backend=backend,
    )

    return {
        "session_id": session_id,
        "topic": description,
        "backend": backend,
        "prior_art_cutoff": prior_art_cutoff,
        "elapsed_ms": result.get("elapsed_ms", 0),
        "total_found": result.get("total_found", 0),
        "planner": result.get("planner", {}),
        "results": result.get("results", []),
        "isError": False,
    }


@mcp.tool()
def patent_search_natural(
    description: str,
    date_cutoff: str | None = None,
    jurisdictions: list[str] | None = None,
    session_id: str | None = None,
    max_results: int = 25,
    backend: str = "auto",
    profile_name: str | None = None,
    enrich_top_n: int | None = None,
    debug: bool = False,
) -> dict[str, Any]:
    """Search for patents using a natural language description.

    Expands your description into multiple query variants using keyword/synonym
    expansion, runs them against Google Patents (browser and/or SerpAPI), merges
    and reranks results, and optionally enriches the top hits with full metadata
    from the patent fetch pipeline.

    Args:
        description: Natural language description of the technology or invention to search for.
                     E.g.: "wireless charging that works through metal objects"
        date_cutoff: Optional date cutoff for prior art (ISO format YYYY-MM-DD).
                     Only returns patents filed/published BEFORE this date.
        jurisdictions: Optional list of jurisdictions (e.g. ["US", "EP", "WO"]).
                       Default: searches all.
        session_id: Optional session ID to automatically save results.
        max_results: Maximum results to return after dedup/ranking (default 25).
        backend: Search backend: "browser" (Playwright), "serpapi", or "auto" (default).
                 "auto" tries browser first, falls back to SerpAPI.
        profile_name: Browser profile name for Playwright backend (default: config value).
        enrich_top_n: Enrich top N results with full metadata via fetch pipeline.
                      Default: from config (typically 5). Set to 0 to disable.
        debug: If True, save debug HTML snapshots of search result pages.

    Returns:
        Ranked list of matching patents with metadata, planner output, and queries run.
    """
    import time as _time

    start = _time.monotonic()

    cfg = _get_config()
    if enrich_top_n is None:
        enrich_top_n = cfg.search_enrich_top_n

    # Resolve "auto": determine preferred backend but keep "auto" semantics
    # so fallback from browser→serpapi works.
    effective_backend = backend
    if effective_backend == "auto":
        effective_backend = cfg.search_backend_default
        if effective_backend == "auto":
            effective_backend = "browser"  # ultimate default

    # Phase 1: Plan — expand description into query variants
    from patent_mcp.search.planner import NaturalLanguagePlanner

    planner = NaturalLanguagePlanner()
    intent = planner.plan(description, date_cutoff, jurisdictions)

    # Phase 2: Execute queries across backends
    hits_by_query: dict[str, list] = {}
    queries_run: list[dict[str, Any]] = []
    browser_failed = False

    # Browser backend
    if effective_backend == "browser":
        try:
            from patent_mcp.search.google_browser_backend import (
                GooglePatentsBrowserBackend,
                GoogleSearchConfig,
            )

            bm = _get_browser_manager(profile_name)
            search_cfg = GoogleSearchConfig(
                max_pages=cfg.search_browser_max_pages,
                timeout_ms=cfg.search_browser_timeout * 1000,
                debug_html_dir=Path(cfg.search_browser_debug_html_dir)
                if (debug and cfg.search_browser_debug_html_dir)
                else None,
            )
            gb = GooglePatentsBrowserBackend(bm, search_cfg)

            for variant in intent.query_variants:
                try:
                    hits = gb.search(
                        query=variant.query,
                        date_before=date_cutoff,
                    )
                    hits_by_query[variant.query] = hits
                    queries_run.append(
                        {
                            "source": "Google_Patents_Browser",
                            "query": variant.query,
                            "variant_type": variant.variant_type,
                            "result_count": len(hits),
                        }
                    )
                except Exception as e:
                    log.warning(
                        "Browser search failed for variant '%s': %s",
                        variant.variant_type,
                        e,
                    )
                    queries_run.append(
                        {
                            "source": "Google_Patents_Browser",
                            "query": variant.query,
                            "variant_type": variant.variant_type,
                            "result_count": 0,
                            "error": str(e),
                        }
                    )
        except Exception as e:
            log.warning("Browser backend unavailable: %s", e)
            browser_failed = True

    # SerpAPI fallback — triggered when backend is "serpapi", or "auto" and browser failed/empty
    if effective_backend == "serpapi" or (
        backend == "auto" and (browser_failed or not hits_by_query)
    ):
        if cfg.serpapi_key:
            from patent_mcp.search.searchers import SerpApiGooglePatentsBackend

            serp = SerpApiGooglePatentsBackend(api_key=cfg.serpapi_key)
            for variant in intent.query_variants:
                if variant.query in hits_by_query and hits_by_query[variant.query]:
                    continue  # already have results for this query
                try:
                    hits = _run(
                        serp.search(
                            query=variant.query,
                            date_to=date_cutoff,
                            max_results=max_results,
                        )
                    )
                    hits_by_query[variant.query] = hits
                    queries_run.append(
                        {
                            "source": "Google_Patents_SerpAPI",
                            "query": variant.query,
                            "variant_type": variant.variant_type,
                            "result_count": len(hits),
                        }
                    )
                except Exception as e:
                    log.warning(
                        "SerpAPI search failed for variant '%s': %s",
                        variant.variant_type,
                        e,
                    )
        else:
            if not hits_by_query:
                log.warning(
                    "No search backends available (browser failed, no SerpAPI key)"
                )

    # Phase 3: Rank
    from patent_mcp.search.ranking import SearchRanker

    ranker = SearchRanker()
    scored = ranker.rank(hits_by_query, intent)

    # Limit to max_results
    scored = scored[:max_results]

    # Phase 4: Enrich top N with full metadata
    enriched_ids: list[str] = []
    if enrich_top_n > 0 and scored:
        enriched_ids = _enrich_hits(scored[:enrich_top_n])

    # Phase 5: Save to session
    all_hits = [s.hit for s in scored]
    if session_id and all_hits:
        _save_to_session(
            session_id,
            queries_run,
            all_hits,
            metadata={
                "search_mode": backend,
                "planner_concepts": intent.concepts,
                "planner_synonyms": {k: v for k, v in intent.synonyms.items()},
                "query_variants": [v.query for v in intent.query_variants],
            },
        )

    elapsed_ms = int((_time.monotonic() - start) * 1000)

    return {
        "query": description,
        "date_cutoff": date_cutoff,
        "backend": backend,
        "planner": {
            "concepts": intent.concepts,
            "synonyms_expanded": list(intent.synonyms.keys()),
            "query_variant_count": len(intent.query_variants),
            "rationale": intent.rationale,
        },
        "queries_run": queries_run,
        "total_found": len(scored),
        "enriched_ids": enriched_ids,
        "results": [
            {
                **_hit_to_dict(s.hit),
                "score": round(s.score, 2),
                "query_matches": s.query_matches,
            }
            for s in scored
        ],
        "elapsed_ms": elapsed_ms,
        "isError": False,
    }


@mcp.tool()
def patent_search_structured(
    query: str,
    sources: list[str] | None = None,
    date_from: str | None = None,
    date_to: str | None = None,
    session_id: str | None = None,
    max_results: int = 25,
) -> dict[str, Any]:
    """Run an expert-syntax Boolean patent query against one or more sources.

    Supports full patent database query syntax:
    - USPTO: TTL/(wireless ADJ charging) AND CPC/H02J50 AND APD/20000101->20191231
    - EPO/Espacenet: ti="wireless charging" AND ic="H02J50/*"
    - Google Patents: title:"wireless charging" claims:"inductive" before:2020

    Args:
        query: Boolean query string with field codes. The same query is sent to all
               selected sources, so prefer neutral terminology over source-specific syntax.
        sources: List of sources to query. Options: "USPTO", "EPO_OPS", "Google_Patents".
                 Default: all available sources.
        date_from: Optional start date filter (ISO format YYYY-MM-DD).
        date_to: Optional end date filter (ISO format YYYY-MM-DD).
        session_id: Optional session ID to automatically save results.
        max_results: Maximum results per source (default 25).

    Returns:
        Combined results from all queried sources, with source attribution.
    """
    if sources is None:
        sources = ["USPTO", "EPO_OPS", "Google_Patents"]

    cfg = _get_config()
    all_results = []
    queries_run = []

    if "USPTO" in sources:
        from patent_mcp.search.searchers import UsptoTextSearchBackend

        backend = UsptoTextSearchBackend()
        hits = _run(
            backend.search(
                query=query,
                date_from=date_from.replace("-", "") if date_from else None,
                date_to=date_to.replace("-", "") if date_to else None,
                max_results=max_results,
            )
        )
        all_results.extend(hits)
        queries_run.append(
            {"source": "USPTO", "query": query, "result_count": len(hits)}
        )

    if "EPO_OPS" in sources:
        from patent_mcp.search.searchers import EpoOpsSearchBackend

        epo = EpoOpsSearchBackend(
            client_id=cfg.epo_client_id,
            client_secret=cfg.epo_client_secret,
        )
        hits = _run(
            epo.search(
                query=query,
                date_from=date_from,
                date_to=date_to,
                max_results=max_results,
            )
        )
        all_results.extend(hits)
        queries_run.append(
            {"source": "EPO_OPS", "query": query, "result_count": len(hits)}
        )

    if "Google_Patents" in sources and cfg.serpapi_key:
        from patent_mcp.search.searchers import SerpApiGooglePatentsBackend

        gp = SerpApiGooglePatentsBackend(api_key=cfg.serpapi_key)
        hits = _run(gp.search(query=query, date_to=date_to, max_results=max_results))
        all_results.extend(hits)
        queries_run.append(
            {"source": "Google_Patents", "query": query, "result_count": len(hits)}
        )

    # Deduplicate
    seen: set[str] = set()
    deduped = []
    for h in all_results:
        if h.patent_id not in seen:
            seen.add(h.patent_id)
            deduped.append(h)

    if session_id and deduped:
        _save_to_session(session_id, queries_run, deduped)

    return {
        "query": query,
        "sources_searched": [q["source"] for q in queries_run],
        "queries_run": queries_run,
        "total_found": len(deduped),
        "results": [_hit_to_dict(h) for h in deduped],
        "isError": False,
    }


@mcp.tool()
def patent_citation_chain(
    patent_id: str,
    direction: str = "backward",
    depth: int = 1,
    session_id: str | None = None,
) -> dict[str, Any]:
    """Follow patent citations forward or backward to discover related patents.

    Backward citations: what did this patent cite? (finds older prior art)
    Forward citations: who cited this patent? (finds similar later work)

    Args:
        patent_id: The seed patent ID (e.g. "US10123456B2", "EP3456789A1").
        direction: "backward" (what it cited), "forward" (who cited it), or "both".
        depth: How many levels of citations to follow (1-3). Default 1.
               Depth 2 finds patents that were cited by the patents that cited your seed.
        session_id: Optional session ID to save the citation tree.

    Returns:
        Citation tree with patent metadata at each level.
    """
    cfg = _get_config()
    from patent_mcp.search.searchers import EpoOpsSearchBackend

    epo = EpoOpsSearchBackend(
        client_id=cfg.epo_client_id,
        client_secret=cfg.epo_client_secret,
    )

    tree: dict[str, Any] = {
        "seed": patent_id,
        "direction": direction,
        "depth": depth,
        "citations": {},
    }

    def _fetch_level(pid: str, dir_: str, current_depth: int) -> list[str]:
        if current_depth <= 0:
            return []
        cited = _run(epo.get_citations(pid, direction=dir_))
        return cited

    directions = []
    if direction in ("backward", "both"):
        directions.append("backward")
    if direction in ("forward", "both"):
        directions.append("forward")

    for dir_ in directions:
        level_1 = _fetch_level(patent_id, dir_, depth)
        tree["citations"][dir_] = {
            "level_1": level_1,
        }
        if depth >= 2 and level_1:
            level_2: list[str] = []
            for pid in level_1[:10]:  # limit to avoid explosion
                more = _fetch_level(pid, dir_, depth - 1)
                level_2.extend(more)
            # Deduplicate
            seen: set[str] = set(level_1)
            level_2 = [p for p in level_2 if p not in seen]
            tree["citations"][dir_]["level_2"] = level_2

    if session_id:
        try:
            sm = _get_session_manager()
            with sm.update_session(session_id) as session:
                if session.citation_chains is None:
                    session.citation_chains = {}
                session.citation_chains[patent_id] = tree["citations"]
        except Exception as e:
            log.warning("Failed to save citation chain to session: %s", e)

    return {**tree, "isError": False}


@mcp.tool()
def patent_classification_search(
    code: str,
    include_subclasses: bool = True,
    date_from: str | None = None,
    date_to: str | None = None,
    session_id: str | None = None,
    max_results: int = 25,
) -> dict[str, Any]:
    """Search patents by IPC or CPC classification code.

    This is often the most effective search method for niche technologies because it
    finds ALL patents in a technical area regardless of the exact keywords used.

    Args:
        code: IPC/CPC classification code (e.g. "H02J50", "H02J50/10", "G06F17").
              Use broader codes (e.g. "H02J50") to include all subclasses.
        include_subclasses: If True, includes all sub-codes under this code (default True).
                            E.g., "H02J50" includes H02J50/10, H02J50/20, etc.
        date_from: Optional start date filter (ISO format YYYY-MM-DD).
        date_to: Optional end date filter (ISO format YYYY-MM-DD).
        session_id: Optional session ID to save results.
        max_results: Maximum results to return (default 25).

    Returns:
        Patents in the specified classification, plus breakdown by sub-class.
    """
    cfg = _get_config()
    from patent_mcp.search.searchers import EpoOpsSearchBackend

    epo = EpoOpsSearchBackend(
        client_id=cfg.epo_client_id,
        client_secret=cfg.epo_client_secret,
    )
    hits = _run(
        epo.search_by_classification(
            cpc_code=code,
            include_subclasses=include_subclasses,
            date_from=date_from,
            date_to=date_to,
            max_results=max_results,
        )
    )

    queries_run = [
        {"source": "EPO_OPS", "code": code, "include_subclasses": include_subclasses}
    ]

    if session_id and hits:
        _save_to_session(session_id, queries_run, hits)
        try:
            sm = _get_session_manager()
            with sm.update_session(session_id) as session:
                if code not in session.classifications_explored:
                    session.classifications_explored.append(code)
        except Exception:
            pass

    return {
        "code": code,
        "include_subclasses": include_subclasses,
        "date_from": date_from,
        "date_to": date_to,
        "total_found": len(hits),
        "results": [_hit_to_dict(h) for h in hits],
        "isError": False,
    }


@mcp.tool()
def patent_family_search(
    patent_id: str,
    session_id: str | None = None,
) -> dict[str, Any]:
    """Find all family members of a patent across jurisdictions.

    Patent families are groups of related patents for the same invention, filed in
    different countries. Finding family members helps you:
    - Determine the scope of protection in different jurisdictions
    - Find earlier priority dates
    - Find different claim scopes (often countries have different claim breadth)

    Args:
        patent_id: The patent ID to find family members for (e.g. "US10123456B2").
        session_id: Optional session ID to save the family data.

    Returns:
        List of family members with jurisdiction, publication number, and dates.
    """
    cfg = _get_config()
    from patent_mcp.search.searchers import EpoOpsSearchBackend

    epo = EpoOpsSearchBackend(
        client_id=cfg.epo_client_id,
        client_secret=cfg.epo_client_secret,
    )
    family_members = _run(epo.get_family(patent_id))

    if session_id and family_members:
        try:
            sm = _get_session_manager()
            with sm.update_session(session_id) as session:
                if session.patent_families is None:
                    session.patent_families = {}
                session.patent_families[patent_id] = [
                    m.get("patent_id", "") for m in family_members
                ]
        except Exception as e:
            log.warning("Failed to save family data to session: %s", e)

    return {
        "patent_id": patent_id,
        "family_size": len(family_members),
        "members": family_members,
        "isError": False,
    }


@mcp.tool()
def patent_suggest_queries(
    topic: str,
    context: str = "",
    prior_art_cutoff: str | None = None,
) -> dict[str, Any]:
    """Generate search strategy suggestions for a patent research topic.

    This tool brainstorms search approaches WITHOUT running them. It now uses the
    NL planner to generate concrete query variants alongside general strategy advice.

    Args:
        topic: The technology or invention to research (natural language description).
        context: Optional additional context (e.g., "looking for prior art for a 2019 patent",
                 "historical patents from 1900-1950", "specifically looking in medical devices").
        prior_art_cutoff: Optional date for prior art (ISO format YYYY-MM-DD).

    Returns:
        A structured set of search strategy recommendations plus concrete query variants.
    """
    # Use the planner to generate concrete query variants
    from patent_mcp.search.planner import NaturalLanguagePlanner

    planner = NaturalLanguagePlanner()
    intent = planner.plan(topic, date_cutoff=prior_art_cutoff)

    suggestions = {
        "topic": topic,
        "context": context,
        "prior_art_cutoff": prior_art_cutoff,
        "planner_output": {
            "concepts": intent.concepts,
            "synonyms": intent.synonyms,
            "rationale": intent.rationale,
            "query_variants": [
                {
                    "query": v.query,
                    "type": v.variant_type,
                    "rationale": v.rationale,
                }
                for v in intent.query_variants
            ],
        },
        "strategy": {
            "step_1_natural_search": {
                "description": "Run patent_search_natural with the query variants above",
                "action": f'patent_search_natural(description="{topic[:80]}", backend="auto")',
            },
            "step_2_classification": {
                "description": "Find IPC/CPC class codes — searches by class find patents regardless of keyword",
                "action": "Use patent_classification_search with codes from the relevant technology area",
                "tip": "Start with a broad code like 'H02J' and explore subclasses",
            },
            "step_3_citation_chain": {
                "description": "After finding any relevant patent, follow its citations",
                "action": "Use patent_citation_chain on the most relevant results (direction='both', depth=2)",
                "why": "The best prior art is often found 1-2 hops away in citation chains",
            },
        },
    }

    if prior_art_cutoff:
        suggestions["strategy"]["prior_art_notes"] = {
            "cutoff_date": prior_art_cutoff,
            "reminder": f"Search for patents filed/published BEFORE {prior_art_cutoff}",
            "tip": f"A patent published after {prior_art_cutoff} can still be prior art if its application was filed before that date",
        }

    suggestions["isError"] = False
    return suggestions


# ---------------------------------------------------------------------------
# Profile tools
# ---------------------------------------------------------------------------


@mcp.tool()
def patent_search_profile_login_start(
    name: str = "default",
) -> dict[str, Any]:
    """Launch a headed browser for manual Google login.

    Opens a visible Chromium window using an isolated browser profile. Log into
    your Google account manually, then close the browser window. Subsequent
    headless searches will reuse the saved login state.

    Args:
        name: Profile name (default: "default"). Creates the profile if it doesn't exist.

    Returns:
        Status message with instructions.
    """
    from patent_mcp.search.profile_manager import ProfileManager, ProfileBusyError

    cfg = _get_config()
    pm = ProfileManager(cfg.search_browser_profiles_dir)

    # Check if profile is busy
    locked, lock_info = pm.is_locked(name)
    if locked and lock_info:
        return {
            "error": f"Profile '{name}' is busy ({lock_info.purpose}, pid={lock_info.pid}). "
            f"Close the existing browser or wait for the search to finish.",
            "isError": True,
        }

    # Launch headed browser in background thread
    profile_dir = pm.get_profile_dir(name)

    def _run_login_browser():
        try:
            from playwright.sync_api import sync_playwright
        except ImportError:
            log.error(
                "Playwright not installed. Run: pip install patent-mcp-server[browser]"
            )
            return

        pm.acquire_lock(name, "login")
        try:
            pw = sync_playwright().start()
            context = pw.chromium.launch_persistent_context(
                user_data_dir=str(profile_dir),
                headless=False,  # always headed for login
                viewport={"width": 1280, "height": 900},
            )
            # Navigate to Google Patents
            page = context.new_page()
            page.goto("https://patents.google.com/", wait_until="networkidle")

            # Block until the user closes the browser window
            try:
                context.wait_for_event("close", timeout=0)
            except Exception:
                pass

            try:
                context.close()
            except Exception:
                pass
            pw.stop()
        except Exception as e:
            log.error("Login browser error: %s", e)
        finally:
            pm.release_lock(name)

    t = threading.Thread(target=_run_login_browser, daemon=True, name=f"login-{name}")
    t.start()

    return {
        "status": "launched",
        "profile_name": name,
        "profile_dir": str(profile_dir),
        "message": (
            f"Headed browser launched with profile '{name}'. "
            f"Log into Google manually, then close the browser window. "
            f"Subsequent searches will reuse the saved login state."
        ),
        "isError": False,
    }


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _hit_to_dict(hit) -> dict[str, Any]:
    """Convert a PatentHit to a plain dict."""
    return asdict(hit)


def _save_to_session(
    session_id: str,
    queries_run: list[dict],
    hits: list,
    metadata: dict[str, Any] | None = None,
) -> None:
    """Save query results to a session."""
    from patent_mcp.search.session_manager import QueryRecord

    sm = _get_session_manager()
    try:
        session = sm.load_session(session_id)
        query_num = len(session.queries) + 1
        for i, q in enumerate(queries_run):
            # Assign results to first query; others get empty (they're the same results, deduped)
            q_hits = hits if i == 0 else []
            record = QueryRecord(
                query_id=f"q{query_num + i:03d}",
                timestamp=now_iso(),
                source=q.get("source", "unknown"),
                query_text=q.get("query", q.get("code", "")),
                result_count=q.get("result_count", len(q_hits)),
                results=q_hits,
                metadata=metadata if i == 0 else None,
            )
            sm.append_query_result(session_id, record)
    except Exception as e:
        log.warning("Failed to save results to session %s: %s", session_id, e)


def _enrich_hits(scored_hits: list) -> list[str]:
    """Enrich top hits with metadata from the fetch pipeline. Returns enriched IDs."""
    enriched_ids: list[str] = []
    try:
        from patent_mcp.id_canon import canonicalize

        cfg = _get_config()
        orchestrator = _get_orchestrator()

        patent_ids = []
        for sh in scored_hits:
            cid = canonicalize(sh.hit.patent_id)
            if cid.canonical:
                patent_ids.append(cid)

        if not patent_ids:
            return []

        output_base = cfg.cache_local_dir
        results = _run(
            orchestrator.fetch_batch(
                patent_ids,
                output_base,
                concurrency=3,
            )
        )

        # Merge metadata back into scored hits
        result_map = {r.canonical_id: r for r in results if r.success and r.metadata}
        for sh in scored_hits:
            cid = canonicalize(sh.hit.patent_id)
            r = result_map.get(cid.canonical)
            if r and r.metadata:
                meta = r.metadata
                if not sh.hit.title and hasattr(meta, "title"):
                    sh.hit.title = meta.title
                if not sh.hit.abstract and hasattr(meta, "abstract"):
                    sh.hit.abstract = getattr(meta, "abstract", None)
                if not sh.hit.assignee and hasattr(meta, "assignee"):
                    sh.hit.assignee = meta.assignee
                if not sh.hit.inventors and hasattr(meta, "inventors"):
                    sh.hit.inventors = meta.inventors or []
                if not sh.hit.date and hasattr(meta, "publication_date"):
                    sh.hit.date = meta.publication_date
                enriched_ids.append(cid.canonical)
    except Exception as e:
        log.warning("Enrichment failed: %s", e)

    return enriched_ids


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------


def run():
    mcp.run()


if __name__ == "__main__":
    run()
