# PRODUCT_GOALS — Deep Patent Searcher Agent

## Primary Goal
Build an OpenCode agent that acts as an expert deep patent searcher — excellent at finding niche patents, prior art (and related non-prior art), constructing advanced search queries, natural language searching, citation chaining, classification navigation, and historical patent research. The agent must save and restore its search state.

## Core Capabilities Required
1. **Natural language → structured query translation**: User says "I need prior art for a wireless charging method that charges through metal objects" → agent constructs precise patent queries
2. **Multi-source parallel search**: Queries USPTO, EPO OPS, Espacenet, Google Patents, Lens.org simultaneously
3. **Niche discovery**: Classification-based search (IPC/CPC), citation chaining, forward/backward references, family expansion
4. **Historical patent search**: Patents from the 1800s-1950s, unusual terminology mapping
5. **Prior art / related art distinction**: Flags whether results are prior art (predates claim date) vs. related art
6. **State persistence**: Saves search sessions (queries run, results found, annotations) to disk; can resume a prior session
7. **Search strategy advisor**: Explains what queries it ran, why, and what it missed

## Success Criteria (Acceptance Criteria)

- [ ] OpenCode agent config file created with correct format for the opencode tool
- [ ] Agent has a detailed, expert-level system prompt covering all search strategies
- [ ] State management: searches saved to JSON/TOML session files; can load by session ID
- [ ] At least 5 distinct search tools defined (natural language search, structured query, citation chaining, classification nav, family search)
- [ ] MCP tool integration: uses the existing patent_mcp fetchers (USPTO, EPO, Espacenet, Google Patents, etc.)
- [ ] Tested: example queries run against real APIs return plausible results
- [ ] Agent instructions cover: Boolean syntax, IPC/CPC navigation, proximity operators, field codes, truncation
- [ ] Non-prior-art / prior-art date reasoning logic
- [ ] Documentation: README or USAGE.md explains how to start a session, use the agent, and recover state

## Non-Goals
- Not a standalone web service (agent lives in opencode framework)
- Not replacing the existing MCP patent server (this is a *user-facing agent* built on top of it)
- Not paid API integration (Derwent, Patsnap) — free/freemium APIs only
- Not OCR/PDF processing (handled by existing mcp-fetch-patents)

## Target User
Max Kaye doing historical patent research and prior art analysis. Needs to find obscure, niche, and historical patents efficiently.
