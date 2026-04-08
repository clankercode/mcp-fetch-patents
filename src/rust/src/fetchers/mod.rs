//! Source fetcher orchestrator — mirrors Python patent_mcp.fetchers.orchestrator module.
//!
//! On cache miss the orchestrator delegates to the Python implementation via subprocess
//! (`python3 -m patent_mcp fetch-one {id} --cache-dir {dir}`), then stores the result
//! in the Rust cache so subsequent calls are served from Rust directly.

pub mod http;
pub mod web_search;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::cache::{ArtifactSet, PatentCache, PatentMetadata, SourceAttempt};
use crate::config::PatentConfig;
use crate::id_canon::CanonicalPatentId;

/// Result of fetching a single patent from one source.
#[derive(Debug, Clone)]
pub struct FetchResult {
    pub source_attempt: SourceAttempt,
    pub pdf_path: Option<PathBuf>,
    pub txt_path: Option<PathBuf>,
    pub metadata: Option<PatentMetadata>,
}

/// Result of the full orchestration for one patent.
#[derive(Debug, Clone)]
pub struct OrchestratorResult {
    pub canonical_id: String,
    pub success: bool,
    pub cache_dir: Option<PathBuf>,
    pub files: HashMap<String, PathBuf>,
    pub metadata: Option<PatentMetadata>,
    pub sources: Vec<SourceAttempt>,
    pub error: Option<String>,
    pub from_cache: bool,
}

/// Coordinates patent fetching across all sources with caching.
pub struct FetcherOrchestrator {
    config: PatentConfig,
    cache: PatentCache,
}

impl FetcherOrchestrator {
    pub fn new(config: PatentConfig, cache: PatentCache) -> Self {
        FetcherOrchestrator { config, cache }
    }

    /// Fetch a single patent, using cache if available.
    /// On miss: delegates to Python implementation via subprocess.
    pub async fn fetch(
        &self,
        patent: &CanonicalPatentId,
        _output_dir: &Path,
    ) -> OrchestratorResult {
        // Cache hit
        if let Ok(Some(cached)) = self.cache.lookup(&patent.canonical) {
            if cached.is_complete {
                return OrchestratorResult {
                    canonical_id: patent.canonical.clone(),
                    success: true,
                    cache_dir: Some(cached.cache_dir),
                    files: cached.files,
                    metadata: cached.metadata,
                    sources: vec![],
                    error: None,
                    from_cache: true,
                };
            }
        }

        // Delegate to Python on cache miss
        let cache_dir_str = self.config.cache_local_dir.to_string_lossy().into_owned();
        let canon_id = patent.canonical.clone();

        let result = tokio::task::spawn_blocking(move || {
            fetch_via_python(&canon_id, &cache_dir_str)
        }).await;

        match result {
            Ok(Ok(py_result)) => {
                // Store in Rust cache if successful
                if py_result.success {
                    if let Some(ref meta) = py_result.metadata {
                        let artifacts = ArtifactSet {
                            pdf: py_result.files.get("pdf").cloned(),
                            txt: py_result.files.get("txt").cloned(),
                            md: py_result.files.get("md").cloned(),
                            images: vec![],
                        };
                        let _ = self.cache.store(&patent.canonical, &artifacts, meta, None);
                    }
                }
                py_result
            }
            Ok(Err(e)) => OrchestratorResult {
                canonical_id: patent.canonical.clone(),
                success: false,
                cache_dir: None,
                files: HashMap::new(),
                metadata: None,
                sources: vec![],
                error: Some(format!("Python delegate error: {}", e)),
                from_cache: false,
            },
            Err(e) => OrchestratorResult {
                canonical_id: patent.canonical.clone(),
                success: false,
                cache_dir: None,
                files: HashMap::new(),
                metadata: None,
                sources: vec![],
                error: Some(format!("Spawn error: {}", e)),
                from_cache: false,
            },
        }
    }

    /// Fetch multiple patents concurrently, respecting `config.concurrency`.
    pub async fn fetch_batch(
        &self,
        patents: &[CanonicalPatentId],
        output_base: &Path,
    ) -> Vec<OrchestratorResult> {
        use futures::stream::{FuturesOrdered, StreamExt};
        let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(self.config.concurrency));
        let mut ordered = FuturesOrdered::new();

        for patent in patents {
            let permit = sem.clone().acquire_owned().await.expect("semaphore closed");
            // _output_dir is unused in fetch(); pass output_base directly
            let fut = self.fetch(patent, output_base);
            ordered.push_back(async move {
                let res = fut.await;
                drop(permit);
                res
            });
        }

        ordered.collect().await
    }
}

// ---------------------------------------------------------------------------
// Python subprocess bridge
// ---------------------------------------------------------------------------

/// JSON shape returned by `python3 -m patent_mcp fetch-one`.
#[derive(Debug, serde::Deserialize)]
struct PyFetchResult {
    canonical_id: String,
    success: bool,
    from_cache: bool,
    files: HashMap<String, String>,
    metadata: Option<serde_json::Value>,
    error: Option<String>,
}

fn fetch_via_python(
    canonical_id: &str,
    cache_dir: &str,
) -> anyhow::Result<OrchestratorResult> {
    let output = std::process::Command::new("python3")
        .args(["-m", "patent_mcp", "fetch-one", canonical_id, "--cache-dir", cache_dir])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Ok(OrchestratorResult {
            canonical_id: canonical_id.to_string(),
            success: false,
            cache_dir: None,
            files: HashMap::new(),
            metadata: None,
            sources: vec![],
            error: Some(format!("Python fetch-one failed: {}", stderr.trim())),
            from_cache: false,
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Find the last line that looks like JSON (Python may emit log lines before)
    let json_line = stdout
        .lines()
        .filter(|l| l.trim_start().starts_with('{'))
        .last()
        .unwrap_or("");

    let py: PyFetchResult = serde_json::from_str(json_line).map_err(|e| {
        anyhow::anyhow!("Failed to parse Python output: {} — output: {}", e, json_line)
    })?;

    let files: HashMap<String, PathBuf> = py
        .files
        .into_iter()
        .map(|(k, v)| (k, PathBuf::from(v)))
        .collect();

    // Parse metadata if present
    let metadata = py.metadata.as_ref().and_then(|v| {
        serde_json::from_value::<PatentMetadata>(v.clone()).ok()
    });

    Ok(OrchestratorResult {
        canonical_id: py.canonical_id,
        success: py.success,
        cache_dir: None,
        files,
        metadata,
        sources: vec![],
        error: py.error,
        from_cache: py.from_cache,
    })
}
