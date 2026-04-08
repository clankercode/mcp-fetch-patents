# SPEC — 01-state-manager

## What It Does
Manages patent search session state on disk. A "session" is a named research project (e.g., "prior-art-wireless-charging-2026"). Sessions accumulate queries, results, notes, explored classifications, citation chains.

## Responsibilities
- Create new session with a user-provided name/topic
- Load existing session by ID or name
- List all sessions (sorted by recent activity)
- Append query results to a session
- Add/update researcher notes on a session
- Auto-generate a session ID (timestamp + slug from topic)
- Save atomically (write to .tmp then rename)

## Data Model

```
.patent-sessions/
  <session-id>.json        ← one file per session
  .index.json              ← fast index: id → {name, created, modified, query_count}
```

### Session JSON Schema
```json
{
  "session_id": "20260407-143000-wireless-charging",
  "topic": "Wireless charging through metal objects",
  "created_at": "ISO8601",
  "modified_at": "ISO8601",
  "prior_art_cutoff": "2020-01-01",
  "notes": "...",
  "queries": [
    {
      "id": "q001",
      "timestamp": "ISO8601",
      "source": "USPTO",
      "query_text": "TTL/(wireless AND charging) AND ACLM/(metal)",
      "result_count": 45,
      "results": [
        {"patent_id": "US10123456B2", "title": "...", "date": "2019-05-14", "relevance": "high", "note": ""}
      ]
    }
  ],
  "classifications_explored": ["H02J50/10", "H01F38/14"],
  "citation_chains": {
    "US10123456B2": {"forward": [...], "backward": [...]}
  },
  "patent_families": {
    "US10123456B2": ["EP3456789A1", "WO2019123456A1"]
  }
}
```

## Interface
See INTERFACE.md — exposed as Python module `patent_mcp.search.session_manager`

## Implementation Notes
- Pure Python stdlib (json, pathlib, datetime) — no external deps
- Session dir configurable via env var `PATENT_SESSIONS_DIR` (default: `.patent-sessions/`)
- .index.json updated on every write for fast listing
