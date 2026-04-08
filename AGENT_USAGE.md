# Patent Searcher Agent — Usage Guide

The `patent-searcher` OpenCode agent is an expert deep patent researcher. It finds niche patents, prior art, related art, and historical patents using multi-source search, citation chaining, and IPC/CPC classification navigation.

## Quick Start

1. **Start OpenCode in the project directory:**
   ```bash
   cd /path/to/your/work
   opencode
   ```

2. **Switch to the patent-searcher agent:**
   - Press `Tab` to cycle through agents, or
   - Type `@patent-searcher` to mention it directly

3. **Start a research session:**
   ```
   I need to find prior art for a patent on wireless charging through metal objects.
   The invention was filed on 2019-03-15.
   ```

The agent will automatically create a session, run multiple searches across USPTO/EPO/Google Patents, follow citations, and save all results.

## Setup

### MCP Server (required for search tools)

Use a user-level OpenCode MCP entry for `patent-search` if you want the Python search server available across projects. The repo's `opencode.json` keeps a disabled example entry that launches from this checkout with `.venv/bin/python -m patent_mcp.search`.

If you want to enable the checkout-local Python server, prerequisite:

```bash
pip install -e ".[dev]"
```

The server uses the same API keys as the main patent fetcher:
- `PATENT_SERPAPI_KEY` — Google Patents search via SerpAPI (most important for natural language search)
- `PATENT_EPO_KEY` — EPO OPS for classification search and citation chains (optional, free tier available without key)

If you are running the Python server from this checkout, you can set these in a repo-local `.env`:
```
PATENT_SERPAPI_KEY=your-key-here
PATENT_EPO_KEY=client_id:client_secret
```

If you are using the installed Rust server, put secrets in `~/.patents-mcp.env`, `~/.patents.toml`, or normal environment variables. The Rust server autoloads `~/.patents-mcp.env` itself, so your launcher can stay simple.

## Agent Capabilities

### Natural Language Search
```
Find me patents about electromagnetic shielding that allows wireless charging.
Search before 2018 for prior art purposes.
```

### Expert Boolean Query Search
```
Run this USPTO query: TTL/(inductive AND transfer) AND CPC/H02J50 AND APD/20000101->20191231
```

### Citation Chaining
```
Follow the citations from US10461587B2, both forward and backward, 2 levels deep.
```

### Classification Search
```
Search all patents in IPC class H02J50 (wireless power transfer) including subclasses.
```

### Family Search
```
Find all patent family members of EP3312981A1 across all jurisdictions.
```

### Query Strategy Planning
```
What search strategy should I use to find prior art for contactless energy transfer 
through metallic barriers? Give me a plan before I start searching.
```

## Session Management

Sessions automatically save all queries, results, and annotations to `.patent-sessions/`.

### Start or resume a session
```
Start a research session for prior art on "method for inductive power transfer through metal".
Set the prior art cutoff date to 2018-06-01.
```

### Check existing sessions
```
List my saved research sessions.
```

### Resume a session
```
Load session 20260407-143000-prior-art-wireless-charging and show me what we found so far.
```

### Add notes
```
Add a note to this session: "H02J50/10 is the most relevant subclass. Found 45 patents there."
```

### Export a report
```
Export a Markdown report of this session.
```

## Session Files

Sessions are stored in `.patent-sessions/` as JSON files:

```
.patent-sessions/
  20260407-143000-prior-art-wireless-charging.json    ← full session data
  20260407-143000-prior-art-wireless-charging-report.md  ← exported report
  .index.json    ← fast index for listing
```

To recover a session after a crash or restart, just tell the agent:
```
I was working on a prior art search for wireless charging through metal. 
Can you find and load that session?
```

## MCP Tools Reference

| Tool | Description |
|------|-------------|
| `patent_search_natural` | Natural language → structured queries, runs on Google Patents via SerpAPI |
| `patent_search_structured` | Expert Boolean queries against USPTO/EPO/Google Patents |
| `patent_citation_chain` | Forward/backward citation following (depth 1-3) |
| `patent_classification_search` | IPC/CPC class-based search including subclasses |
| `patent_family_search` | Finds patent family members across jurisdictions |
| `patent_suggest_queries` | Brainstorms search strategy without running queries |
| `patent_session_create` | Creates a new named research session |
| `patent_session_load` | Loads a saved session |
| `patent_session_list` | Lists all saved sessions |
| `patent_session_note` | Adds researcher notes to a session |
| `patent_session_annotate` | Marks a patent as high/medium/low relevance |
| `patent_session_export` | Exports session to Markdown report |

## Tips for Best Results

1. **Always use sessions** — saves progress across conversations
2. **Start broad, then narrow** — use `patent_suggest_queries` to plan before searching
3. **Use classification search for niche topics** — finds patents with unusual terminology
4. **Always follow citations** — the most relevant prior art is often 1-2 hops away
5. **Search multiple sources** — USPTO, EPO, and Google Patents have different coverage
6. **For old patents (pre-1976)**: Tell the agent to use Google Patents specifically

## Architecture

```
OpenCode agent (patent-searcher.md)
    ↓ uses
patent-search MCP server (src/python/patent_mcp/search/server.py)
    ↓ depends on
session_manager.py    — JSON session persistence
searchers.py          — HTTP backends for USPTO, EPO OPS, SerpAPI Google Patents
    ↓ reuses
patent_mcp.fetchers   — existing fetch-by-ID infrastructure
patent_mcp.config     — API key configuration
```
