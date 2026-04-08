# DECISIONS — patent-searcher-agent

## ADR-01: OpenCode agent framework
**Status:** Pending research (opencode-research agent running)
**Question:** Is the target "opencode" the SST tool at opencode.ai? What's the agent config format?
**Decision:** TBD after research completes.

## ADR-02: State persistence format
**Status:** Draft
**Question:** How should search session state be saved?
**Options:**
- JSON file per session in `.patent-sessions/`
- SQLite (too heavy for search state)
- TOML (readable, good for config-like state)
**Decision:** JSON files in `.patent-sessions/<session-id>.json` — flexible schema, easy to read/write from Python or OpenCode tools, human-readable.
**Rationale:** Lightweight, appendable, no dependencies beyond stdlib.

## ADR-03: Tool architecture for the agent
**Status:** Draft
**Question:** Should the agent tools be MCP tools (server) or OpenCode built-in tools?
**Options:**
- Use existing mcp-fetch-patents server directly
- Define agent-specific tools as MCP tools in this project
- Use OpenCode's built-in file/web tools only
**Decision:** TBD — depends on OpenCode agent tool system (research pending).
**Leaning toward:** Wrap existing patent fetchers as an additional MCP agent in opencode config, calling the patent_mcp server.

## ADR-04: System prompt depth
**Status:** Draft
**Decision:** System prompt will be extensive (~1000-2000 words) covering:
- Complete Boolean query syntax for each patent database
- IPC/CPC classification guide
- Citation chaining workflow
- Prior art date reasoning
- Niche/obscure patent finding tactics
- Natural language → structured query translation steps
