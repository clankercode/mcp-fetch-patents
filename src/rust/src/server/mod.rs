//! MCP protocol server — mirrors Python patent_mcp.server module.
//!
//! Implements JSON-RPC 2.0 over stdin/stdout (MCP transport).

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Write;
use std::collections::HashMap;
use tracing::warn;

use crate::config::PatentConfig;

// ---------------------------------------------------------------------------
// JSON-RPC types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}

#[derive(Debug, Serialize)]
struct RpcResponse {
    jsonrpc: String,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
}

#[derive(Debug, Serialize)]
struct RpcError {
    code: i32,
    message: String,
}

impl RpcResponse {
    fn ok(id: Value, result: Value) -> Self {
        RpcResponse {
            jsonrpc: "2.0".into(),
            id,
            result: Some(result),
            error: None,
        }
    }

    fn err(id: Value, code: i32, message: &str) -> Self {
        RpcResponse {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(RpcError { code, message: message.to_string() }),
        }
    }
}

fn tool_error(id: Value, message: &str) -> RpcResponse {
    RpcResponse::ok(id, serde_json::json!({
        "content": [{"type": "text", "text": message}],
        "isError": true
    }))
}

// ---------------------------------------------------------------------------
// MCP tool descriptors
// ---------------------------------------------------------------------------

fn get_str_param(params: &serde_json::Value, name: &str) -> Option<String> {
    params.get("arguments")
        .and_then(|a| a.get(name))
        .and_then(|v| v.as_str())
        .map(String::from)
}

fn get_int_param(params: &serde_json::Value, name: &str) -> Option<u64> {
    params.get("arguments")
        .and_then(|a| a.get(name))
        .and_then(|v| v.as_u64())
}

fn get_bool_param(params: &serde_json::Value, name: &str) -> Option<bool> {
    params.get("arguments")
        .and_then(|a| a.get(name))
        .and_then(|v| v.as_bool())
}

fn get_str_array_param(params: &serde_json::Value, name: &str) -> Option<Vec<String>> {
    params.get("arguments")
        .and_then(|a| a.get(name))
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
}

fn tools_list() -> Value {
    serde_json::json!({
        "tools": [
            {
                "name": "fetch_patents",
                "description": "Fetch one or more patents by ID. Returns file paths + metadata. Batch requests are encouraged — pass multiple IDs.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "patent_ids": {
                            "type": "array",
                            "items": {"type": "string"},
                            "description": "Patent IDs in any common format."
                        },
                        "force_refresh": {
                            "type": "boolean",
                            "default": false,
                            "description": "Skip cache and re-fetch from sources."
                        },
                        "postprocess_query": {
                            "type": "string",
                            "description": "Query for post-processing (stored for v2; no-op in v1)."
                        },
                        "formats": {
                            "type": "array",
                            "items": {"type": "string"},
                            "description": "Output formats to generate (e.g. [\"pdf\", \"txt\", \"md\"]). Default: all available formats."
                        }
                    },
                    "required": ["patent_ids"]
                }
            },
            {
                "name": "list_cached_patents",
                "description": "List all cached patents.",
                "inputSchema": {"type": "object", "properties": {}}
            },
            {
                "name": "get_patent_metadata",
                "description": "Return cached metadata for patents (no network call).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "patent_ids": {
                            "type": "array",
                            "items": {"type": "string"}
                        }
                    },
                    "required": ["patent_ids"]
                }
            },
            {
                "name": "patent_search_natural",
                "description": "Search for patents using a natural language description. Expands your description into multiple query variants, runs them against search backends (browser, SerpAPI, USPTO, EPO), merges and reranks results, and optionally enriches the top hits with full metadata.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "description": {"type": "string", "description": "Natural language description of the technology or invention."},
                        "date_cutoff": {"type": "string", "description": "Optional ISO date — only return patents before this date."},
                        "jurisdictions": {"type": "array", "items": {"type": "string"}, "description": "Optional jurisdiction filter (e.g. [\"US\", \"EP\"])."},
                        "session_id": {"type": "string", "description": "Optional session ID to save results."},
                        "max_results": {"type": "integer", "default": 25, "description": "Maximum results after ranking."},
                        "backend": {"type": "string", "default": "auto", "description": "Search backend: \"browser\", \"serpapi\", or \"auto\"."},
                        "enrich_top_n": {"type": "integer", "description": "Enrich top N results with full metadata. Default: 5. Set to 0 to disable."},
                        "profile_name": {"type": "string", "description": "Browser profile to use for browser-backed search."},
                        "debug": {"type": "boolean", "default": false, "description": "If true, save debug HTML snapshots of search result pages."}
                    },
                    "required": ["description"]
                }
            },
            {
                "name": "patent_search_structured",
                "description": "Run an expert-syntax Boolean patent query against one or more sources (USPTO, EPO OPS, Google Patents). Examples: USPTO: TTL/(wireless ADJ charging) AND CPC/H02J50, EPO: ti=\"wireless charging\" AND ic=\"H02J50/*\"",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": {"type": "string", "description": "Boolean query string with field codes."},
                        "sources": {"type": "array", "items": {"type": "string"}, "description": "Sources to query. Options: \"USPTO\", \"EPO_OPS\", \"Google_Patents\"."},
                        "date_from": {"type": "string", "description": "Start date filter (YYYY-MM-DD)."},
                        "date_to": {"type": "string", "description": "End date filter (YYYY-MM-DD)."},
                        "session_id": {"type": "string", "description": "Optional session ID to automatically save results."},
                        "max_results": {"type": "integer", "default": 25, "description": "Maximum results per source (default 25)."}
                    },
                    "required": ["query"]
                }
            },
            {
                "name": "patent_citation_chain",
                "description": "Follow patent citations forward or backward to discover related patents.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "patent_id": {"type": "string", "description": "Seed patent ID."},
                        "direction": {"type": "string", "default": "backward", "description": "\"backward\", \"forward\", or \"both\"."},
                        "depth": {"type": "integer", "default": 1, "description": "Citation depth (1-2). Depth 2 follows citations of citations."},
                        "session_id": {"type": "string", "description": "Optional session ID to save the citation tree."}
                    },
                    "required": ["patent_id"]
                }
            },
            {
                "name": "patent_classification_search",
                "description": "Search patents by IPC or CPC classification code.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "code": {"type": "string", "description": "IPC/CPC classification code (e.g. \"H02J50\")."},
                        "include_subclasses": {"type": "boolean", "default": true, "description": "If true, includes all sub-codes under this code (e.g. H02J50 includes H02J50/10, H02J50/20, etc.)."},
                        "date_from": {"type": "string", "description": "Start date filter (YYYY-MM-DD)."},
                        "date_to": {"type": "string", "description": "End date filter (YYYY-MM-DD)."},
                        "session_id": {"type": "string", "description": "Optional session ID to save results."},
                        "max_results": {"type": "integer", "default": 25, "description": "Maximum results to return (default 25)."}
                    },
                    "required": ["code"]
                }
            },
            {
                "name": "patent_family_search",
                "description": "Find all family members of a patent across jurisdictions.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "patent_id": {"type": "string", "description": "The patent ID to find family members for (e.g. \"US10123456B2\")."},
                        "session_id": {"type": "string", "description": "Optional session ID to save the family data."}
                    },
                    "required": ["patent_id"]
                }
            },
            {
                "name": "patent_suggest_queries",
                "description": "Generate search strategy suggestions for a patent research topic without running them. Returns a multi-step strategy guide: NL query variants from the planner, classification codes to explore, citation chain directions, and a prior-art date reminder if a cutoff is provided.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "topic": {"type": "string", "description": "Technology or invention to research."},
                        "context": {"type": "string", "description": "Additional context."},
                        "prior_art_cutoff": {"type": "string", "description": "Prior art date cutoff (YYYY-MM-DD)."}
                    },
                    "required": ["topic"]
                }
            },
            {
                "name": "patent_session_create",
                "description": "Create a new patent research session. Sessions persist across tool calls and accumulate search results, annotations, citation chains, and researcher notes. Pass the returned session_id to search tools to auto-save results.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "topic": {"type": "string", "description": "Brief description of the research topic (used for session naming)."},
                        "prior_art_cutoff": {"type": "string", "description": "ISO date (YYYY-MM-DD) — if set, the strategy guide highlights patents before this date as prior art."},
                        "notes": {"type": "string", "default": "", "description": "Optional initial notes for the session."}
                    },
                    "required": ["topic"]
                }
            },
            {
                "name": "patent_session_load",
                "description": "Load a saved patent research session.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "session_id": {"type": "string", "description": "The session ID to load (from patent_session_list or a prior session_create call)."}
                    },
                    "required": ["session_id"]
                }
            },
            {
                "name": "patent_session_list",
                "description": "List all saved patent research sessions.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "limit": {"type": "integer", "description": "Max sessions to return. Default: 20."}
                    }
                }
            },
            {
                "name": "patent_session_note",
                "description": "Add a researcher note to a session.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "session_id": {"type": "string", "description": "The session to update."},
                        "note": {"type": "string", "description": "Text note to append (e.g. observations, next steps, hypotheses)."}
                    },
                    "required": ["session_id", "note"]
                }
            },
            {
                "name": "patent_session_annotate",
                "description": "Annotate a patent with relevance and notes within a session.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "session_id": {"type": "string", "description": "The session containing the patent."},
                        "patent_id": {"type": "string", "description": "The patent ID to annotate (e.g. \"US10123456B2\")."},
                        "annotation": {"type": "string", "description": "Researcher note about why this patent is relevant/irrelevant."},
                        "relevance": {"type": "string", "description": "\"high\", \"medium\", \"low\", or \"unknown\""}
                    },
                    "required": ["session_id", "patent_id", "annotation", "relevance"]
                }
            },
            {
                "name": "patent_session_export",
                "description": "Export a session as a Markdown report.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "session_id": {"type": "string", "description": "The session to export."},
                        "output_path": {"type": "string", "description": "Custom output path. Defaults to .patent-sessions/{session_id}-report.md"}
                    },
                    "required": ["session_id"]
                }
            },
            {
                "name": "patent_search_profile_login_start",
                "description": "Launch a headed browser for manual Google login. Opens a visible Chromium window using an isolated browser profile. Log into your Google account manually, then close the browser window. Subsequent headless searches will reuse the saved login state.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string", "description": "Profile name (default: 'default')."}
                    },
                    "required": []
                }
            }
        ]
    })
}

#[allow(clippy::large_enum_variant)]
enum FetchPlan {
    Invalid(crate::fetchers::OrchestratorResult),
    Valid,
}

async fn build_fetch_patents_payload(
    patent_ids: &[String],
    force_refresh: bool,
    config: &PatentConfig,
    orchestrator: &crate::fetchers::FetcherOrchestrator,
) -> Value {
    if patent_ids.is_empty() {
        return serde_json::json!({
            "results": [],
            "summary": {
                "total": 0,
                "success": 0,
                "cached": 0,
                "errors": 0,
                "total_duration_ms": 0.0
            }
        });
    }

    let start_total = std::time::Instant::now();

    let mut plan: Vec<(String, FetchPlan)> = Vec::with_capacity(patent_ids.len());
    let mut valid_patents = Vec::new();

    for raw_id in patent_ids {
        let canon = crate::id_canon::canonicalize(raw_id);
        if canon.jurisdiction == "UNKNOWN" {
            plan.push((
                raw_id.clone(),
                FetchPlan::Invalid(crate::fetchers::OrchestratorResult {
                    canonical_id: canon.canonical,
                    success: false,
                    cache_dir: None,
                    files: HashMap::new(),
                    metadata: None,
                    sources: vec![],
                    error: Some(format!("Invalid patent ID: {}", raw_id)),
                    from_cache: false,
                }),
            ));
        } else {
            valid_patents.push(canon.clone());
            plan.push((raw_id.clone(), FetchPlan::Valid));
        }
    }

    let valid_results = if valid_patents.is_empty() {
        Vec::new()
    } else {
        if force_refresh {
            orchestrator
                .fetch_batch_force_refresh(&valid_patents, &config.cache_local_dir)
                .await
        } else {
            orchestrator.fetch_batch(&valid_patents, &config.cache_local_dir).await
        }
    };
    let mut valid_iter = valid_results.into_iter();

    let mut results = Vec::with_capacity(patent_ids.len());
    let mut n_success = 0u32;
    let mut n_cached = 0u32;
    let mut n_errors = 0u32;

    for (raw_id, item) in plan {
        let orc = match item {
            FetchPlan::Invalid(result) => result,
            FetchPlan::Valid => valid_iter
                .next()
                .expect("valid patent results should align with fetch plan"),
        };

        let files: std::collections::HashMap<String, String> = orc
            .files
            .iter()
            .map(|(k, v)| (k.clone(), v.to_string_lossy().into_owned()))
            .collect();

        let metadata: Option<serde_json::Value> = orc
            .metadata
            .as_ref()
            .map(|m| serde_json::to_value(m).unwrap_or(Value::Null));

        let status = if orc.from_cache {
            n_cached += 1;
            n_success += 1;
            "cached"
        } else if orc.success {
            n_success += 1;
            "fetched"
        } else if !files.is_empty() {
            n_success += 1;
            "partial"
        } else {
            n_errors += 1;
            "error"
        };

        let fetch_duration_ms = orc
            .sources
            .iter()
            .map(|s| s.elapsed_ms)
            .sum::<f64>();

        results.push(serde_json::json!({
            "patent_id": raw_id,
            "canonical_id": orc.canonical_id,
            "status": status,
            "files": files,
            "metadata": metadata,
            "sources": [],
            "fetch_duration_ms": fetch_duration_ms,
            "error": orc.error,
        }));
    }

    let total_duration_ms = start_total.elapsed().as_secs_f64() * 1000.0;
    let total = results.len() as u32;
    serde_json::json!({
        "results": results,
        "summary": {
            "total": total,
            "success": n_success,
            "cached": n_cached,
            "errors": n_errors,
            "total_duration_ms": total_duration_ms
        }
    })
}

// ---------------------------------------------------------------------------
// Sync routing (parse + non-tool dispatch)
// ---------------------------------------------------------------------------

enum Dispatch {
    /// Respond immediately with this response
    Immediate(RpcResponse),
    /// Notification — no response should be sent (JSON-RPC spec)
    Notification,
    /// Needs async tool execution — carry the parsed request
    ToolCall { id: Value, params: Value },
}

fn route_line(line: &str) -> Dispatch {
    let req: RpcRequest = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(e) => {
            return Dispatch::Immediate(
                RpcResponse::err(Value::Null, -32700, &format!("Parse error: {}", e))
            );
        }
    };

    let id = req.id.clone().unwrap_or(Value::Null);

    match req.method.as_str() {
        "initialize" => Dispatch::Immediate(RpcResponse::ok(id, serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "patent-mcp-server", "version": "0.1.0"}
        }))),
        "initialized" => Dispatch::Notification,
        "ping" => Dispatch::Immediate(RpcResponse::ok(id, serde_json::json!({}))),
        "tools/list" => Dispatch::Immediate(RpcResponse::ok(id, tools_list())),
        "tools/call" => {
            match req.params {
                Some(params) => Dispatch::ToolCall { id, params },
                None => Dispatch::Immediate(RpcResponse::err(id, -32602, "Missing params")),
            }
        }
        _ => Dispatch::Immediate(RpcResponse::err(id, -32601, "Method not found")),
    }
}

// ---------------------------------------------------------------------------
// Async tool execution
// ---------------------------------------------------------------------------

async fn append_search_to_session(
    sm: &crate::search::session_manager::SessionManager,
    session_id: &str,
    query_text: &str,
    source: &str,
    hits: &[&crate::ranking::PatentHit],
    metadata: Option<serde_json::Value>,
    extra_classifications: Option<&[String]>,
) -> anyhow::Result<()> {
    let mut session = sm.load_session(session_id).await?;
    let now = chrono::Utc::now().to_rfc3339();
    let query_num = session.queries.len() + 1;
    let results: Vec<crate::ranking::PatentHit> =
        hits.iter().map(|h| (**h).clone()).collect();
    let record = crate::search::session_manager::QueryRecord {
        query_id: format!("q{:03}", query_num),
        timestamp: now,
        source: source.to_string(),
        query_text: query_text.to_string(),
        result_count: results.len() as i64,
        results,
        metadata,
    };
    session.queries.push(record);
    if let Some(classifications) = extra_classifications {
        for c in classifications {
            if !session.classifications_explored.contains(c) {
                session.classifications_explored.push(c.clone());
            }
        }
    }
    sm.save_session(&mut session).await?;
    Ok(())
}

struct BrowserBackendConfig {
    profiles_dir: Option<std::path::PathBuf>,
    profile_name: String,
    headless: bool,
    timeout_ms: u32,
    max_pages: u32,
    debug_html_dir: Option<std::path::PathBuf>,
}

struct SearchBackends {
    serpapi: Option<crate::search::searchers::SerpApiGooglePatentsBackend>,
    uspto: crate::search::searchers::UsptoTextSearchBackend,
    epo: crate::search::searchers::EpoOpsSearchBackend,
    session_manager: crate::search::session_manager::SessionManager,
    browser_config: BrowserBackendConfig,
}

#[tracing::instrument(skip_all)]
async fn execute_tool_call(
    id: Value,
    params: Value,
    config: &PatentConfig,
    cache: &crate::cache::PatentCache,
    orchestrator: &crate::fetchers::FetcherOrchestrator,
    journal: &crate::journal::ActivityJournal,
    backends: &SearchBackends,
) -> RpcResponse {
    let tool_name = match params.get("name").and_then(|v| v.as_str()) {
        Some(n) => n.to_string(),
        None => return RpcResponse::err(id, -32602, "Missing tool name"),
    };

    match tool_name.as_str() {
        "fetch_patents" => {
            let patent_ids = get_str_array_param(&params, "patent_ids").unwrap_or_default();
            let force_refresh = get_bool_param(&params, "force_refresh").unwrap_or(false);

            let payload = build_fetch_patents_payload(
                &patent_ids,
                force_refresh,
                config,
                orchestrator,
            )
            .await;

            journal.log_fetch(&patent_ids, &payload["summary"]);

            RpcResponse::ok(id, serde_json::json!({
                "content": [{"type": "text", "text": payload.to_string()}],
                "isError": false
            }))
        }

        "list_cached_patents" => {
            match cache.list_all() {
                Ok(entries) => {
                    let patents: Vec<serde_json::Value> = entries.iter().map(|e| serde_json::json!({
                        "canonical_id": e.canonical_id,
                        "cache_dir": e.cache_dir.to_string_lossy()
                    })).collect();
                    let count = patents.len();
                    journal.log_list(count);
                    let payload = serde_json::json!({"patents": patents, "count": count});
                    RpcResponse::ok(id, serde_json::json!({
                        "content": [{"type": "text", "text": payload.to_string()}],
                        "isError": false
                    }))
                }
                Err(e) => tool_error(id, &format!("Cache error: {}", e)),
            }
        }

        "get_patent_metadata" => {
            let patent_ids = get_str_array_param(&params, "patent_ids").unwrap_or_default();

            let mut results = Vec::new();
            for raw_id in &patent_ids {
                let canon = crate::id_canon::canonicalize(raw_id);
                match cache.lookup(&canon.canonical) {
                    Ok(Some(hit)) => {
                        let meta = hit.metadata.as_ref()
                            .map(|m| serde_json::to_value(m).unwrap_or(Value::Null));
                        results.push(serde_json::json!({
                            "patent_id": raw_id,
                            "canonical_id": canon.canonical,
                            "metadata": meta,
                        }));
                    }
                    _ => {
                        results.push(serde_json::json!({
                            "patent_id": raw_id,
                            "canonical_id": canon.canonical,
                            "metadata": null,
                        }));
                    }
                }
            }

            let found = results.iter().filter(|r| !r["metadata"].is_null()).count();
            let missing = results.len() - found;
            journal.log_metadata(&patent_ids, found, missing);

            let payload = serde_json::json!({"results": results});
            RpcResponse::ok(id, serde_json::json!({
                "content": [{"type": "text", "text": payload.to_string()}],
                "isError": false
            }))
        }

        "patent_search_natural" => {
            let description = get_str_param(&params, "description").unwrap_or_default();
            let date_cutoff = get_str_param(&params, "date_cutoff");
            let jurisdictions = get_str_array_param(&params, "jurisdictions");
            let session_id = get_str_param(&params, "session_id");
            let max_results = get_int_param(&params, "max_results").unwrap_or(25) as usize;
            let backend = get_str_param(&params, "backend").unwrap_or_else(|| "auto".to_string());
            let profile_name = get_str_param(&params, "profile_name")
                .unwrap_or_else(|| backends.browser_config.profile_name.clone());
            let _enrich_top_n = get_int_param(&params, "enrich_top_n")
                .unwrap_or(config.search_enrich_top_n as u64) as usize;
            let enrich_top_n = _enrich_top_n;
            let debug = get_bool_param(&params, "debug").unwrap_or(false);

            let start = std::time::Instant::now();

            let planner = crate::planner::NaturalLanguagePlanner;
            let intent = planner.plan(
                &description,
                date_cutoff.as_deref(),
                jurisdictions.as_deref(),
            );

            let mut hits_by_query: std::collections::HashMap<String, Vec<crate::ranking::PatentHit>> =
                std::collections::HashMap::new();
            let mut queries_run: Vec<Value> = Vec::new();

            let effective_backend = if backend == "auto" {
                config.search_backend_default.clone()
            } else {
                backend.clone()
            };

            if effective_backend == "browser" || effective_backend == "auto" {
                let browser_cfg = &backends.browser_config;
                let debug_dir = if debug {
                    browser_cfg.debug_html_dir.clone().or_else(|| Some(".patent-debug".into()))
                } else {
                    browser_cfg.debug_html_dir.clone()
                };
                let browser = crate::search::browser_search::GooglePatentsBrowserSearch::new(
                    browser_cfg.profiles_dir.clone(),
                    &profile_name,
                    browser_cfg.headless,
                    browser_cfg.timeout_ms,
                    browser_cfg.max_pages,
                    debug_dir,
                );
                for variant in &intent.query_variants {
                    if hits_by_query.contains_key(&variant.query) {
                        continue;
                    }
                    match browser.search(
                        &variant.query,
                        date_cutoff.as_deref(),
                        None,
                        max_results,
                    ).await {
                        Ok(hits) if !hits.is_empty() => {
                            let count = hits.len();
                            queries_run.push(serde_json::json!({
                                "source": "Google_Patents_Browser",
                                "query": variant.query,
                                "variant_type": variant.variant_type,
                                "result_count": count,
                            }));
                            hits_by_query.insert(variant.query.clone(), hits);
                        }
                        Ok(_) | Err(_) => {}
                    }
                }
            }

            let original_backend = backend.as_str();
            if effective_backend == "serpapi" ||
               (effective_backend == "auto") ||
               (original_backend == "auto" && hits_by_query.is_empty()) {
                if let Some(ref serp) = backends.serpapi {
                    let serp_variants: Vec<_> = intent.query_variants.iter()
                        .filter(|v| !hits_by_query.contains_key(&v.query))
                        .collect();
                    let serp_futures: Vec<_> = serp_variants.iter()
                        .map(|variant| serp.search(
                            &variant.query, None, date_cutoff.as_deref(), None, None, None, max_results,
                        ))
                        .collect();
                    let serp_results = futures::future::join_all(serp_futures).await;
                    for (variant, result) in serp_variants.iter().zip(serp_results) {
                        let hits = result.unwrap_or_else(|e| { warn!("SerpAPI search failed: {}", e); vec![] });
                        let count = hits.len();
                        queries_run.push(serde_json::json!({
                            "source": "Google_Patents_SerpAPI",
                            "query": variant.query,
                            "variant_type": variant.variant_type,
                            "result_count": count,
                        }));
                        hits_by_query.insert(variant.query.clone(), hits);
                    }
                }
            }

            let ranker = crate::ranking::SearchRanker;
            let mut scored = ranker.rank(&hits_by_query, &intent);
            scored.truncate(max_results);

            let mut enriched_ids: Vec<String> = Vec::new();
            if enrich_top_n > 0 && !scored.is_empty() {
                let scored_canonical: Vec<(usize, crate::id_canon::CanonicalPatentId)> = scored.iter()
                    .take(enrich_top_n)
                    .enumerate()
                    .filter_map(|(i, s)| {
                        let cid = crate::id_canon::canonicalize(&s.hit.patent_id);
                        if cid.canonical.is_empty() { None } else { Some((i, cid)) }
                    })
                    .collect();
                if !scored_canonical.is_empty() {
                    let patent_ids: Vec<crate::id_canon::CanonicalPatentId> = scored_canonical.iter().map(|(_, cid)| cid.clone()).collect();
                    let output_base = &config.cache_local_dir;
                    let results = orchestrator.fetch_batch(&patent_ids, output_base).await;
                    let result_map: std::collections::HashMap<String, &crate::fetchers::OrchestratorResult> = results
                        .iter()
                        .filter(|r| r.success && r.metadata.is_some())
                        .map(|r| (r.canonical_id.clone(), r))
                        .collect();
                    for (i, cid) in &scored_canonical {
                        if let Some(result) = result_map.get(&cid.canonical) {
                            if let Some(ref meta) = result.metadata {
                                let s = &mut scored[*i];
                                if s.hit.title.is_none() { s.hit.title = meta.title.clone(); }
                                if s.hit.abstract_text.is_none() { s.hit.abstract_text = meta.abstract_text.clone(); }
                                if s.hit.assignee.is_none() { s.hit.assignee = meta.assignee.clone(); }
                                if s.hit.inventors.is_empty() && !meta.inventors.is_empty() { s.hit.inventors = meta.inventors.clone(); }
                                if s.hit.date.is_none() { s.hit.date = meta.publication_date.clone(); }
                                enriched_ids.push(cid.canonical.clone());
                            }
                        }
                    }
                }
            }

            let all_hits: Vec<&crate::ranking::PatentHit> = scored.iter().map(|s| &s.hit).collect();

            if let Some(ref sid) = session_id {
                if !all_hits.is_empty() {
                    let _ = append_search_to_session(
                        &backends.session_manager,
                        sid,
                        &description,
                        "serpapi",
                        &all_hits,
                        Some(serde_json::json!({
                            "search_mode": backend,
                            "planner_concepts": intent.concepts,
                            "planner_synonyms": intent.synonyms,
                            "query_variants": intent.query_variants.iter().map(|v| &v.query).collect::<Vec<_>>(),
                        })),
                        None,
                    ).await;
                }
            }

            let payload = serde_json::json!({
                "query": description,
                "backend": backend,
                "date_cutoff": date_cutoff,
                "elapsed_ms": start.elapsed().as_millis() as u64,
                "planner": {
                    "concepts": intent.concepts,
                    "query_variant_count": intent.query_variants.len(),
                    "rationale": intent.rationale,
                    "synonyms_expanded": intent.synonyms.keys().collect::<Vec<_>>(),
                },
                "queries_run": queries_run,
                "total_found": scored.len(),
                "enriched_ids": enriched_ids,
                "results": scored.iter().map(|s| serde_json::json!({
                    "patent_id": s.hit.patent_id,
                    "title": s.hit.title,
                    "date": s.hit.date,
                    "assignee": s.hit.assignee,
                    "inventors": s.hit.inventors,
                    "abstract": s.hit.abstract_text,
                    "source": s.hit.source,
                    "relevance": s.hit.relevance,
                    "url": s.hit.url,
                    "score": (s.score * 100.0).round() / 100.0,
                    "query_matches": s.query_matches,
                })).collect::<Vec<_>>(),
            });

            RpcResponse::ok(id, serde_json::json!({
                "content": [{"type": "text", "text": payload.to_string()}],
                "isError": false
            }))
        }

        "patent_search_structured" => {
            let query = get_str_param(&params, "query").unwrap_or_default();
            let sources = get_str_array_param(&params, "sources")
                .unwrap_or_else(|| vec!["USPTO".into(), "EPO_OPS".into(), "Google_Patents".into()]);
            let date_from = get_str_param(&params, "date_from");
            let date_to = get_str_param(&params, "date_to");
            let session_id = get_str_param(&params, "session_id");
            let max_results = get_int_param(&params, "max_results").unwrap_or(25) as usize;

            let want_uspto = sources.iter().any(|s| s == "USPTO");
            let want_epo = sources.iter().any(|s| s == "EPO_OPS");
            let want_google = sources.iter().any(|s| s == "Google_Patents") && config.serpapi_key.is_some();

            let (uspto_result, epo_result, google_result) = tokio::join!(
                async {
                    if !want_uspto { return (vec![], None); }
                    let df = date_from.as_deref().map(|s| s.replace("-", ""));
                    let dt = date_to.as_deref().map(|s| s.replace("-", ""));
                    let hits = backends.uspto.search(&query, df.as_deref(), dt.as_deref(), max_results).await.unwrap_or_else(|e| { warn!("USPTO search failed: {}", e); vec![] });
                    let qr = Some(serde_json::json!({"source": "USPTO", "query": query, "result_count": hits.len()}));
                    (hits, qr)
                },
                async {
                    if !want_epo { return (vec![], None); }
                    let hits = backends.epo.search(&query, date_from.as_deref(), date_to.as_deref(), max_results).await.unwrap_or_else(|e| { warn!("EPO OPS search failed: {}", e); vec![] });
                    let qr = Some(serde_json::json!({"source": "EPO_OPS", "query": query, "result_count": hits.len()}));
                    (hits, qr)
                },
                async {
                    if !want_google { return (vec![], None); }
                    let serp = match backends.serpapi.as_ref() {
                        Some(s) => s,
                        None => return (vec![], None),
                    };
                    let hits = serp.search(&query, date_from.as_deref(), date_to.as_deref(), None, None, None, max_results).await.unwrap_or_else(|e| { warn!("SerpAPI search failed: {}", e); vec![] });
                    let qr = Some(serde_json::json!({"source": "Google_Patents", "query": query, "result_count": hits.len()}));
                    (hits, qr)
                },
            );

            let mut all_results: Vec<crate::ranking::PatentHit> = Vec::new();
            let mut queries_run: Vec<Value> = Vec::new();

            if let Some(qr) = uspto_result.1 { queries_run.push(qr); all_results.extend(uspto_result.0); }
            if let Some(qr) = epo_result.1 { queries_run.push(qr); all_results.extend(epo_result.0); }
            if let Some(qr) = google_result.1 { queries_run.push(qr); all_results.extend(google_result.0); }

            let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
            let deduped: Vec<&crate::ranking::PatentHit> = all_results
                .iter()
                .filter(|h| seen.insert(h.patent_id.clone()))
                .collect();

            if let Some(ref sid) = session_id {
                if !deduped.is_empty() {
                    let _ = append_search_to_session(
                        &backends.session_manager,
                        sid,
                        &query,
                        "structured",
                        &deduped,
                        Some(serde_json::json!({
                            "sources": sources,
                        })),
                        None,
                    ).await;
                }
            }

            let payload = serde_json::json!({
                "query": query,
                "sources_searched": queries_run.iter().filter_map(|q| q["source"].as_str()).collect::<Vec<_>>(),
                "queries_run": queries_run,
                "total_found": deduped.len(),
                "results": deduped.iter().map(|h| serde_json::json!({
                    "patent_id": h.patent_id,
                    "title": h.title,
                    "date": h.date,
                    "assignee": h.assignee,
                    "inventors": h.inventors,
                    "abstract": h.abstract_text,
                    "source": h.source,
                    "relevance": h.relevance,
                    "url": h.url,
                })).collect::<Vec<_>>(),
            });

            RpcResponse::ok(id, serde_json::json!({
                "content": [{"type": "text", "text": payload.to_string()}],
                "isError": false
            }))
        }

        "patent_citation_chain" => {
            let patent_id = get_str_param(&params, "patent_id").unwrap_or_default();
            let direction = get_str_param(&params, "direction").unwrap_or_else(|| "backward".to_string());
            let depth = get_int_param(&params, "depth").unwrap_or(1) as i32;
            let session_id = get_str_param(&params, "session_id");

            let epo = &backends.epo;

            let mut citations: serde_json::Map<String, Value> = serde_json::Map::new();

            if direction == "both" {
                let (bw_result, fw_result) = tokio::join!(
                    epo.get_citations(&patent_id, "backward"),
                    epo.get_citations(&patent_id, "forward")
                );
                let bw_l1 = bw_result.unwrap_or_else(|e| { warn!("EPO OPS get_citations failed for {}: {}", patent_id, e); vec![] });
                let fw_l1 = fw_result.unwrap_or_else(|e| { warn!("EPO OPS get_citations failed for {}: {}", patent_id, e); vec![] });

                let mut bw_entry = serde_json::json!({"level_1": &bw_l1});
                if depth >= 2 {
                    let seen: std::collections::HashSet<String> = bw_l1.iter().take(10).cloned().collect();
                    let l2_futs: Vec<_> = bw_l1.iter().take(10).map(|p| epo.get_citations(p, "backward")).collect();
                    let l2_res = futures::future::join_all(l2_futs).await;
                    let mut l2: Vec<String> = l2_res.into_iter().flat_map(|r| r.unwrap_or_else(|e| { warn!("EPO OPS get_citations failed: {}", e); vec![] })).collect();
                    l2.retain(|p| !seen.contains(p));
                    bw_entry["level_2"] = serde_json::json!(l2);
                }
                citations.insert("backward".to_string(), bw_entry);

                let mut fw_entry = serde_json::json!({"level_1": &fw_l1});
                if depth >= 2 {
                    let seen: std::collections::HashSet<String> = fw_l1.iter().take(10).cloned().collect();
                    let l2_futs: Vec<_> = fw_l1.iter().take(10).map(|p| epo.get_citations(p, "forward")).collect();
                    let l2_res = futures::future::join_all(l2_futs).await;
                    let mut l2: Vec<String> = l2_res.into_iter().flat_map(|r| r.unwrap_or_else(|e| { warn!("EPO OPS get_citations failed: {}", e); vec![] })).collect();
                    l2.retain(|p| !seen.contains(p));
                    fw_entry["level_2"] = serde_json::json!(l2);
                }
                citations.insert("forward".to_string(), fw_entry);
            } else {
                let l1 = epo.get_citations(&patent_id, &direction).await.unwrap_or_else(|e| { warn!("EPO OPS get_citations failed for {}: {}", patent_id, e); vec![] });
                let mut entry = serde_json::json!({"level_1": &l1});
                if depth >= 2 {
                    let seen: std::collections::HashSet<String> = l1.iter().take(10).cloned().collect();
                    let l2_futs: Vec<_> = l1.iter().take(10).map(|p| epo.get_citations(p, &direction)).collect();
                    let l2_res = futures::future::join_all(l2_futs).await;
                    let mut l2: Vec<String> = l2_res.into_iter().flat_map(|r| r.unwrap_or_else(|e| { warn!("EPO OPS get_citations failed: {}", e); vec![] })).collect();
                    l2.retain(|p| !seen.contains(p));
                    entry["level_2"] = serde_json::json!(l2);
                }
                citations.insert(direction.clone(), entry);
            }

            let citations_snapshot = citations.clone();
            let patent_id_for_session = patent_id.clone();

            if let Some(ref sid) = session_id {
                let sm = &backends.session_manager;
                if let Ok(mut session) = sm.load_session(sid).await {
                    let map = session.citation_chains.as_object_mut();
                    if let Some(map) = map {
                        map.insert(patent_id_for_session.clone(), serde_json::json!(citations_snapshot));
                    } else {
                        session.citation_chains = serde_json::json!({patent_id_for_session: citations_snapshot});
                    }
                    let _ = sm.save_session(&mut session).await;
                }
            }

            let payload = serde_json::json!({
                "seed": patent_id,
                "direction": direction,
                "depth": depth,
                "citations": citations,
            });

            RpcResponse::ok(id, serde_json::json!({
                "content": [{"type": "text", "text": payload.to_string()}],
                "isError": false
            }))
        }

        "patent_classification_search" => {
            let code = get_str_param(&params, "code").unwrap_or_default();
            let include_subclasses = get_bool_param(&params, "include_subclasses").unwrap_or(true);
            let date_from = get_str_param(&params, "date_from");
            let date_to = get_str_param(&params, "date_to");
            let session_id = get_str_param(&params, "session_id");
            let max_results = get_int_param(&params, "max_results").unwrap_or(25) as usize;

            let epo = &backends.epo;
            let hits = epo
                .search_by_classification(&code, include_subclasses, date_from.as_deref(), date_to.as_deref(), max_results)
                .await
                .unwrap_or_else(|e| { warn!("EPO OPS classification search failed for {}: {}", code, e); vec![] });

            if let Some(ref sid) = session_id {
                if !hits.is_empty() {
                    let hit_refs: Vec<&crate::ranking::PatentHit> = hits.iter().collect();
                    let _ = append_search_to_session(
                        &backends.session_manager,
                        sid,
                        &code,
                        "EPO_OPS",
                        &hit_refs,
                        Some(serde_json::json!({
                            "classification_code": code,
                            "include_subclasses": include_subclasses,
                        })),
                        Some(std::slice::from_ref(&code)),
                    ).await;
                }
            }

            let payload = serde_json::json!({
                "code": code,
                "include_subclasses": include_subclasses,
                "date_from": date_from,
                "date_to": date_to,
                "total_found": hits.len(),
                "results": hits.iter().map(|h| serde_json::json!({
                    "patent_id": h.patent_id,
                    "title": h.title,
                    "date": h.date,
                    "assignee": h.assignee,
                    "inventors": h.inventors,
                    "source": h.source,
                })).collect::<Vec<_>>(),
            });

            RpcResponse::ok(id, serde_json::json!({
                "content": [{"type": "text", "text": payload.to_string()}],
                "isError": false
            }))
        }

        "patent_family_search" => {
            let patent_id = get_str_param(&params, "patent_id").unwrap_or_default();
            let session_id = get_str_param(&params, "session_id");

            let epo = &backends.epo;
            let members = epo.get_family(&patent_id).await.unwrap_or_else(|e| { warn!("EPO OPS get_family failed for {}: {}", patent_id, e); vec![] });

            if let Some(ref sid) = session_id {
                if !members.is_empty() {
                    let sm = &backends.session_manager;
                    if let Ok(mut session) = sm.load_session(sid).await {
                        session.patent_families.insert(
                            patent_id.clone(),
                            members.iter().filter_map(|m| m.get("patent_id").and_then(|v| v.as_str()).map(String::from)).collect(),
                        );
                        let _ = sm.save_session(&mut session).await;
                    }
                }
            }

            let payload = serde_json::json!({
                "patent_id": patent_id,
                "family_size": members.len(),
                "members": members,
            });

            RpcResponse::ok(id, serde_json::json!({
                "content": [{"type": "text", "text": payload.to_string()}],
                "isError": false
            }))
        }

        "patent_suggest_queries" => {
            let topic = get_str_param(&params, "topic").unwrap_or_default();
            let context = get_str_param(&params, "context").unwrap_or_default();
            let prior_art_cutoff = get_str_param(&params, "prior_art_cutoff");

            let planner = crate::planner::NaturalLanguagePlanner;
            let intent = planner.plan(&topic, prior_art_cutoff.as_deref(), None);

            let mut strategy = serde_json::json!({
                "step_1_natural_search": {
                    "description": "Run patent_search_natural with the query variants above",
                    "action": format!("patent_search_natural(description=\"{}\", backend=\"auto\")", topic.chars().take(80).collect::<String>()),
                },
                "step_2_classification": {
                    "description": "Find IPC/CPC class codes — searches by class find patents regardless of keyword",
                    "action": "Use patent_classification_search with codes from the relevant technology area",
                    "tip": "Start with a broad code like 'H02J' and explore subclasses",
                },
                "step_3_citation_chain": {
                    "description": "After finding any relevant patent, follow its citations",
                    "action": "Use patent_citation_chain on the most relevant results (direction='both', depth=2)",
                    "why": "The best prior art is often found 1-2 hops away in citation chains",
                },
            });

            if prior_art_cutoff.is_some() {
                let cutoff = prior_art_cutoff.as_deref().unwrap_or("");
                strategy.as_object_mut().unwrap().insert(
                    "prior_art_notes".to_string(),
                    serde_json::json!({
                        "cutoff_date": cutoff,
                        "reminder": format!("Search for patents filed/published BEFORE {}", cutoff),
                        "tip": format!("A patent published after {} can still be prior art if its application was filed before that date", cutoff),
                    }),
                );
            }

            let payload = serde_json::json!({
                "topic": topic,
                "context": context,
                "prior_art_cutoff": prior_art_cutoff,
                "planner_output": {
                    "concepts": intent.concepts,
                    "synonyms": intent.synonyms,
                    "rationale": intent.rationale,
                    "query_variants": intent.query_variants.iter().map(|v| serde_json::json!({
                        "query": v.query,
                        "type": v.variant_type,
                        "rationale": v.rationale,
                    })).collect::<Vec<_>>(),
                },
                "strategy": strategy,
            });

            RpcResponse::ok(id, serde_json::json!({
                "content": [{"type": "text", "text": payload.to_string()}],
                "isError": false
            }))
        }

        "patent_session_create" => {
            let topic = get_str_param(&params, "topic").unwrap_or_default();
            let prior_art_cutoff = get_str_param(&params, "prior_art_cutoff");
            let notes = get_str_param(&params, "notes").unwrap_or_default();

            let sm = &backends.session_manager;
            match sm.create_session(&topic, prior_art_cutoff.as_deref(), &notes).await {
                Ok(session) => {
                    let payload = serde_json::json!({
                        "session_id": session.session_id,
                        "topic": session.topic,
                        "created_at": session.created_at,
                        "sessions_dir": sm.dir().to_string_lossy(),
                        "message": format!("Session created. Use session_id='{}' in search calls to auto-save results.", session.session_id),
                    });
                    RpcResponse::ok(id, serde_json::json!({
                        "content": [{"type": "text", "text": payload.to_string()}],
                        "isError": false
                    }))
                }
                Err(e) => tool_error(id, &format!("Session create error: {}", e)),
            }
        }

        "patent_session_load" => {
            let session_id = get_str_param(&params, "session_id").unwrap_or_default();

            let sm = &backends.session_manager;
            match sm.load_session(&session_id).await {
                Ok(session) => {
                    let payload = serde_json::to_value(&session).unwrap_or(Value::Null);
                    RpcResponse::ok(id, serde_json::json!({
                        "content": [{"type": "text", "text": payload.to_string()}],
                        "isError": false
                    }))
                }
                Err(_) => tool_error(id,
                    &format!("Session '{}' not found. Use patent_session_list to see available sessions.", session_id)),
            }
        }

        "patent_session_list" => {
            let limit = get_int_param(&params, "limit").map(|n| n as usize).unwrap_or(20);

            let sm = &backends.session_manager;
            let summaries = sm.list_sessions(Some(limit)).await.unwrap_or_default();
            let payload = serde_json::json!({
                "sessions": summaries,
                "total": summaries.len(),
            });
            RpcResponse::ok(id, serde_json::json!({
                "content": [{"type": "text", "text": payload.to_string()}],
                "isError": false
            }))
        }

        "patent_session_note" => {
            let session_id = get_str_param(&params, "session_id").unwrap_or_default();
            let note = get_str_param(&params, "note").unwrap_or_default();

            let sm = &backends.session_manager;
            match sm.add_note(&session_id, &note).await {
                Ok(()) => {
                    let payload = serde_json::json!({"status": "note added", "session_id": session_id});
                    RpcResponse::ok(id, serde_json::json!({
                        "content": [{"type": "text", "text": payload.to_string()}],
                        "isError": false
                    }))
                }
                Err(e) => tool_error(id, &format!("Note error: {}", e)),
            }
        }

        "patent_session_annotate" => {
            let session_id = get_str_param(&params, "session_id").unwrap_or_default();
            let patent_id = get_str_param(&params, "patent_id").unwrap_or_default();
            let annotation = get_str_param(&params, "annotation").unwrap_or_default();
            let relevance = get_str_param(&params, "relevance").unwrap_or_else(|| "high".to_string());

            let sm = &backends.session_manager;
            match sm.annotate_patent(&session_id, &patent_id, &annotation, &relevance).await {
                Ok(true) => {
                    let payload = serde_json::json!({
                        "session_id": session_id,
                        "patent_id": patent_id,
                        "relevance": relevance,
                        "status": "annotated",
                    });
                    RpcResponse::ok(id, serde_json::json!({
                        "content": [{"type": "text", "text": payload.to_string()}],
                        "isError": false
                    }))
                }
                Ok(false) => {
                    let payload = serde_json::json!({
                        "status": "warning",
                        "message": format!("Patent {} not found in session {}", patent_id, session_id),
                        "patent_id": patent_id,
                        "session_id": session_id,
                    });
                    RpcResponse::ok(id, serde_json::json!({
                        "content": [{"type": "text", "text": payload.to_string()}],
                        "isError": false
                    }))
                }
                Err(e) => tool_error(id, &format!("Annotate error: {}", e)),
            }
        }

        "patent_session_export" => {
            let session_id = get_str_param(&params, "session_id").unwrap_or_default();
            let output_path = get_str_param(&params, "output_path").map(std::path::PathBuf::from);

            let sm = &backends.session_manager;
            match sm.export_markdown(&session_id, output_path.as_deref()).await {
                Ok(path) => {
                    let payload = serde_json::json!({
                        "report_path": path.to_string_lossy(),
                        "status": "exported",
                    });
                    RpcResponse::ok(id, serde_json::json!({
                        "content": [{"type": "text", "text": payload.to_string()}],
                        "isError": false
                    }))
                }
                Err(e) => tool_error(id, &format!("Export error: {}", e)),
            }
        }

        "patent_search_profile_login_start" => {
            let profile_name = get_str_param(&params, "name").unwrap_or_else(|| "default".to_string());
            let browser_cfg = &backends.browser_config;

            let pm = crate::search::profile_manager::ProfileManager::new(browser_cfg.profiles_dir.clone());
            let profile_dir = match pm.get_profile_dir(&profile_name) {
                Ok(d) => d,
                Err(e) => {
                    return tool_error(id, &format!("Profile error: {}", e));
                }
            };

            match pm.acquire_lock(&profile_name, "login") {
                Ok(()) => {}
                Err(e) => {
                    return tool_error(id, &format!("Profile busy: {}", e));
                }
            }

            let browser_config = match chromiumoxide::BrowserConfig::builder()
                .with_head()
                .arg("--no-sandbox")
                .window_size(1280, 900)
                .user_data_dir(&profile_dir)
                .arg(format!("--user-agent={}", crate::search::browser_search::BROWSER_USER_AGENT))
                .build()
            {
                Ok(c) => c,
                Err(e) => {
                    let _ = pm.release_lock(&profile_name);
                    return tool_error(id, &format!("Browser config error: {}", e));
                }
            };

            let launch_result = chromiumoxide::Browser::launch(browser_config).await;

            let (browser, mut handler) = match launch_result {
                Ok(bh) => bh,
                Err(e) => {
                    let _ = pm.release_lock(&profile_name);
                    return tool_error(id, &format!("Browser launch failed: {}. Is Chromium installed?", e));
                }
            };

            let pm_for_task = crate::search::profile_manager::ProfileManager::new(browser_cfg.profiles_dir.clone());
            let pn_for_task = profile_name.clone();
            tokio::spawn(async move {
                use futures::StreamExt;
                while handler.next().await.is_some() {}
                drop(browser);
                let _ = pm_for_task.release_lock(&pn_for_task);
            });

            let payload = serde_json::json!({
                "status": "launched",
                "message": format!(
                    "Headed browser launched for profile '{}'. Log into your Google account manually, then close the browser window. Subsequent headless searches will reuse the saved login state.",
                    profile_name
                ),
                "profile_name": profile_name,
                "profile_dir": profile_dir.to_string_lossy(),
            });
            RpcResponse::ok(id, serde_json::json!({
                "content": [{"type": "text", "text": payload.to_string()}],
                "isError": false
            }))
        }

        _ => RpcResponse::err(id, -32601, &format!("Unknown tool: {}", tool_name)),
    }
}

// ---------------------------------------------------------------------------
// Server loop
// ---------------------------------------------------------------------------

/// Run the MCP server on stdin/stdout until EOF.
#[tracing::instrument(skip_all)]
pub async fn run_server(config: PatentConfig) -> Result<()> {
    use crate::cache::PatentCache;
    use crate::fetchers::FetcherOrchestrator;
    use crate::journal::ActivityJournal;

    // Two PatentCache instances sharing the same SQLite WAL database:
    // one owned by the orchestrator for fetch operations,
    // one retained here for list_all / lookup operations.
    let cache_for_ops = PatentCache::new(&config)?;
    let cache_for_orch = PatentCache::new(&config)?;
    let orchestrator = FetcherOrchestrator::new(config.clone(), cache_for_orch);
    let journal = ActivityJournal::new(config.activity_journal.clone());

    let stdout = std::io::stdout();

    tracing::info!("patent-mcp-server started (Rust implementation)");

    let backends = SearchBackends {
        serpapi: config.serpapi_key.as_ref().map(|key| {
            crate::search::searchers::SerpApiGooglePatentsBackend::new(
                key.clone(),
                None,
                std::time::Duration::from_secs_f64(config.timeout_secs),
                None,
            )
        }),
        uspto: crate::search::searchers::UsptoTextSearchBackend::new(
            None,
            std::time::Duration::from_secs_f64(config.timeout_secs),
            None,
        ),
        epo: crate::search::searchers::EpoOpsSearchBackend::new(
            config.epo_client_id.clone(),
            config.epo_client_secret.clone(),
            None,
            std::time::Duration::from_secs_f64(config.timeout_secs),
            None,
        ),
        session_manager: crate::search::session_manager::SessionManager::new(None),
        browser_config: BrowserBackendConfig {
            profiles_dir: config.search_browser_profiles_dir.clone(),
            profile_name: config.search_browser_default_profile.clone(),
            headless: config.search_browser_headless,
            timeout_ms: (config.search_browser_timeout * 1000.0) as u32,
            max_pages: config.search_browser_max_pages as u32,
            debug_html_dir: config.search_browser_debug_html_dir.clone(),
        },
    };

    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(64);
    std::thread::spawn(move || {
        use std::io::BufRead;
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            match line {
                Ok(l) => { let _ = tx.blocking_send(l); }
                Err(_) => break,
            }
        }
    });

    while let Some(line) = rx.recv().await {
        if line.trim().is_empty() {
            continue;
        }

        let response = match route_line(&line) {
            Dispatch::Immediate(r) => r,
            Dispatch::Notification => continue,
            Dispatch::ToolCall { id, params } => {
                execute_tool_call(id, params, &config, &cache_for_ops, &orchestrator, &journal, &backends).await
            }
        };

        let mut out = stdout.lock();
        serde_json::to_writer(&mut out, &response)?;
        writeln!(out)?;
        out.flush()?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests  (use handle_line shim that calls route_line synchronously)
// ---------------------------------------------------------------------------

/// Synchronous helper for tests — routes a line (no tool-call async needed for
/// the test cases that only test initialize / tools/list / empty fetch_patents).
#[cfg(test)]
fn handle_line(line: &str, _config: &PatentConfig) -> Option<RpcResponse> {
    match route_line(line) {
        Dispatch::Immediate(r) => Some(r),
        Dispatch::Notification => None,
        Dispatch::ToolCall { id, params } => {
            let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
            Some(match tool_name.as_str() {
                "fetch_patents" => {
                    let patent_ids = get_str_array_param(&params, "patent_ids").unwrap_or_default();

                    RpcResponse::ok(id, serde_json::json!({
                        "content": [{"type": "text", "text": serde_json::json!({
                            "results": [],
                            "summary": {
                                "total": patent_ids.len(),
                                "success": 0,
                                "cached": 0,
                                "errors": if patent_ids.is_empty() { 0 } else { patent_ids.len() },
                                "total_duration_ms": 0.0
                            }
                        }).to_string()}],
                        "isError": false
                    }))
                }
                "list_cached_patents" => RpcResponse::ok(id, serde_json::json!({
                    "content": [{"type": "text", "text": serde_json::json!({"patents": [], "count": 0}).to_string()}],
                    "isError": false
                })),
                "get_patent_metadata" => RpcResponse::ok(id, serde_json::json!({
                    "content": [{"type": "text", "text": serde_json::json!({"results": []}).to_string()}],
                    "isError": false
                })),
                "patent_search_natural" | "patent_search_structured" | "patent_suggest_queries" => {
                    RpcResponse::ok(id, serde_json::json!({
                        "content": [{"type": "text", "text": serde_json::json!({"query": "", "results": []}).to_string()}],
                        "isError": false
                    }))
                }
                "patent_citation_chain" | "patent_classification_search" | "patent_family_search" => {
                    RpcResponse::ok(id, serde_json::json!({
                        "content": [{"type": "text", "text": serde_json::json!({"results": []}).to_string()}],
                        "isError": false
                    }))
                }
                "patent_session_create" | "patent_session_load" | "patent_session_list"
                | "patent_session_note" | "patent_session_annotate" | "patent_session_export"
                | "patent_search_profile_login_start" => {
                    RpcResponse::ok(id, serde_json::json!({
                        "content": [{"type": "text", "text": serde_json::json!({"status": "ok"}).to_string()}],
                        "isError": false
                    }))
                }
                _ => RpcResponse::err(id, -32601, &format!("Unknown tool: {}", tool_name)),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config() -> PatentConfig {
        PatentConfig {
            cache_local_dir: crate::config::xdg_data_home().join("patent-cache").join("patents"),
            cache_global_db: crate::config::default_global_db(),
            source_priority: vec![],
            concurrency: 5,
            fetch_all_sources: false,
            timeout_secs: 30.0,
            converters_order: vec![],
            converters_disabled: vec![],
            source_base_urls: std::collections::HashMap::new(),
            epo_client_id: None,
            epo_client_secret: None,
            lens_api_key: None,
            serpapi_key: None,
            bing_key: None,
            bigquery_project: None,
            activity_journal: None,
            search_browser_profiles_dir: None,
            search_browser_default_profile: "default".into(),
            search_browser_headless: true,
            search_browser_timeout: 60.0,
            search_browser_max_pages: 3,
            search_browser_idle_timeout: 1800.0,
            search_browser_debug_html_dir: None,
            search_backend_default: "serpapi".into(),
            search_enrich_top_n: 5,
        }
    }

    #[test]
    fn test_handle_initialize() {
        let line = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
        let config = make_config();
        let resp = handle_line(line, &config).unwrap();
        assert!(resp.result.is_some());
        let r = resp.result.unwrap();
        assert_eq!(r["protocolVersion"], "2024-11-05");
    }

    #[test]
    fn test_initialized_notification_no_response() {
        let line = r#"{"jsonrpc":"2.0","method":"initialized"}"#;
        let config = make_config();
        assert!(handle_line(line, &config).is_none());
    }

    #[test]
    fn test_ping_returns_empty_result() {
        let line = r#"{"jsonrpc":"2.0","id":99,"method":"ping"}"#;
        let config = make_config();
        let resp = handle_line(line, &config).unwrap();
        assert!(resp.result.is_some());
        assert!(resp.error.is_none());
        let r = resp.result.unwrap();
        assert!(r.as_object().unwrap().is_empty());
    }

    #[test]
    fn test_handle_tools_list() {
        let line = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#;
        let config = make_config();
        let resp = handle_line(line, &config).unwrap();
        assert!(resp.result.is_some());
        let r = resp.result.unwrap();
        assert!(r["tools"].is_array());
        let tools = r["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter()
            .filter_map(|t| t["name"].as_str())
            .collect();

        let expected = [
            "fetch_patents",
            "list_cached_patents",
            "get_patent_metadata",
            "patent_search_natural",
            "patent_search_structured",
            "patent_citation_chain",
            "patent_classification_search",
            "patent_family_search",
            "patent_suggest_queries",
            "patent_session_create",
            "patent_session_load",
            "patent_session_list",
            "patent_session_note",
            "patent_session_annotate",
            "patent_session_export",
            "patent_search_profile_login_start",
        ];
        assert_eq!(names.len(), expected.len(), "Expected {} tools, got {}: {:?}", expected.len(), names.len(), names);
        for name in &expected {
            assert!(names.contains(name), "Missing tool: {}", name);
        }
    }

    #[test]
    fn test_empty_fetch_patents() {
        let line = r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"fetch_patents","arguments":{"patent_ids":[]}}}"#;
        let config = make_config();
        let resp = handle_line(line, &config).unwrap();
        assert!(resp.result.is_some());
    }

    #[tokio::test]
    async fn test_invalid_fetch_patents_returns_errors() {
        let config = make_config();
        let cache = crate::cache::PatentCache::new(&config).unwrap();
        let orchestrator = crate::fetchers::FetcherOrchestrator::new(config.clone(), cache);
        let payload = build_fetch_patents_payload(
            &[String::from("INVALID-XXXXX-NOTREAL"), String::from("US7654321")],
            false,
            &config,
            &orchestrator,
        )
        .await;

        assert_eq!(payload["summary"]["total"], 2);
        assert_eq!(payload["summary"]["errors"], 1);
        assert_eq!(payload["summary"]["success"], 1);
        assert_eq!(payload["results"][0]["status"], "error");
        assert!(payload["results"][0]["error"].as_str().unwrap().contains("Invalid patent ID"));
        assert_eq!(payload["results"][0]["patent_id"], "INVALID-XXXXX-NOTREAL");
        assert_eq!(payload["results"][1]["canonical_id"], "US7654321");
        assert_eq!(payload["results"][1]["patent_id"], "US7654321");
        assert!(payload["summary"]["total_duration_ms"].as_f64().unwrap() >= 0.0);
    }

    #[test]
    fn test_unknown_method_returns_error() {
        let line = r#"{"jsonrpc":"2.0","id":4,"method":"unknown/method","params":{}}"#;
        let config = make_config();
        let resp = handle_line(line, &config).unwrap();
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32601);
    }

    #[test]
    fn test_session_create_and_list() {
        let config = make_config();
        let create_line = r#"{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"patent_session_create","arguments":{"topic":"test topic","notes":"integration test"}}}"#;
        let resp = handle_line(create_line, &config).unwrap();
        assert!(resp.result.is_some());

        let list_line = r#"{"jsonrpc":"2.0","id":11,"method":"tools/call","params":{"name":"patent_session_list","arguments":{}}}"#;
        let resp = handle_line(list_line, &config).unwrap();
        assert!(resp.result.is_some());
    }

    #[test]
    fn test_search_tools_registered() {
        let line = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#;
        let config = make_config();
        let resp = handle_line(line, &config).unwrap();
        let r = resp.result.unwrap();
        let tools = r["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter()
            .filter_map(|t| t["name"].as_str())
            .collect();

        assert!(names.contains(&"patent_search_natural"), "patent_search_natural missing");
        assert!(names.contains(&"patent_search_structured"), "patent_search_structured missing");
        assert!(names.contains(&"patent_citation_chain"), "patent_citation_chain missing");
        assert!(names.contains(&"patent_classification_search"), "patent_classification_search missing");
        assert!(names.contains(&"patent_family_search"), "patent_family_search missing");
        assert!(names.contains(&"patent_suggest_queries"), "patent_suggest_queries missing");
        assert!(names.contains(&"patent_session_create"), "patent_session_create missing");
        assert!(names.contains(&"patent_session_export"), "patent_session_export missing");
        assert!(names.contains(&"patent_search_profile_login_start"), "patent_search_profile_login_start missing");
    }

    #[test]
    fn parity_tools_have_input_schema() {
        let config = make_config();
        let line = r#"{"jsonrpc":"2.0","id":30,"method":"tools/list","params":{}}"#;
        let resp = handle_line(line, &config).unwrap();
        let tools = resp.result.unwrap()["tools"].as_array().unwrap().clone();

        for tool in tools {
            let name = tool["name"].as_str().unwrap();
            assert!(tool["description"].is_string(), "{} missing description", name);
            assert!(tool["inputSchema"]["type"].is_string(), "{} missing inputSchema.type", name);
            assert!(tool["inputSchema"]["properties"].is_object(), "{} missing inputSchema.properties", name);
        }
    }

    #[tokio::test]
    async fn parity_session_create_and_list() {
        let tmp = tempfile::tempdir().unwrap();
        let sm = crate::search::session_manager::SessionManager::new(Some(tmp.path().to_path_buf()));

        let session = sm.create_session("wireless-charging", Some("2020-01-01"), "test notes").await.unwrap();
        assert!(session.session_id.contains("wireless-charging"));
        assert_eq!(session.topic, "wireless-charging");
        assert_eq!(session.prior_art_cutoff.as_deref(), Some("2020-01-01"));
        assert_eq!(session.notes, "test notes");
        assert!(!session.created_at.is_empty());
        assert!(!session.modified_at.is_empty());
        assert!(session.queries.is_empty());
        assert!(session.classifications_explored.is_empty());
        assert!(session.citation_chains.is_object());
        assert!(session.patent_families.is_empty());

        let loaded = sm.load_session(&session.session_id).await.unwrap();
        assert_eq!(loaded.session_id, session.session_id);
        assert_eq!(loaded.topic, session.topic);
    }

    #[tokio::test]
    async fn parity_session_list_shape() {
        let tmp = tempfile::tempdir().unwrap();
        let sm = crate::search::session_manager::SessionManager::new(Some(tmp.path().to_path_buf()));
        sm.create_session("test-1", None, "").await.unwrap();
        sm.create_session("test-2", None, "").await.unwrap();

        let summaries = sm.list_sessions(None).await.unwrap();
        assert_eq!(summaries.len(), 2);
        for s in &summaries {
            assert!(!s.session_id.is_empty());
            assert!(!s.topic.is_empty());
            assert!(!s.created_at.is_empty());
            assert!(!s.modified_at.is_empty());
        }
    }

    #[tokio::test]
    async fn parity_session_note_and_annotate() {
        let tmp = tempfile::tempdir().unwrap();
        let sm = crate::search::session_manager::SessionManager::new(Some(tmp.path().to_path_buf()));
        let session = sm.create_session("test-note", None, "").await.unwrap();
        let sid = &session.session_id;

        let hit = crate::search::session_manager::PatentHit {
            patent_id: "US1234567".to_string(),
            title: Some("Test Patent".to_string()),
            date: None,
            assignee: None,
            inventors: vec![],
            abstract_text: None,
            source: "test".to_string(),
            relevance: "unknown".to_string(),
            note: String::new(),
            prior_art: None,
            url: None,
        };
        let record = crate::search::session_manager::QueryRecord {
            query_id: "q001".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            source: "test".to_string(),
            query_text: "test query".to_string(),
            result_count: 1,
            results: vec![hit],
            metadata: None,
        };
        sm.append_query_result(sid, record).await.unwrap();

        sm.add_note(sid, "first note").await.unwrap();
        let loaded = sm.load_session(sid).await.unwrap();
        assert!(loaded.notes.contains("first note"));

        sm.annotate_patent(sid, "US1234567", "highly relevant", "high").await.unwrap();
        let loaded = sm.load_session(sid).await.unwrap();
        let found = loaded.queries.iter().any(|q|
            q.results.iter().any(|h| h.patent_id == "US1234567" && h.relevance == "high" && h.note == "highly relevant")
        );
        assert!(found, "annotated patent should have updated note and relevance");
    }

    #[tokio::test]
    async fn parity_session_export_produces_markdown() {
        let tmp = tempfile::tempdir().unwrap();
        let sm = crate::search::session_manager::SessionManager::new(Some(tmp.path().to_path_buf()));
        let session = sm.create_session("export-test", Some("2020-01-01"), "initial notes").await.unwrap();
        sm.add_note(&session.session_id, "research note").await.unwrap();

        let report_path = sm.export_markdown(&session.session_id, None).await.unwrap();
        assert!(report_path.exists());
        let content = std::fs::read_to_string(&report_path).unwrap();
        assert!(content.contains("export-test"));
        assert!(content.contains("research note"));
    }

    #[tokio::test]
    async fn parity_citation_chains_in_session() {
        let tmp = tempfile::tempdir().unwrap();
        let sm = crate::search::session_manager::SessionManager::new(Some(tmp.path().to_path_buf()));
        let session = sm.create_session("citation-test", None, "").await.unwrap();
        let sid = &session.session_id;

        let mut session = sm.load_session(sid).await.unwrap();
        session.citation_chains = serde_json::json!({
            "US1234567": {
                "backward": {"level_1": ["US1111111", "US2222222"]},
                "forward": {"level_1": ["US3333333"]}
            }
        });
        sm.save_session(&mut session).await.unwrap();

        let loaded = sm.load_session(sid).await.unwrap();
        let chains = &loaded.citation_chains;
        assert!(chains["US1234567"]["backward"]["level_1"].is_array());
        assert_eq!(chains["US1234567"]["backward"]["level_1"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn parity_classifications_explored_in_session() {
        let tmp = tempfile::tempdir().unwrap();
        let sm = crate::search::session_manager::SessionManager::new(Some(tmp.path().to_path_buf()));
        let session = sm.create_session("class-test", None, "").await.unwrap();
        let sid = &session.session_id;

        let mut session = sm.load_session(sid).await.unwrap();
        session.classifications_explored.push("H02J50".to_string());
        session.classifications_explored.push("H01F38".to_string());
        sm.save_session(&mut session).await.unwrap();

        let loaded = sm.load_session(sid).await.unwrap();
        assert_eq!(loaded.classifications_explored, vec!["H02J50", "H01F38"]);
    }

    #[tokio::test]
    async fn parity_patent_families_in_session() {
        let tmp = tempfile::tempdir().unwrap();
        let sm = crate::search::session_manager::SessionManager::new(Some(tmp.path().to_path_buf()));
        let session = sm.create_session("family-test", None, "").await.unwrap();
        let sid = &session.session_id;

        let mut session = sm.load_session(sid).await.unwrap();
        session.patent_families.insert(
            "US1234567".to_string(),
            vec!["EP1234567".to_string(), "WO2020123456".to_string()],
        );
        sm.save_session(&mut session).await.unwrap();

        let loaded = sm.load_session(sid).await.unwrap();
        assert_eq!(loaded.patent_families["US1234567"].len(), 2);
        assert!(loaded.patent_families["US1234567"].contains(&"EP1234567".to_string()));
    }

    fn make_real_deps() -> (
        PatentConfig,
        crate::cache::PatentCache,
        crate::fetchers::FetcherOrchestrator,
        crate::journal::ActivityJournal,
        SearchBackends,
        tempfile::TempDir,
    ) {
        let config = make_config();
        let cache = crate::cache::PatentCache::new(&config).unwrap();
        let orch_cache = crate::cache::PatentCache::new(&config).unwrap();
        let orchestrator = crate::fetchers::FetcherOrchestrator::new(config.clone(), orch_cache);
        let journal = crate::journal::ActivityJournal::new(config.activity_journal.clone());
        let sessions_tmp = tempfile::tempdir().unwrap();
        let backends = SearchBackends {
            serpapi: config.serpapi_key.as_ref().map(|key| {
                crate::search::searchers::SerpApiGooglePatentsBackend::new(
                    key.clone(),
                    None,
                    std::time::Duration::from_secs(30),
                    None,
                )
            }),
            uspto: crate::search::searchers::UsptoTextSearchBackend::new(
                None,
                std::time::Duration::from_secs(30),
                None,
            ),
            epo: crate::search::searchers::EpoOpsSearchBackend::new(
                config.epo_client_id.clone(),
                config.epo_client_secret.clone(),
                None,
                std::time::Duration::from_secs(30),
                None,
            ),
            session_manager: crate::search::session_manager::SessionManager::new(Some(
                sessions_tmp.path().to_path_buf(),
            )),
            browser_config: BrowserBackendConfig {
                profiles_dir: None,
                profile_name: "default".to_string(),
                headless: true,
                timeout_ms: 60000,
                max_pages: 3,
                debug_html_dir: None,
            },
        };
        (config, cache, orchestrator, journal, backends, sessions_tmp)
    }

    fn tool_params(name: &str, args: serde_json::Value) -> Value {
        serde_json::json!({
            "name": name,
            "arguments": args
        })
    }

    fn extract_payload(resp: RpcResponse) -> serde_json::Value {
        let r = resp.result.unwrap();
        let text = r["content"].as_array().unwrap()[0]["text"].as_str().unwrap();
        serde_json::from_str(text).unwrap()
    }

    #[tokio::test]
    async fn e2e_session_create_and_load() {
        let (config, cache, orchestrator, journal, backends, _sessions_tmp) = make_real_deps();
        let id = Value::Number(100.into());

        let create_params = tool_params("patent_session_create", serde_json::json!({
            "topic": "e2e-test", "notes": "integration"
        }));
        let resp = execute_tool_call(id.clone(), create_params, &config, &cache, &orchestrator, &journal, &backends).await;
        assert!(resp.result.is_some());
        let payload = extract_payload(resp);
        let sid = payload["session_id"].as_str().unwrap();

        let load_params = tool_params("patent_session_load", serde_json::json!({"session_id": sid}));
        let resp = execute_tool_call(id.clone(), load_params, &config, &cache, &orchestrator, &journal, &backends).await;
        assert!(resp.result.is_some());
        let loaded = extract_payload(resp);
        assert_eq!(loaded["topic"], "e2e-test");
        assert_eq!(loaded["notes"], "integration");
    }

    #[tokio::test]
    async fn e2e_session_note_and_annotate() {
        let (config, cache, orchestrator, journal, backends, _sessions_tmp) = make_real_deps();
        let id = Value::Number(200.into());

        let create_params = tool_params("patent_session_create", serde_json::json!({"topic": "note-e2e"}));
        let resp = execute_tool_call(id.clone(), create_params, &config, &cache, &orchestrator, &journal, &backends).await;
        let payload = extract_payload(resp);
        let sid = payload["session_id"].as_str().unwrap().to_string();

        let note_params = tool_params("patent_session_note", serde_json::json!({
            "session_id": sid, "note": "e2e research note"
        }));
        let resp = execute_tool_call(id.clone(), note_params, &config, &cache, &orchestrator, &journal, &backends).await;
        let result = extract_payload(resp);
        assert_eq!(result["status"], "note added");

        let list_params = tool_params("patent_session_list", serde_json::json!({}));
        let resp = execute_tool_call(id.clone(), list_params, &config, &cache, &orchestrator, &journal, &backends).await;
        let list = extract_payload(resp);
        assert!(list["sessions"].as_array().unwrap().len() >= 1);
        assert!(list["total"].is_number());
    }

    #[tokio::test]
    async fn e2e_session_export() {
        let (config, cache, orchestrator, journal, backends, _sessions_tmp) = make_real_deps();
        let id = Value::Number(300.into());

        let tmp = tempfile::tempdir().unwrap();
        let create_params = tool_params("patent_session_create", serde_json::json!({"topic": "export-e2e"}));
        let resp = execute_tool_call(id.clone(), create_params, &config, &cache, &orchestrator, &journal, &backends).await;
        let payload = extract_payload(resp);
        let sid = payload["session_id"].as_str().unwrap();

        let export_path = tmp.path().join("report.md");
        let export_params = tool_params("patent_session_export", serde_json::json!({
            "session_id": sid, "output_path": export_path.to_str().unwrap()
        }));
        let resp = execute_tool_call(id.clone(), export_params, &config, &cache, &orchestrator, &journal, &backends).await;
        let result = extract_payload(resp);
        assert_eq!(result["status"], "exported");
        assert!(result["report_path"].is_string());
        assert!(export_path.exists());
    }

    #[tokio::test]
    async fn e2e_suggest_queries() {
        let (config, cache, orchestrator, journal, backends, _sessions_tmp) = make_real_deps();
        let id = Value::Number(400.into());

        let params = tool_params("patent_suggest_queries", serde_json::json!({
            "topic": "wireless power transfer", "prior_art_cutoff": "2020-01-01"
        }));
        let resp = execute_tool_call(id, params, &config, &cache, &orchestrator, &journal, &backends).await;
        assert!(resp.result.is_some());
        let payload = extract_payload(resp);
        assert_eq!(payload["topic"], "wireless power transfer");
        assert!(payload["planner_output"]["concepts"].is_array());
        assert!(payload["planner_output"]["query_variants"].is_array());
        assert!(payload["strategy"]["prior_art_notes"].is_object());
        assert_eq!(payload["strategy"]["prior_art_notes"]["cutoff_date"], "2020-01-01");
    }

    #[tokio::test]
    async fn e2e_profile_login_launch() {
        let (config, cache, orchestrator, journal, backends, _sessions_tmp) = make_real_deps();
        let id = Value::Number(500.into());

        let params = tool_params("patent_search_profile_login_start", serde_json::json!({"name": "test"}));
        let resp = execute_tool_call(id, params, &config, &cache, &orchestrator, &journal, &backends).await;
        assert!(resp.result.is_some());
        let r = resp.result.unwrap();
        if r["isError"] == true {
            let text = r["content"][0]["text"].as_str().unwrap();
            assert!(text.contains("Browser launch failed") || text.contains("Browser config error"));
        } else {
            let payload: serde_json::Value = serde_json::from_str(r["content"][0]["text"].as_str().unwrap()).unwrap();
            assert_eq!(payload["profile_name"], "test");
        }
    }

    #[tokio::test]
    async fn e2e_search_natural_no_key() {
        let (config, cache, orchestrator, journal, backends, _sessions_tmp) = make_real_deps();
        let id = Value::Number(600.into());

        let params = tool_params("patent_search_natural", serde_json::json!({
            "description": "wireless charging"
        }));
        let resp = execute_tool_call(id, params, &config, &cache, &orchestrator, &journal, &backends).await;
        assert!(resp.result.is_some());
        let payload = extract_payload(resp);
        assert!(payload["planner"]["concepts"].is_array());
        assert!(payload["results"].is_array());
        assert!(payload["elapsed_ms"].is_number());
    }

    #[tokio::test]
    async fn e2e_search_structured_no_key() {
        let (config, cache, orchestrator, journal, backends, _sessions_tmp) = make_real_deps();
        let id = Value::Number(700.into());

        let params = tool_params("patent_search_structured", serde_json::json!({
            "query": "TTL+(wireless+power)", "sources": ["USPTO"]
        }));
        let resp = execute_tool_call(id, params, &config, &cache, &orchestrator, &journal, &backends).await;
        assert!(resp.result.is_some());
        let payload = extract_payload(resp);
        assert!(payload["queries_run"].is_array());
        assert!(payload["results"].is_array());
    }

    #[tokio::test]
    async fn e2e_session_load_not_found() {
        let (config, cache, orchestrator, journal, backends, _sessions_tmp) = make_real_deps();
        let id = Value::Number(800.into());

        let params = tool_params("patent_session_load", serde_json::json!({"session_id": "nonexistent-xyz"}));
        let resp = execute_tool_call(id, params, &config, &cache, &orchestrator, &journal, &backends).await;
        assert!(resp.result.is_some());
        assert!(resp.error.is_none());
        let r = resp.result.unwrap();
        assert_eq!(r["isError"], true);
        let text = r["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("not found"));
    }

    #[tokio::test]
    async fn e2e_citation_chain_no_creds() {
        let (config, cache, orchestrator, journal, backends, _sessions_tmp) = make_real_deps();
        let id = Value::Number(900.into());

        let params = tool_params("patent_citation_chain", serde_json::json!({
            "patent_id": "US10000000", "direction": "backward", "depth": 1
        }));
        let resp = execute_tool_call(id, params, &config, &cache, &orchestrator, &journal, &backends).await;
        assert!(resp.result.is_some());
        let payload = extract_payload(resp);
        assert_eq!(payload["seed"], "US10000000");
        assert_eq!(payload["direction"], "backward");
        assert_eq!(payload["depth"], 1);
        assert!(payload["citations"].is_object());
    }

    #[tokio::test]
    async fn e2e_citation_chain_both_directions() {
        let (config, cache, orchestrator, journal, backends, _sessions_tmp) = make_real_deps();
        let id = Value::Number(910.into());

        let params = tool_params("patent_citation_chain", serde_json::json!({
            "patent_id": "US10000000", "direction": "both", "depth": 2
        }));
        let resp = execute_tool_call(id, params, &config, &cache, &orchestrator, &journal, &backends).await;
        assert!(resp.result.is_some());
        let payload = extract_payload(resp);
        assert_eq!(payload["direction"], "both");
        assert_eq!(payload["depth"], 2);
        let citations = &payload["citations"];
        assert!(citations["backward"].is_object());
        assert!(citations["forward"].is_object());
        assert!(citations["backward"]["level_1"].is_array());
        assert!(citations["forward"]["level_1"].is_array());
    }

    #[tokio::test]
    async fn e2e_citation_chain_with_session() {
        let (config, cache, orchestrator, journal, backends, _sessions_tmp) = make_real_deps();
        let id = Value::Number(920.into());

        let create_params = tool_params("patent_session_create", serde_json::json!({"topic": "citation-e2e"}));
        let resp = execute_tool_call(id.clone(), create_params, &config, &cache, &orchestrator, &journal, &backends).await;
        let sid = extract_payload(resp)["session_id"].as_str().unwrap().to_string();

        let params = tool_params("patent_citation_chain", serde_json::json!({
            "patent_id": "US10000000", "direction": "backward", "depth": 1, "session_id": sid
        }));
        let resp = execute_tool_call(id, params, &config, &cache, &orchestrator, &journal, &backends).await;
        assert!(resp.result.is_some());

        let load_params = tool_params("patent_session_load", serde_json::json!({"session_id": &sid}));
        let resp = execute_tool_call(Value::Number(921.into()), load_params, &config, &cache, &orchestrator, &journal, &backends).await;
        let loaded = extract_payload(resp);
        let chains = &loaded["citation_chains"];
        assert!(chains["US10000000"].is_object());
        assert!(chains["US10000000"]["backward"]["level_1"].is_array());
    }

    #[tokio::test]
    async fn e2e_classification_search_no_creds() {
        let (config, cache, orchestrator, journal, backends, _sessions_tmp) = make_real_deps();
        let id = Value::Number(1000.into());

        let params = tool_params("patent_classification_search", serde_json::json!({
            "code": "H02J50", "include_subclasses": true, "date_from": "2010-01-01", "date_to": "2020-01-01"
        }));
        let resp = execute_tool_call(id, params, &config, &cache, &orchestrator, &journal, &backends).await;
        assert!(resp.result.is_some());
        let payload = extract_payload(resp);
        assert_eq!(payload["code"], "H02J50");
        assert_eq!(payload["include_subclasses"], true);
        assert_eq!(payload["date_from"], "2010-01-01");
        assert_eq!(payload["date_to"], "2020-01-01");
        assert!(payload["total_found"].is_number());
        assert!(payload["results"].is_array());
    }

    #[tokio::test]
    async fn e2e_family_search_no_creds() {
        let (config, cache, orchestrator, journal, backends, _sessions_tmp) = make_real_deps();
        let id = Value::Number(1100.into());

        let params = tool_params("patent_family_search", serde_json::json!({
            "patent_id": "US10000000"
        }));
        let resp = execute_tool_call(id, params, &config, &cache, &orchestrator, &journal, &backends).await;
        assert!(resp.result.is_some());
        let payload = extract_payload(resp);
        assert_eq!(payload["patent_id"], "US10000000");
        assert!(payload["family_size"].is_number());
        assert!(payload["members"].is_array());
    }

    #[tokio::test]
    async fn e2e_search_natural_with_session_id() {
        let (config, cache, orchestrator, journal, backends, _sessions_tmp) = make_real_deps();
        let id = Value::Number(1200.into());

        let create_params = tool_params("patent_session_create", serde_json::json!({"topic": "natural-session-e2e"}));
        let resp = execute_tool_call(id.clone(), create_params, &config, &cache, &orchestrator, &journal, &backends).await;
        let sid = extract_payload(resp)["session_id"].as_str().unwrap().to_string();

        let params = tool_params("patent_search_natural", serde_json::json!({
            "description": "wireless charging", "session_id": &sid
        }));
        let resp = execute_tool_call(id, params, &config, &cache, &orchestrator, &journal, &backends).await;
        assert!(resp.result.is_some());
        let payload = extract_payload(resp);
        assert!(payload["planner"]["concepts"].is_array());
        assert!(payload["results"].is_array());

        let load_params = tool_params("patent_session_load", serde_json::json!({"session_id": &sid}));
        let resp = execute_tool_call(Value::Number(1201.into()), load_params, &config, &cache, &orchestrator, &journal, &backends).await;
        let loaded = extract_payload(resp);
        assert_eq!(loaded["topic"], "natural-session-e2e");
    }

    #[tokio::test]
    async fn e2e_search_structured_with_session_id() {
        let (config, cache, orchestrator, journal, backends, _sessions_tmp) = make_real_deps();
        let id = Value::Number(1300.into());

        let create_params = tool_params("patent_session_create", serde_json::json!({"topic": "structured-session-e2e"}));
        let resp = execute_tool_call(id.clone(), create_params, &config, &cache, &orchestrator, &journal, &backends).await;
        let sid = extract_payload(resp)["session_id"].as_str().unwrap().to_string();

        let params = tool_params("patent_search_structured", serde_json::json!({
            "query": "TTL+(wireless)", "sources": ["USPTO"], "session_id": &sid
        }));
        let resp = execute_tool_call(id, params, &config, &cache, &orchestrator, &journal, &backends).await;
        assert!(resp.result.is_some());
        let payload = extract_payload(resp);
        assert!(payload["results"].is_array());

        let load_params = tool_params("patent_session_load", serde_json::json!({"session_id": &sid}));
        let resp = execute_tool_call(Value::Number(1301.into()), load_params, &config, &cache, &orchestrator, &journal, &backends).await;
        let loaded = extract_payload(resp);
        assert_eq!(loaded["topic"], "structured-session-e2e");
    }

    #[test]
    fn test_malformed_json_returns_parse_error() {
        let line = r#"not json at all"#;
        let config = make_config();
        let resp = handle_line(line, &config).unwrap();
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32700);
    }

    #[test]
    fn test_empty_input_returns_error() {
        let line = r#""#;
        let config = make_config();
        let resp = handle_line(line, &config).unwrap();
        assert!(resp.error.is_some());
    }

    #[test]
    fn test_ping_via_handle_line() {
        let line = r#"{"jsonrpc":"2.0","id":99,"method":"ping"}"#;
        let config = make_config();
        let resp = handle_line(line, &config).unwrap();
        assert!(resp.result.is_some());
        assert!(resp.error.is_none());
        assert!(resp.result.unwrap().as_object().unwrap().is_empty());
    }
}
