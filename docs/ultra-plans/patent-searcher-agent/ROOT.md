# ROOT — Deep Patent Searcher Agent

## Project: patent-searcher-agent
**Goal:** OpenCode agent for expert deep patent searching with state persistence

## Tree Structure

```
patent-searcher-agent/
├── 01-state-manager/        ← JSON session persistence (save/restore/list)
├── 02-search-tools/         ← MCP tools: structured, natural language, citation, classification, family
├── 03-session-tools/        ← MCP tools: save/load/list sessions
├── 04-agent-config/         ← opencode.json agent entry + system prompt
└── 05-tests-docs/           ← Integration tests + usage docs
```

## Status Dashboard

| Node | Status | Owner |
|------|--------|-------|
| 01-state-manager | ⬜ not started | — |
| 02-search-tools | ⬜ not started | — |
| 03-session-tools | ⬜ not started | — |
| 04-agent-config | ⬜ not started (awaiting opencode research) | — |
| 05-tests-docs | ⬜ not started | — |

## Key Dependencies
- `01-state-manager` ← used by `03-session-tools`
- `02-search-tools` + `03-session-tools` ← registered in `04-agent-config`
- Research: opencode agent config format (pending: RESEARCH_opencode_agents.md)
- Research: patent search strategies (pending: RESEARCH_patent_search.md)
