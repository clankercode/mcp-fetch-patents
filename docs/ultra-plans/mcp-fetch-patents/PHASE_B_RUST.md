# PHASE B — Rust Production Implementation

*Prerequisite: Phase A (Python) complete and all Python tests passing.*
*Rust mirrors Python behavior exactly. Test harness validates parity.*

---

## Architecture Decisions for Rust

### Async Runtime
- **Tokio** with `#[tokio::main]` macro
- Blocking operations (SQLite writes, subprocess spawns, PDF tool subprocesses) wrapped in `tokio::task::spawn_blocking()`
- HTTP: `reqwest` async client (built on tokio)

### Error Handling
- `anyhow::Result<T>` for all functions that can fail (not `unwrap()` except in tests)
- `thiserror` for defining structured error types per module
- No panics in production code; only `expect()` on truly impossible conditions

### MCP Protocol (stdin/stdout)
- **No external MCP crate assumed** — implement bare JSON-RPC 2.0 over stdin/stdout
- Protocol is simple: read newline-delimited JSON messages from stdin; write to stdout
- Key messages to handle: `initialize`, `initialized`, `tools/list`, `tools/call`
- If a community crate (`rmcp`, `mcp-rs`) exists and is stable, use it; otherwise implement inline
- **Research task**: check crates.io for `mcp` or `rmcp` before implementing

### Configuration
- `dirs` crate for XDG paths (`dirs::data_dir()`)
- `toml` crate for TOML parsing (serde-based)
- All env vars read via `std::env::var()`

### Logging
- `tracing` + `tracing-subscriber` with `EnvFilter`
- All output to stderr: `tracing::info!`, `tracing::warn!`
- Stdout: ONLY JSON-RPC messages

---

## B01 — Scaffolding (from SCAFFOLDING.md S03-S04)
- Create `src/rust/` with Cargo.toml (see SCAFFOLDING.md S04)
- Create `src/rust/src/{main,lib,config,id_canon,cache,fetchers,converters,server}/mod.rs` files
- Verify: `cargo check` passes on empty stubs
- Verify: `cargo build --release` produces binary named `patent-mcp-server`

---

## B02 — Config (mirrors 06-config Python T01-T11)

### B02-T01 — PatentConfig struct + defaults
```rust
#[derive(Debug, Clone, serde::Deserialize)]
pub struct PatentConfig {
    pub cache_local_dir: PathBuf,
    pub cache_global_db: PathBuf,
    pub source_priority: Vec<String>,
    pub concurrency: usize,
    pub fetch_all_sources: bool,
    pub timeout_secs: f64,
    pub converters_order: Vec<String>,
    pub converters_disabled: Vec<String>,
    pub epo_client_id: Option<String>,
    pub epo_client_secret: Option<String>,
    pub lens_api_key: Option<String>,
    pub serpapi_key: Option<String>,
    pub bing_key: Option<String>,
    pub agent_command: String,
    pub log_level: String,
    pub source_base_urls: HashMap<String, String>,
}
```
- **RED**: `test_default_config` — `PatentConfig::default()` has same values as Python defaults
- **GREEN**: implement `Default` trait

### B02-T02 — Load from env + TOML
- **RED**: `test_env_overrides_defaults` — set env vars; verify loaded config
- **RED**: `test_toml_loaded` — parse temp TOML file; verify fields
- **RED**: `test_xdg_path` — `default_global_db()` uses `dirs::data_dir()`
- **GREEN**: implement `load_config()` with same precedence as Python

### B02-T03 — Parity with Python config
- **Cross-impl test** (from Phase C): serialize both configs to JSON; compare

---

## B03 — Patent ID Canonicalization (mirrors 01-id-canon Python T01-T17)

### B03-T01 — Regex patterns
```rust
// src/id_canon/mod.rs
use regex::Regex;
use once_cell::sync::Lazy;

static US_PATENT_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"...").unwrap());
// One Lazy<Regex> per jurisdiction pattern
```
- All 20+ regex patterns compiled once at startup via `once_cell::Lazy`
- **RED → GREEN**: mirror all T01-T15 from Python (same test cases, different syntax)

### B03-T02 — CanonicalPatentId struct
```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CanonicalPatentId {
    pub raw: String,
    pub canonical: String,
    pub jurisdiction: String,
    pub number: String,
    pub kind_code: Option<String>,
    pub doc_type: String,
    pub filing_year: Option<i32>,
    pub errors: Vec<String>,
}
```
- Derives `Serialize`/`Deserialize` so JSON output matches Python's dataclass JSON

### B03-T03 — Round-trip + fuzzing
- **RED**: `test_roundtrip` (same as Python T15)
- `cargo fuzz` target (Phase C T21)

---

## B04 — Cache + Global DB (mirrors 02-cache-db Python T01-T14)

### B04-T01 — SQLite setup with rusqlite
```rust
use rusqlite::{Connection, params};

pub struct PatentCache {
    db: Connection,
    local_dir: PathBuf,
}

impl PatentCache {
    pub fn new(config: &PatentConfig) -> anyhow::Result<Self> {
        let conn = Connection::open(&config.cache_global_db)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
        Self::run_migrations(&conn)?;
        Ok(Self { db: conn, local_dir: config.cache_local_dir.clone() })
    }
}
```
- **RED**: `test_schema_created` — same tables as Python
- **GREEN**: `run_migrations()` with same SQL as Python

### B04-T02 — store() + lookup()
- **RED**: mirror Python T03-T05
- **GREEN**: use `rusqlite` parameterized queries; serde_json for JSON files

### B04-T03 — SessionCache (in-memory)
```rust
use std::collections::HashMap;
use chrono::{DateTime, Utc};

pub struct SessionCache {
    tokens: HashMap<String, (String, DateTime<Utc>)>,  // source -> (token, expires_at)
}
```
- **RED**: mirror Python T08
- Note: no `Mutex` needed if `SessionCache` is owned by a single tokio task; use `Arc<Mutex<SessionCache>>` if shared across tasks

### B04-T04 — Concurrent writes (tokio)
- `spawn_blocking` wraps all SQLite operations (SQLite is not async-native)
- **RED**: `test_concurrent_stores` using `tokio::spawn` + `join!`

---

## B05 — Format Conversion (mirrors 04-format-conversion Python T01-T15)

### B05-T01 — Tool availability check
```rust
fn check_tool_available(name: &str) -> bool {
    std::process::Command::new(name).arg("--version")
        .stdout(Stdio::null()).stderr(Stdio::null())
        .status().map(|s| s.success()).unwrap_or(false)
}
```

### B05-T02 — Subprocess converters
All converters called via `std::process::Command`:
```rust
async fn run_pdftotext(pdf: &Path, output: &Path) -> anyhow::Result<()> {
    let result = tokio::process::Command::new("pdftotext")
        .args(["-layout", pdf.to_str().unwrap(), output.to_str().unwrap()])
        .output().await?;
    if !result.status.success() {
        anyhow::bail!("pdftotext failed: {}", String::from_utf8_lossy(&result.stderr));
    }
    Ok(())
}
```
- pymupdf4llm: spawns `python -m patent_mcp.converters.pymupdf_runner {pdf} {output}`
- tesseract: `tesseract {image} stdout -l eng`
- marker: `python -m patent_mcp.converters.marker_runner {pdf} {output}` (when enabled)

### B05-T03 — Markdown assembly
```rust
pub fn assemble_markdown(base_md: &str, metadata: &PatentMetadata, images: &[ImageResult]) -> String {
    // Same template as Python assemble_markdown()
}
```
- **RED**: `test_assemble_markdown_structure` — same structure as Python output

---

## B06 — HTTP Sources (mirrors 03a Python T01-T19)

### B06-T01 — PatentSource trait
```rust
use async_trait::async_trait;

#[async_trait]
pub trait PatentSource: Send + Sync {
    fn source_name(&self) -> &str;
    fn supported_jurisdictions(&self) -> &[&str];  // ["US"] or ["*"] for all
    async fn fetch(&self, id: &CanonicalPatentId, output_dir: &Path, config: &PatentConfig) -> SourceAttempt;
}
```

### B06-T02 — PpubsSource with HTTP/2
```rust
pub struct PpubsSource {
    client: reqwest::Client,   // built with .http2_prior_knowledge()
    session_cache: Arc<Mutex<SessionCache>>,
}
```
- **Note**: `reqwest` supports HTTP/2 via `http2` feature flag + `.http2_prior_knowledge()`
- Session establishment: POST to PPUBS session endpoint; store in `SessionCache`

### B06-T03 — EpoOpsSource with OAuth2
- OAuth2 token management: POST to EPO auth endpoint with Basic auth (client_id:client_secret)
- Token cache: `session_cache.set_with_expiry()` using `expires_in` from OAuth response

### B06-T04 — Retry logic
```rust
// Manual retry loop (no external crate needed for 3 retries)
async fn fetch_with_retry<F, Fut, T>(f: F, max_attempts: u32) -> anyhow::Result<T>
where F: Fn() -> Fut, Fut: Future<Output = anyhow::Result<T>>
{
    for attempt in 0..max_attempts {
        match f().await {
            Ok(v) => return Ok(v),
            Err(e) if attempt < max_attempts - 1 => {
                tokio::time::sleep(Duration::from_secs(2_u64.pow(attempt))).await;
            }
            Err(e) => return Err(e),
        }
    }
    unreachable!()
}
```

### B06-T05 — BigQuery source
- BigQuery has no official Rust client for querying
- Use the BigQuery REST API directly: `POST https://bigquery.googleapis.com/bigquery/v2/projects/{project}/queries`
- Auth: `google-auth` equivalent → use `reqwest` with OAuth2 bearer token from service account JSON (manual JWT signing via `jsonwebtoken` crate)
- **Note**: This is the most complex part of Rust Phase B; consider calling Python subprocess for BigQuery queries if JWT complexity is excessive
- **Decision point**: If BigQuery REST API JWT signing adds >1 sprint, call Python `patent_mcp.fetchers.bigquery_runner {id}` as subprocess (same pattern as Playwright)

---

## B07 — Browser Sources (mirrors 03b Python T01-T09)

### B07-T01 — Rust subprocess bridge
```rust
pub struct BrowserSource {
    python_cmd: String,
}

#[async_trait]
impl PatentSource for BrowserSource {
    async fn fetch(&self, id: &CanonicalPatentId, output_dir: &Path, config: &PatentConfig) -> SourceAttempt {
        let request = serde_json::to_string(&PlaywrightRequest { ... }).unwrap();
        let result = tokio::process::Command::new(&self.python_cmd)
            .args(["-m", "patent_mcp.scrapers.playwright_runner"])
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
            .spawn();
        // Write request JSON to stdin, read SourceAttempt JSON from stdout
    }
}
```
- **RED**: `test_browser_source_calls_subprocess` — spawn mock Python script that echoes canned JSON
- **GREEN**: implement subprocess communication

---

## B08 — Web Search Fallback (mirrors 03c Python T01-T07)
- Small module; straightforward `reqwest` HTTP calls
- Query generation + confidence scoring: pure string functions, no Rust-specific complexity

---

## B09 — Fetcher Orchestrator (mirrors 03 Python T01-T13)

### B09-T01 — Tokio concurrent batch
```rust
pub async fn fetch_batch(&self, ids: &[CanonicalPatentId], base_cache: &Path) -> Vec<OrchestratorResult> {
    let semaphore = Arc::new(Semaphore::new(self.config.concurrency));
    let futures: Vec<_> = ids.iter().map(|id| {
        let sem = semaphore.clone();
        async move {
            let _permit = sem.acquire().await.unwrap();
            self.fetch(id, &base_cache.join(id.canonical.clone())).await
        }
    }).collect();
    futures::future::join_all(futures).await
}
```

---

## B10 — MCP Protocol Server (mirrors 05-mcp-protocol Python T01-T18)

### B10-T01 — Research MCP crate availability
- **Task**: `cargo search mcp` / check crates.io for `rmcp`, `mcp-server-sdk`, `mcp-rs`
- If a stable crate exists (>v0.5, recent activity), use it
- If not, implement minimal JSON-RPC 2.0 over stdin

### B10-T02 — Minimal JSON-RPC stdin loop
```rust
async fn run_mcp_server(state: Arc<AppState>) {
    let stdin = tokio::io::stdin();
    let mut reader = tokio::io::BufReader::new(stdin);
    let stdout = tokio::io::stdout();
    let mut writer = tokio::io::BufWriter::new(stdout);
    
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).await? == 0 { break; }  // EOF
        let request: serde_json::Value = serde_json::from_str(&line)?;
        let response = handle_message(&state, request).await;
        writer.write_all((serde_json::to_string(&response)? + "\n").as_bytes()).await?;
        writer.flush().await?;
    }
}
```

### B10-T03 — Tool handlers
- `handle_fetch_patents()`: call Rust orchestrator, return JSON
- `handle_list_cached_patents()`: query SQLite, return JSON
- `handle_get_patent_metadata()`: query SQLite, return JSON

---

## Phase C: Cross-Validation (after Phase A + B)

### C01 — Run full parity suite (07-test-infra T16-T19)
- Fix any divergences found

### C02 — Fuzzing (07-test-infra T20-T21)
- `hypothesis` on Python canonicalize
- `cargo fuzz` on Rust canonicalize

### C03 — Performance benchmark
- Measure: Python batch(100 patents, all cache hits) vs Rust batch(same)
- Expected: Rust should be 10-100x faster for cache-only workloads
- Document in `docs/benchmarks.md`

---

## Rust-Specific Risks

| Risk | Mitigation |
|---|---|
| BigQuery REST JWT signing complexity | Call Python subprocess as fallback (same as Playwright) |
| MCP crate unavailable / unstable | Implement minimal JSON-RPC inline (< 100 lines) |
| rusqlite thread safety with tokio | Use `spawn_blocking` for all DB ops |
| HTTP/2 with reqwest not working for PPUBS | Fall back to HTTP/1.1; PPUBS works either way |
| Regex crate behavior differs from Python re | Test all regex patterns explicitly in parity suite |
