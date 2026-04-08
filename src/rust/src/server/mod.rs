//! MCP protocol server — mirrors Python patent_mcp.server module.
//!
//! Implements JSON-RPC 2.0 over stdin/stdout (MCP transport).

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Write;
use std::collections::HashMap;

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

// ---------------------------------------------------------------------------
// MCP tool descriptors
// ---------------------------------------------------------------------------

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
                        }
                    },
                    "required": ["patent_ids"]
                }
            },
            {
                "name": "list_cached_patents",
                "description": "List all patents cached in the local .patents/ directory.",
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
            }
        ]
    })
}

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

    let mut plan = Vec::with_capacity(patent_ids.len());
    let mut valid_patents = Vec::new();

    for raw_id in patent_ids {
        let canon = crate::id_canon::canonicalize(raw_id);
        if canon.jurisdiction == "UNKNOWN" {
            plan.push(FetchPlan::Invalid(crate::fetchers::OrchestratorResult {
                canonical_id: canon.canonical,
                success: false,
                cache_dir: None,
                files: HashMap::new(),
                metadata: None,
                sources: vec![],
                error: Some(format!("Invalid patent ID: {}", raw_id)),
                from_cache: false,
            }));
        } else {
            valid_patents.push(canon.clone());
            plan.push(FetchPlan::Valid);
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

    for item in plan {
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

        if orc.from_cache {
            n_cached += 1;
            n_success += 1;
        } else if orc.success {
            n_success += 1;
        } else {
            n_errors += 1;
        }

        results.push(serde_json::json!({
            "canonical_id": orc.canonical_id,
            "success": orc.success,
            "from_cache": orc.from_cache,
            "files": files,
            "metadata": metadata,
            "error": orc.error,
        }));
    }

    let total = results.len() as u32;
    serde_json::json!({
        "results": results,
        "summary": {
            "total": total,
            "success": n_success,
            "cached": n_cached,
            "errors": n_errors,
            "total_duration_ms": 0.0
        }
    })
}

// ---------------------------------------------------------------------------
// Sync routing (parse + non-tool dispatch)
// ---------------------------------------------------------------------------

enum Dispatch {
    /// Respond immediately with this response
    Immediate(RpcResponse),
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
        "initialized" => Dispatch::Immediate(RpcResponse::ok(id, Value::Null)),
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

async fn execute_tool_call(
    id: Value,
    params: Value,
    config: &PatentConfig,
    cache: &crate::cache::PatentCache,
    orchestrator: &crate::fetchers::FetcherOrchestrator,
) -> RpcResponse {
    let tool_name = match params.get("name").and_then(|v| v.as_str()) {
        Some(n) => n.to_string(),
        None => return RpcResponse::err(id, -32602, "Missing tool name"),
    };

    match tool_name.as_str() {
        "fetch_patents" => {
            let patent_ids: Vec<String> = params
                .get("arguments")
                .and_then(|a| a.get("patent_ids"))
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();

            let force_refresh = params
                .get("arguments")
                .and_then(|a| a.get("force_refresh"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let payload = build_fetch_patents_payload(
                &patent_ids,
                force_refresh,
                config,
                orchestrator,
            )
            .await;

            RpcResponse::ok(id, serde_json::json!({
                "content": [{"type": "text", "text": payload.to_string()}]
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
                    let payload = serde_json::json!({"patents": patents, "count": count});
                    RpcResponse::ok(id, serde_json::json!({
                        "content": [{"type": "text", "text": payload.to_string()}]
                    }))
                }
                Err(e) => RpcResponse::err(id, -32603, &format!("Cache error: {}", e)),
            }
        }

        "get_patent_metadata" => {
            let patent_ids: Vec<String> = params
                .get("arguments")
                .and_then(|a| a.get("patent_ids"))
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();

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

            let payload = serde_json::json!({"results": results});
            RpcResponse::ok(id, serde_json::json!({
                "content": [{"type": "text", "text": payload.to_string()}]
            }))
        }

        _ => RpcResponse::err(id, -32601, &format!("Unknown tool: {}", tool_name)),
    }
}

// ---------------------------------------------------------------------------
// Server loop
// ---------------------------------------------------------------------------

/// Run the MCP server on stdin/stdout until EOF.
pub async fn run_server(config: PatentConfig) -> Result<()> {
    use crate::cache::PatentCache;
    use crate::fetchers::FetcherOrchestrator;

    // Two PatentCache instances sharing the same SQLite WAL database:
    // one owned by the orchestrator for fetch operations,
    // one retained here for list_all / lookup operations.
    let cache_for_ops = PatentCache::new(&config)?;
    let cache_for_orch = PatentCache::new(&config)?;
    let orchestrator = FetcherOrchestrator::new(config.clone(), cache_for_orch);

    let stdout = std::io::stdout();

    tracing::info!("patent-mcp-server started (Rust implementation)");

    // Read lines from stdin one at a time using a blocking reader on a separate thread,
    // sending them through a channel so the async runtime can process them.
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    std::thread::spawn(move || {
        use std::io::BufRead;
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            match line {
                Ok(l) => { let _ = tx.send(l); }
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
            Dispatch::ToolCall { id, params } => {
                execute_tool_call(id, params, &config, &cache_for_ops, &orchestrator).await
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
fn handle_line(line: &str, _config: &PatentConfig) -> RpcResponse {
    match route_line(line) {
        Dispatch::Immediate(r) => r,
        Dispatch::ToolCall { id, params } => {
            // For sync test use: handle the empty fetch_patents case inline.
            let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
            match tool_name.as_str() {
                "fetch_patents" => {
                    let patent_ids: Vec<String> = params
                        .get("arguments")
                        .and_then(|a| a.get("patent_ids"))
                        .and_then(|v| v.as_array())
                        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                        .unwrap_or_default();

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
                        }).to_string()}]
                    }))
                }
                "list_cached_patents" => RpcResponse::ok(id, serde_json::json!({
                    "content": [{"type": "text", "text": serde_json::json!({"patents": [], "count": 0}).to_string()}]
                })),
                "get_patent_metadata" => RpcResponse::ok(id, serde_json::json!({
                    "content": [{"type": "text", "text": serde_json::json!({"results": []}).to_string()}]
                })),
                _ => RpcResponse::err(id, -32601, &format!("Unknown tool: {}", tool_name)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config() -> PatentConfig {
        PatentConfig {
            cache_local_dir: std::path::PathBuf::from(".patents"),
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
        }
    }

    #[test]
    fn test_handle_initialize() {
        let line = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
        let config = make_config();
        let resp = handle_line(line, &config);
        assert!(resp.result.is_some());
        let r = resp.result.unwrap();
        assert_eq!(r["protocolVersion"], "2024-11-05");
    }

    #[test]
    fn test_handle_tools_list() {
        let line = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#;
        let config = make_config();
        let resp = handle_line(line, &config);
        assert!(resp.result.is_some());
        let r = resp.result.unwrap();
        assert!(r["tools"].is_array());
        let tools = r["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter()
            .filter_map(|t| t["name"].as_str())
            .collect();
        assert!(names.contains(&"fetch_patents"));
        assert!(names.contains(&"list_cached_patents"));
        assert!(names.contains(&"get_patent_metadata"));
    }

    #[test]
    fn test_empty_fetch_patents() {
        let line = r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"fetch_patents","arguments":{"patent_ids":[]}}}"#;
        let config = make_config();
        let resp = handle_line(line, &config);
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
        assert_eq!(payload["results"][0]["success"], false);
        assert!(payload["results"][0]["error"].as_str().unwrap().contains("Invalid patent ID"));
        assert_eq!(payload["results"][1]["canonical_id"], "US7654321");
    }

    #[test]
    fn test_unknown_method_returns_error() {
        let line = r#"{"jsonrpc":"2.0","id":4,"method":"unknown/method","params":{}}"#;
        let config = make_config();
        let resp = handle_line(line, &config);
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32601);
    }
}
