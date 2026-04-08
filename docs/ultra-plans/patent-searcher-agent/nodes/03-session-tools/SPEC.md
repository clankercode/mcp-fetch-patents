# SPEC — 03-session-tools

## What It Does
MCP tools for session lifecycle management — exposed to the OpenCode agent so it can save and recover research state.

## Tools

### 1. patent_session_create
Create a new research session.
- Input: `topic` (str), `prior_art_cutoff` (optional date), `notes` (optional str)
- Output: `session_id` (str), `session_path` (str)

### 2. patent_session_load
Load a session by ID and return its full state.
- Input: `session_id` (str)
- Output: full session JSON

### 3. patent_session_list
List all saved sessions with summary info.
- Input: none (or optional `limit` int)
- Output: list of {session_id, topic, created_at, modified_at, query_count, patent_count}

### 4. patent_session_note
Add or update a researcher note on a session.
- Input: `session_id` (str), `note` (str)
- Output: updated session summary

### 5. patent_session_annotate
Annotate a specific patent result within a session.
- Input: `session_id`, `patent_id`, `annotation` (str), `relevance` ("high" | "medium" | "low" | "irrelevant")
- Output: confirmation

### 6. patent_session_export
Export session to a readable Markdown report.
- Input: `session_id`, `output_path` (optional, default: `.patent-sessions/<id>-report.md`)
- Output: path to generated report file

## Implementation
- Python module: `patent_mcp.search.session_tools`
- All tools delegate to `session_manager` (node 01)
- Session export generates structured Markdown with table of found patents, queries run, notes
