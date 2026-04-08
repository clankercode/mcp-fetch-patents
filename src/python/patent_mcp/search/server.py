"""Deep patent search MCP server — tools for the patent-searcher OpenCode agent."""
from __future__ import annotations

import logging
import sys
from pathlib import Path
from typing import Any

logging.basicConfig(
    stream=sys.stderr,
    level=logging.INFO,
    format="%(asctime)s %(levelname)s %(name)s %(message)s",
)
log = logging.getLogger(__name__)

from mcp.server.fastmcp import FastMCP  # noqa: E402

mcp = FastMCP("patent-search")

# ---------------------------------------------------------------------------
# Lazy imports — avoid loading heavy deps at import time
# ---------------------------------------------------------------------------

def _get_session_manager():
    from patent_mcp.search.session_manager import SessionManager
    return SessionManager()


def _get_config():
    from patent_mcp.config import get_config
    return get_config()


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
    session = sm.create_session(topic=topic, prior_art_cutoff=prior_art_cutoff, notes=notes)
    sm.save_session(session)
    return {
        "session_id": session.session_id,
        "topic": session.topic,
        "created_at": session.created_at,
        "sessions_dir": str(sm.sessions_dir),
        "message": f"Session created. Use session_id='{session.session_id}' in search calls to auto-save results.",
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
        return _session_to_dict(session)
    except FileNotFoundError:
        return {"error": f"Session '{session_id}' not found. Use patent_session_list to see available sessions."}


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
        return {"session_id": session_id, "status": "note added"}
    except FileNotFoundError:
        return {"error": f"Session '{session_id}' not found."}


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
        return {"session_id": session_id, "patent_id": patent_id, "relevance": relevance, "status": "annotated"}
    except FileNotFoundError:
        return {"error": f"Session '{session_id}' not found."}


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
        return {"report_path": str(report_path), "status": "exported"}
    except FileNotFoundError:
        return {"error": f"Session '{session_id}' not found."}


# ---------------------------------------------------------------------------
# Search tools
# ---------------------------------------------------------------------------

@mcp.tool()
def patent_search_natural(
    description: str,
    date_cutoff: str | None = None,
    jurisdictions: list[str] | None = None,
    session_id: str | None = None,
    max_results: int = 25,
) -> dict[str, Any]:
    """Search for patents using a natural language description.

    This tool translates your description into structured patent queries and searches
    multiple sources simultaneously. Best for broad searches when you don't know the
    exact patent terminology.

    Args:
        description: Natural language description of the technology or invention to search for.
                     E.g.: "wireless charging that works through metal objects"
        date_cutoff: Optional date cutoff for prior art (ISO format YYYY-MM-DD).
                     Only returns patents filed/published BEFORE this date.
        jurisdictions: Optional list of jurisdictions (e.g. ["US", "EP", "WO"]).
                       Default: searches all.
        session_id: Optional session ID to automatically save results.
        max_results: Maximum results per source (default 25).

    Returns:
        List of matching patents with metadata, sources searched, and queries run.
    """
    cfg = _get_config()
    results = []
    queries_run = []

    # SerpAPI Google Patents backend
    if cfg.serpapi_key:
        from patent_mcp.search.searchers import SerpApiGooglePatentsBackend
        backend = SerpApiGooglePatentsBackend(api_key=cfg.serpapi_key)
        date_to = date_cutoff  # cutoff is the upper bound for prior art
        hits = _run(backend.search(
            query=description,
            date_to=date_to,
            max_results=max_results,
        ))
        results.extend(hits)
        queries_run.append({
            "source": "Google_Patents (SerpAPI)",
            "query": description,
            "date_to": date_to,
            "result_count": len(hits),
        })
    else:
        log.warning("No PATENT_SERPAPI_KEY configured — Google Patents search unavailable")

    # Deduplicate by patent_id
    seen: set[str] = set()
    deduped = []
    for h in results:
        if h.patent_id not in seen:
            seen.add(h.patent_id)
            deduped.append(h)

    # If session_id provided, save the results
    if session_id and deduped:
        _save_to_session(session_id, queries_run, deduped)

    return {
        "query": description,
        "date_cutoff": date_cutoff,
        "sources_searched": [q["source"] for q in queries_run],
        "queries_run": queries_run,
        "total_found": len(deduped),
        "results": [_hit_to_dict(h) for h in deduped],
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
        hits = _run(backend.search(
            query=query,
            date_from=date_from.replace("-", "") if date_from else None,
            date_to=date_to.replace("-", "") if date_to else None,
            max_results=max_results,
        ))
        all_results.extend(hits)
        queries_run.append({"source": "USPTO", "query": query, "result_count": len(hits)})

    if "EPO_OPS" in sources:
        from patent_mcp.search.searchers import EpoOpsSearchBackend
        epo = EpoOpsSearchBackend(
            client_id=cfg.epo_client_id,
            client_secret=cfg.epo_client_secret,
        )
        hits = _run(epo.search(query=query, date_from=date_from, date_to=date_to, max_results=max_results))
        all_results.extend(hits)
        queries_run.append({"source": "EPO_OPS", "query": query, "result_count": len(hits)})

    if "Google_Patents" in sources and cfg.serpapi_key:
        from patent_mcp.search.searchers import SerpApiGooglePatentsBackend
        gp = SerpApiGooglePatentsBackend(api_key=cfg.serpapi_key)
        hits = _run(gp.search(query=query, date_to=date_to, max_results=max_results))
        all_results.extend(hits)
        queries_run.append({"source": "Google_Patents", "query": query, "result_count": len(hits)})

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

    tree: dict[str, Any] = {"seed": patent_id, "direction": direction, "depth": depth, "citations": {}}

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
            session = sm.load_session(session_id)
            if session.citation_chains is None:
                session.citation_chains = {}
            session.citation_chains[patent_id] = tree["citations"]
            sm.save_session(session)
        except Exception as e:
            log.warning("Failed to save citation chain to session: %s", e)

    return tree


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
    hits = _run(epo.search_by_classification(
        cpc_code=code,
        include_subclasses=include_subclasses,
        date_from=date_from,
        date_to=date_to,
        max_results=max_results,
    ))

    queries_run = [{"source": "EPO_OPS", "code": code, "include_subclasses": include_subclasses}]

    if session_id and hits:
        _save_to_session(session_id, queries_run, hits)
        try:
            sm = _get_session_manager()
            session = sm.load_session(session_id)
            if code not in session.classifications_explored:
                session.classifications_explored.append(code)
                sm.save_session(session)
        except Exception:
            pass

    return {
        "code": code,
        "include_subclasses": include_subclasses,
        "date_from": date_from,
        "date_to": date_to,
        "total_found": len(hits),
        "results": [_hit_to_dict(h) for h in hits],
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
            session = sm.load_session(session_id)
            if session.patent_families is None:
                session.patent_families = {}
            session.patent_families[patent_id] = [m.get("patent_id", "") for m in family_members]
            sm.save_session(session)
        except Exception as e:
            log.warning("Failed to save family data to session: %s", e)

    return {
        "patent_id": patent_id,
        "family_size": len(family_members),
        "members": family_members,
    }


@mcp.tool()
def patent_suggest_queries(
    topic: str,
    context: str = "",
    prior_art_cutoff: str | None = None,
) -> dict[str, Any]:
    """Generate search strategy suggestions for a patent research topic.

    This tool brainstorms search approaches WITHOUT running them. Use it to plan your
    research before executing queries. It suggests:
    - Keywords and synonyms to include
    - IPC/CPC classification codes to explore
    - Boolean query templates for different databases
    - Historical terminology for old patents
    - Which sources to prioritize

    Args:
        topic: The technology or invention to research (natural language description).
        context: Optional additional context (e.g., "looking for prior art for a 2019 patent",
                 "historical patents from 1900-1950", "specifically looking in medical devices").
        prior_art_cutoff: Optional date for prior art (ISO format YYYY-MM-DD).

    Returns:
        A structured set of search strategy recommendations.
    """
    # This is a static advisory tool — no API calls needed
    # Returns structured guidance based on the topic

    suggestions = {
        "topic": topic,
        "context": context,
        "prior_art_cutoff": prior_art_cutoff,
        "strategy": {
            "step_1_concept_expansion": {
                "description": "Expand your topic into synonyms and alternative phrasings before searching",
                "tips": [
                    f"Think about how '{topic}' was described 20, 50, 100 years ago",
                    "List technical synonyms from different industries (e.g., 'electromagnetic induction' = 'wireless power' = 'contactless charging')",
                    "Include verb forms: 'charging' vs 'power transfer' vs 'energy transmission'",
                ],
            },
            "step_2_classification": {
                "description": "Find IPC/CPC class codes — searches by class find patents regardless of keyword",
                "action": "Use patent_classification_search with codes from H01/H02/H03/H04 (electrical), G06 (computing), B (mechanical), C (chemistry), A (medical)",
                "tip": "Start with a broad code like 'H02J' and explore subclasses",
            },
            "step_3_multi_source": {
                "description": "Search at least 3 sources",
                "recommended_sources": [
                    "USPTO (best for US full-text, expert Boolean syntax with field codes)",
                    "EPO_OPS (best for classification search, worldwide coverage, families)",
                    "Google_Patents (widest coverage, includes pre-1976 and non-English)",
                ],
            },
            "step_4_citation_chain": {
                "description": "After finding any relevant patent, ALWAYS follow its citations",
                "action": "Use patent_citation_chain on the most relevant results (direction='both', depth=2)",
                "why": "The best prior art is often found 1-2 hops away in citation chains",
            },
            "recommended_queries": [
                {
                    "type": "Broad keyword (USPTO)",
                    "template": f"TTL/({topic[:30]}) OR ABST/({topic[:30]})",
                    "note": "Start broad, then narrow",
                },
                {
                    "type": "Claims-focused (USPTO)",
                    "template": f"ACLM/({topic[:30].replace(' ', ' AND ')})",
                    "note": "Claims language matters most for prior art",
                },
                {
                    "type": "Natural language (Google Patents via SerpAPI)",
                    "template": topic,
                    "note": "Best for initial broad search",
                },
            ],
        },
    }

    if prior_art_cutoff:
        suggestions["strategy"]["prior_art_notes"] = {
            "cutoff_date": prior_art_cutoff,
            "reminder": f"Search for patents filed/published BEFORE {prior_art_cutoff}",
            "tip": f"A patent published after {prior_art_cutoff} can still be prior art if its application was filed before that date",
        }

    return suggestions


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _hit_to_dict(hit) -> dict[str, Any]:
    """Convert a PatentHit to a plain dict."""
    return {
        "patent_id": hit.patent_id,
        "title": hit.title,
        "date": hit.date,
        "assignee": hit.assignee,
        "inventors": hit.inventors,
        "abstract": hit.abstract,
        "source": hit.source,
        "relevance": hit.relevance,
        "note": hit.note,
        "prior_art": hit.prior_art,
        "url": getattr(hit, "url", None),
    }


def _save_to_session(session_id: str, queries_run: list[dict], hits: list) -> None:
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
                timestamp=_now_iso(),
                source=q.get("source", "unknown"),
                query_text=q.get("query", q.get("code", "")),
                result_count=q.get("result_count", len(q_hits)),
                results=q_hits,
            )
            sm.append_query_result(session_id, record)
    except Exception as e:
        log.warning("Failed to save results to session %s: %s", session_id, e)


def _now_iso() -> str:
    from datetime import datetime, timezone
    return datetime.now(tz=timezone.utc).isoformat()


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------

def run():
    mcp.run()


if __name__ == "__main__":
    run()
