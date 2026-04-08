# INTERFACE — 05-mcp-protocol

## Exposes
MCP server process (no programmatic API — protocol boundary):

### CLI entry points
```
python -m patent_mcp.server [--cache-dir PATH] [--log-level LEVEL]
./patent-mcp-server [--cache-dir PATH] [--log-level LEVEL]
```

### MCP Tools registered
- `fetch_patents(patent_ids, formats?, force_refresh?, postprocess_query?)` → FetchResult
- `list_cached_patents()` → list[CacheEntry]
- `get_patent_metadata(patent_ids)` → list[PatentMetadata | null]

### FetchResult (MCP response content)
```json
{
  "results": [ PatentFetchResult, ... ],
  "summary": { "total": N, "success": N, "cached": N, "errors": N, "total_duration_ms": N }
}
```

## Depends On
01-id-canon, 02-cache-db, 03-source-fetchers, 04-format-conversion, 06-config

## Consumed By
MCP clients (agents). Not consumed by other nodes.
