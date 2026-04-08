# INTERVIEW QUEUE — mcp-fetch-patents

Priority: P0 = blocking, P1 = important, P2 = nice-to-know

## P0 — Blocking

### IQ-01: Language / Runtime
What language/runtime should this be implemented in? (Python, TypeScript, Rust, Go, other?)
- Most MCP ecosystem is Python + TypeScript
- Affects: framework choices, PDF tooling, distribution

### IQ-02: Patent ID Scope
Which patent jurisdictions must we support for v1?
- US only (USPTO)?
- International (EP, WO, JP, CN, KR, etc.)?
- All of the above?
- Any specific ones that are highest priority?

### IQ-03: Primary Patent Sources — Priority Order
Which sources should we attempt first, and in what order?
Candidates: Google Patents, USPTO Full-Text, EPO OPS API, WIPO PatentScope, Lens.org, Espacenet, SerpAPI/web scrape
- Do you have any API keys already (e.g., EPO OPS, Lens.org)?
- Any sources that are off-limits (rate-limit concerns, ToS, etc.)?

### IQ-04: Global DB / Index Location
Where should the system-wide patent index live?
- `~/.patent-cache/patents.db` (SQLite)?
- `~/.local/share/patent-cache/`?
- XDG-compliant path?
- User-configurable via env var?

### IQ-05: Local Cache Location (per-repo)
The description says "patent cache should be local to that repo."
- Should this be `.patents/` in the working directory?
- Configurable via `.patents.toml` or similar?
- How should the server discover other caches on the system (scan $HOME? registered list? both)?

## P1 — Important

### IQ-06: Test Framework
What test framework preference?
- Python: pytest (standard), unittest
- TypeScript: vitest, jest
- Must run in <1s → all network calls must be mocked/stubbed

### IQ-07: PDF-to-Markdown Conversion Tool
Any preference for how to convert patent PDFs to markdown?
- marker (ML-based, high quality but slow/heavy)
- pymupdf4llm (fast, good quality, MIT license)
- pdfplumber (lightweight but less accurate)
- pdftotext + post-processing
- Multiple tools in fallback chain?

### IQ-08: HTTP Server Framework (for HTTP mode)
For HTTP transport (in addition to stdin):
- FastAPI? Starlette? Flask? Other?
- Is HTTP a v1 requirement or future?

### IQ-09: `postprocess_query` — CLI Agent
The description mentions a future `postprocess_query` parameter that calls a CLI coding agent.
- Which CLI agent to call by default: `claude`, `aider`, or configurable?
- Should it be pluggable (env var / config)?
- For v1, this parameter is accepted but ignored — confirm?

### IQ-10: Legal / Application Status Sources
Where to source legal status and application timeline data?
- USPTO PAIR / PatentsView API?
- EPO Register?
- Is this a v1 feature or v2?

## P2 — Nice-to-Know

### IQ-11: Image / Diagram Handling
For patent figures/diagrams:
- Download as PNG/JPEG only?
- Also run OCR (e.g., tesseract) to extract figure captions?
- Store in `images/` subdirectory per patent?

### IQ-12: Packaging / Distribution
How should this be distributed?
- PyPI package? npm package?
- Docker image?
- Just a git repo + instructions?

### IQ-13: Authentication / API Keys Configuration
How should API keys be configured?
- Environment variables?
- Config file (e.g., `~/.patents.toml`)?
- MCP server startup arguments?

## Answered
(none yet)
