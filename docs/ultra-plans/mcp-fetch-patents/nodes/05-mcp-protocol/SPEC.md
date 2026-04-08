# SPEC — 05-mcp-protocol: MCP Server + Tool Definitions

## Responsibility
Implement the MCP server layer: stdin/stdout transport, tool registration, request dispatch, response formatting, and batching semantics.

## Transport
- **v1**: stdin/stdout only (MCP standard JSON-RPC 2.0 over newline-delimited messages)
- **v2**: HTTP (FastAPI/axum) — not in scope for v1
- Python: use `fastmcp` library (pip install fastmcp) or official `mcp` SDK
- Rust: implement MCP protocol directly or use community crate (`mcp-server` if available); fallback to manual JSON-RPC over stdin

## Tool: `fetch_patents`

### Description (to appear in MCP tool listing)
```
Fetch patents by ID and return file paths to all downloaded artifacts.

IMPORTANT: This tool is optimized for batch requests. You SHOULD request many 
patents at once (dozens to hundreds) in a single call rather than making 
individual requests. Batching is significantly faster due to parallel fetching 
and shared cache lookups.

Supported jurisdictions: US, EP, WO, JP, CN, KR, AU, CA, NZ, BR, IN, DE, FR, 
GB, and many more. Patent IDs can be in any standard format (e.g., "US7654321",
"EP1234567B1", "WO2024/123456", "US 7,654,321").

Returns file paths to: PDF, plain text, markdown, images, and raw source files.
Does NOT return patent content inline — use the returned file paths to read files.
```

### Input Schema
```json
{
  "name": "fetch_patents",
  "inputSchema": {
    "type": "object",
    "required": ["patent_ids"],
    "properties": {
      "patent_ids": {
        "type": "array",
        "items": {"type": "string"},
        "description": "List of patent IDs in any standard format. Prefer large batches.",
        "minItems": 1
      },
      "formats": {
        "type": "array",
        "items": {"type": "string", "enum": ["pdf", "txt", "md", "images", "all"]},
        "default": ["all"],
        "description": "Formats to download. 'all' downloads everything available."
      },
      "force_refresh": {
        "type": "boolean",
        "default": false,
        "description": "If true, re-fetch even if cached. Use sparingly."
      },
      "postprocess_query": {
        "type": "string",
        "description": "(Future: ignored in v1) A natural language query to run against the patent content using an LLM agent after download."
      }
    }
  }
}
```

### Output Schema
```json
{
  "results": [
    {
      "patent_id": "US7654321",
      "canonical_id": "US7654321",
      "status": "success",    // "success" | "cached" | "partial" | "not_found" | "error"
      "cache_dir": "/abs/path/.patents/US7654321/",
      "files": {
        "pdf": "/abs/path/.patents/US7654321/patent.pdf",
        "txt": "/abs/path/.patents/US7654321/patent.txt",
        "md": "/abs/path/.patents/US7654321/patent.md",
        "images": [
          "/abs/path/.patents/US7654321/images/fig001.png",
          "/abs/path/.patents/US7654321/images/fig002.png"
        ],
        "raw": [
          "/abs/path/.patents/US7654321/raw/patent.xml"
        ]
      },
      "metadata": {
        "title": "Widget assembly",
        "inventors": ["Alice Smith"],
        "assignee": "ACME Corp",
        "filing_date": "2005-03-12",
        "publication_date": "2010-01-19"
      },
      "sources_used": ["USPTO", "Google_Patents"],
      "fetch_duration_ms": 1240
    },
    {
      "patent_id": "INVALID123",
      "canonical_id": null,
      "status": "error",
      "error": "Could not parse patent ID: 'INVALID123'",
      "files": {}
    }
  ],
  "summary": {
    "total": 2,
    "success": 1,
    "cached": 0,
    "not_found": 0,
    "errors": 1,
    "total_duration_ms": 1350
  }
}
```

### Batching Behavior
- Process all `patent_ids` in parallel (configurable concurrency limit, default 10)
- Each patent processed independently; failure of one doesn't fail others
- Results in same order as input `patent_ids`
- `postprocess_query` in v1: log warning "postprocess_query received but not yet implemented", include in result metadata, ignore otherwise

## Tool: `list_cached_patents` (secondary tool)
```
List all patents in the local and global cache.
Returns: array of {canonical_id, cache_dir, formats_available, fetched_at}
Useful for agents to know what's already downloaded.
```

## Tool: `get_patent_metadata` (secondary tool)
```
Return metadata for a list of patent IDs without fetching content.
Checks global index only (no network). Fast.
```

## Server Startup
```
python -m patent_mcp.server   # Python
./patent-mcp-server           # Rust binary
```
Both accept:
- `--cache-dir PATH`: override local cache directory
- `--log-level debug|info|warn|error`

## Token Budget Management
The MCP response body should never be so large it overwhelms the calling agent's context window. Apply token budget checks before returning:
```python
MAX_RESPONSE_TOKENS = 8000  # configurable via PATENT_MAX_RESPONSE_TOKENS env var

def estimate_tokens(text: str) -> int:
    return len(text) // 4   # rough approximation

def truncate_if_needed(data: dict, max_tokens: int) -> dict:
    """Truncate large metadata fields (abstract, title) if response is too large."""
    ...
```
File paths are small; the concern is large metadata fields (abstracts, titles). Truncate abstracts at 500 chars if budget exceeded. Always preserve file paths (never truncate).

## Stderr/Stdout Discipline
CRITICAL: All logging output must go to **stderr**. **Stdout is reserved exclusively for MCP JSON-RPC protocol messages.** Any accidental stdout output breaks the MCP client.
- Python: `logging.basicConfig(stream=sys.stderr)`
- Rust: use `eprintln!()` for all debug output

## Error Handling
- Per-patent errors returned in results array, not as MCP protocol errors
- Only fatal errors (e.g., DB corrupt, can't write to cache) returned as MCP errors
- All errors include: error code, human message, patent_id if applicable

## Dependencies
- `fastmcp` or `mcp` Python SDK
- All other nodes (01-06)

## Test Surface
- Unit: tool schema validates correctly
- Unit: batch result ordering matches input ordering
- Unit: `postprocess_query` parameter accepted without crash in v1
- Unit: error in one patent doesn't affect others in batch
- Integration (mocked): end-to-end MCP message → tool call → result
