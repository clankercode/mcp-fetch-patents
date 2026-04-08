# SPEC — 02-search-tools

## What It Does
MCP tools for deep patent searching, exposed to the OpenCode agent. Each tool performs a specific type of search and returns structured results that the agent can reason over.

## Tools

### 1. patent_search_natural
Convert a natural-language description to structured patent queries, run them, return results.
- Input: `description` (str), `date_cutoff` (optional ISO date), `jurisdictions` (optional list), `session_id` (optional)
- Behavior: Agent auto-expands synonyms and alternative terminology, constructs 2-4 complementary queries, runs them across configured sources, deduplicates results
- Output: list of patents with metadata, relevance notes, queries actually run

### 2. patent_search_structured
Run an expert-syntax patent query against one or more sources.
- Input: `query` (str, supports Boolean + field codes), `sources` (list, default all), `date_from` / `date_to` (optional), `session_id` (optional)
- Behavior: Routes to USPTO, EPO OPS, Espacenet, Google Patents per source
- Output: list of patent results + query echo

### 3. patent_citation_chain
Follow forward or backward citations from a seed patent.
- Input: `patent_id` (str), `direction` ("forward" | "backward" | "both"), `depth` (int 1-3), `session_id` (optional)
- Behavior: Recursively fetches citations. Depth=1 is direct only. Returns a tree structure.
- Output: citation tree with metadata per patent

### 4. patent_classification_search
Search by IPC/CPC classification code, optionally traversing the hierarchy.
- Input: `code` (str, e.g. "H02J50"), `include_subclasses` (bool), `date_from`/`date_to` (optional), `session_id` (optional)
- Behavior: Queries EPO OPS or Espacenet by classification code; can expand to sub-classes
- Output: list of patents in that class + subclass breakdown

### 5. patent_family_search
Find all family members of a patent across jurisdictions.
- Input: `patent_id` (str), `session_id` (optional)
- Behavior: Queries Espacenet family endpoint; returns WO/EP/US/AU/etc. equivalents
- Output: family members with jurisdiction, publication date, status

### 6. patent_suggest_queries
Given a topic, suggest a set of search strategies (not run yet).
- Input: `topic` (str), `context` (optional str)
- Output: list of suggested queries with explanations, IPC/CPC codes to explore, synonyms/terminology alternatives

## Implementation
- Python module: `patent_mcp.search.tools`
- Each tool calls existing fetchers in `patent_mcp.fetchers` where possible
- Falls back to HTTP/scraping where fetchers don't support a query
- Returns session-aware results (if session_id provided, appends to session)
- Results are typed dataclasses, serialized to JSON for MCP transport

## Dependencies
- patent_mcp.fetchers.orchestrator (existing)
- patent_mcp.search.session_manager (node 01)
- httpx (existing dep)
