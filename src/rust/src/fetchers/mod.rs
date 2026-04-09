//! Source fetcher orchestrator — coordinates all native Rust patent sources.
//!
//! Tries sources in config priority order per jurisdiction.
//! Falls back to web search if all structured sources fail.

pub mod browser;
pub mod http;
pub mod web_search;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use tracing::debug;

use crate::cache::{ArtifactSet, PatentCache, PatentMetadata, SessionCache, SourceAttempt};
use crate::config::PatentConfig;
use crate::converters::ConverterPipeline;
use crate::id_canon::CanonicalPatentId;

fn merge_metadata(existing: &mut Option<PatentMetadata>, incoming: Option<PatentMetadata>) {
    let Some(incoming) = incoming else {
        return;
    };

    match existing {
        None => *existing = Some(incoming),
        Some(current) => {
            if current.title.is_none() {
                current.title = incoming.title;
            }
            if current.abstract_text.is_none() {
                current.abstract_text = incoming.abstract_text;
            }
            if current.inventors.is_empty() && !incoming.inventors.is_empty() {
                current.inventors = incoming.inventors;
            }
            if current.assignee.is_none() {
                current.assignee = incoming.assignee;
            }
            if current.filing_date.is_none() {
                current.filing_date = incoming.filing_date;
            }
            if current.publication_date.is_none() {
                current.publication_date = incoming.publication_date;
            }
            if current.grant_date.is_none() {
                current.grant_date = incoming.grant_date;
            }
            if current.legal_status.is_none() {
                current.legal_status = incoming.legal_status;
            }
        }
    }
}

/// Result of fetching a single patent from one source.
#[derive(Debug, Clone)]
pub struct FetchResult {
    pub source_attempt: SourceAttempt,
    pub pdf_path: Option<PathBuf>,
    pub txt_path: Option<PathBuf>,
    pub metadata: Option<PatentMetadata>,
}

/// Trait implemented by all patent data sources.
///
/// Mirrors Python `patent_mcp.fetchers.base.BasePatentSource`.
#[async_trait]
pub trait PatentSource: Send + Sync {
    /// Unique identifier for this source (e.g. "USPTO", "EPO_OPS").
    fn source_name(&self) -> &str;

    /// Jurisdiction codes this source supports; empty slice = all jurisdictions.
    fn supported_jurisdictions(&self) -> &[&str];

    /// Returns true if this source can handle the given patent's jurisdiction.
    fn can_fetch(&self, patent: &CanonicalPatentId) -> bool {
        let jx = self.supported_jurisdictions();
        jx.is_empty() || jx.contains(&patent.jurisdiction.as_str())
    }

    /// Fetch patent data and write files to `output_dir`.
    async fn fetch(
        &self,
        patent: &CanonicalPatentId,
        output_dir: &Path,
        config: &PatentConfig,
    ) -> FetchResult;
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
    sources: Vec<Box<dyn PatentSource>>,
}

impl FetcherOrchestrator {
    pub fn new(config: PatentConfig, cache: PatentCache) -> Self {
        let session_cache = Arc::new(SessionCache::new());
        let sources = Self::build_sources(&config, session_cache);
        FetcherOrchestrator {
            config,
            cache,
            sources,
        }
    }

    /// Build all available sources in config priority order.
    /// Mirrors Python FetcherOrchestrator._build_sources().
    fn build_sources(
        config: &PatentConfig,
        session_cache: Arc<SessionCache>,
    ) -> Vec<Box<dyn PatentSource>> {
        use crate::fetchers::http::*;
        use crate::fetchers::browser::BrowserSource;

        // All available sources keyed by their config name
        let mut all_sources: Vec<(&str, Box<dyn PatentSource>)> = vec![
            (
                "USPTO",
                Box::new(PpubsSource {
                    session_cache: session_cache.clone(),
                }),
            ),
            (
                "EPO_OPS",
                Box::new(EpoOpsSource {
                    session_cache: session_cache.clone(),
                }),
            ),
            ("BigQuery", Box::new(BigQuerySource)),
            ("Espacenet", Box::new(EspacenetSource)),
            ("WIPO_Scrape", Box::new(WipoScrapeSource)),
            ("IP_Australia", Box::new(IpAustraliaSource)),
            ("CIPO", Box::new(CipoScrapeSource)),
            ("Google_Patents", Box::new(BrowserSource)),
            // Note: web_search is handled separately as a last resort
        ];

        // Order by config.source_priority
        let mut ordered: Vec<Box<dyn PatentSource>> = Vec::new();

        for name in &config.source_priority {
            if name == "web_search" {
                continue;
            } // handled separately
            if let Some(pos) = all_sources.iter().position(|(k, _)| *k == name.as_str()) {
                let (_, source) = all_sources.remove(pos);
                ordered.push(source);
            }
        }
        // Add remaining sources not in priority list
        for (_, source) in all_sources {
            ordered.push(source);
        }

        ordered
    }

    /// Get sources that support a patent's jurisdiction, in priority order.
    pub fn get_sources_for(&self, patent: &CanonicalPatentId) -> Vec<&dyn PatentSource> {
        self.sources
            .iter()
            .filter(|s| s.can_fetch(patent))
            .map(|s| s.as_ref())
            .collect()
    }

    /// Fetch a single patent, using cache if available.
    pub async fn fetch(
        &self,
        patent: &CanonicalPatentId,
        output_dir: &Path,
    ) -> OrchestratorResult {
        self.fetch_internal(patent, output_dir, false).await
    }

    pub async fn fetch_force_refresh(
        &self,
        patent: &CanonicalPatentId,
        output_dir: &Path,
    ) -> OrchestratorResult {
        self.fetch_internal(patent, output_dir, true).await
    }

    async fn fetch_internal(
        &self,
        patent: &CanonicalPatentId,
        output_dir: &Path,
        force_refresh: bool,
    ) -> OrchestratorResult {
        // Cache hit
        if !force_refresh {
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
        }

        // Create output dir
        let _ = std::fs::create_dir_all(output_dir);

        let sources = self.get_sources_for(patent);

        let mut all_attempts = Vec::new();
        let mut all_pdfs: Vec<PathBuf> = Vec::new();
        let mut all_txts: Vec<PathBuf> = Vec::new();
        let mut best_metadata: Option<PatentMetadata> = None;

        // Collect from all structured sources so later sources can fill metadata gaps.
        for source in &sources {
            let source_name = source.source_name().to_string();
            debug!("Trying source {} for {}", source_name, patent.canonical);
            let result = source.fetch(patent, output_dir, &self.config).await;
            all_attempts.push(result.source_attempt.clone());
            if result.source_attempt.success {
                if let Some(p) = result.pdf_path {
                    all_pdfs.push(p);
                }
                if let Some(p) = result.txt_path {
                    all_txts.push(p);
                }
                merge_metadata(&mut best_metadata, result.metadata);
            }
        }

        let any_success = all_attempts.iter().any(|a| a.success);

        // Web search fallback if no structured source succeeded
        if !any_success {
            debug!(
                "All structured sources failed for {}, trying web search fallback",
                patent.canonical
            );
            let ws_result =
                web_search::WebSearchFallbackSource::fetch(patent, output_dir, &self.config).await;
            all_attempts.push(ws_result.source_attempt.clone());
            if ws_result.source_attempt.success {
                if let Some(p) = ws_result.pdf_path {
                    all_pdfs.push(p);
                }
                if let Some(p) = ws_result.txt_path {
                    all_txts.push(p);
                }
                merge_metadata(&mut best_metadata, ws_result.metadata);
            }
        }

        // Convert PDF to markdown if we got a PDF
        let mut md_path: Option<PathBuf> = None;
        if let Some(pdf) = all_pdfs.first() {
            let pipeline = ConverterPipeline::new(
                self.config.converters_order.clone(),
                self.config.converters_disabled.clone(),
            );
            let md_out = output_dir.join(format!("{}.md", patent.canonical));
            if let Ok(cr) = pipeline.pdf_to_markdown(pdf, &md_out) {
                if cr.success {
                    md_path = cr.output_path;
                }
            }
        }

        // Build files dict
        let mut files: HashMap<String, PathBuf> = HashMap::new();
        if let Some(p) = all_pdfs.first() {
            files.insert("pdf".into(), p.clone());
        }
        if let Some(p) = all_txts.first() {
            files.insert("txt".into(), p.clone());
        }
        if let Some(p) = &md_path {
            files.insert("md".into(), p.clone());
        }

        // Store in cache
        let updated_any_success = all_attempts.iter().any(|a| a.success);
        if updated_any_success {
            if let Some(ref meta) = best_metadata {
                let artifacts = ArtifactSet {
                    pdf: all_pdfs.first().cloned(),
                    txt: all_txts.first().cloned(),
                    md: md_path.clone(),
                    images: vec![],
                };
                let _ = self
                    .cache
                    .store(&patent.canonical, &artifacts, meta, Some(&all_attempts));
            }
        }

        OrchestratorResult {
            canonical_id: patent.canonical.clone(),
            success: updated_any_success || !files.is_empty(),
            cache_dir: if !files.is_empty() {
                Some(output_dir.to_path_buf())
            } else {
                None
            },
            files,
            metadata: best_metadata,
            sources: all_attempts,
            error: None,
            from_cache: false,
        }
    }

    /// Fetch multiple patents concurrently, respecting `config.concurrency`.
    pub async fn fetch_batch(
        &self,
        patents: &[CanonicalPatentId],
        output_base: &Path,
    ) -> Vec<OrchestratorResult> {
        self.fetch_batch_internal(patents, output_base, false).await
    }

    pub async fn fetch_batch_force_refresh(
        &self,
        patents: &[CanonicalPatentId],
        output_base: &Path,
    ) -> Vec<OrchestratorResult> {
        self.fetch_batch_internal(patents, output_base, true).await
    }

    async fn fetch_batch_internal(
        &self,
        patents: &[CanonicalPatentId],
        output_base: &Path,
        force_refresh: bool,
    ) -> Vec<OrchestratorResult> {
        use futures::stream::{FuturesOrdered, StreamExt};
        let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(self.config.concurrency));
        let mut ordered = FuturesOrdered::new();

        for patent in patents {
            let sem = sem.clone();
            let out_dir = output_base.join(&patent.canonical);
            let patent_clone = patent.clone();
            ordered.push_back(async move {
                // Acquire permit inside the async block so FuturesOrdered can
                // poll futures concurrently instead of blocking the for loop.
                let permit = sem.acquire_owned().await.expect("semaphore closed");
                let res = self
                    .fetch_internal(&patent_clone, &out_dir, force_refresh)
                    .await;
                drop(permit);
                res
            });
        }

        ordered.collect().await
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    struct StubSource {
        name: &'static str,
        jurisdictions: &'static [&'static str],
        result: FetchResult,
    }

    #[async_trait]
    impl PatentSource for StubSource {
        fn source_name(&self) -> &str {
            self.name
        }

        fn supported_jurisdictions(&self) -> &[&str] {
            self.jurisdictions
        }

        async fn fetch(
            &self,
            _patent: &CanonicalPatentId,
            _output_dir: &Path,
            _config: &PatentConfig,
        ) -> FetchResult {
            self.result.clone()
        }
    }

    fn make_config(tmp: &TempDir) -> PatentConfig {
        PatentConfig {
            cache_local_dir: tmp.path().join("cache").join("patents"),
            cache_global_db: tmp.path().join("global").join("index.db"),
            source_priority: vec![
                "USPTO".into(),
                "EPO_OPS".into(),
                "BigQuery".into(),
                "Espacenet".into(),
                "WIPO_Scrape".into(),
                "IP_Australia".into(),
                "CIPO".into(),
                "web_search".into(),
            ],
            concurrency: 5,
            fetch_all_sources: false,
            timeout_secs: 30.0,
            converters_order: vec![],
            converters_disabled: vec![],
            source_base_urls: HashMap::new(),
            epo_client_id: None,
            epo_client_secret: None,
            lens_api_key: None,
            serpapi_key: None,
            bing_key: None,
            bigquery_project: None,
            activity_journal: None,
        }
    }

    #[test]
    fn test_orchestrator_builds_sources() {
        let tmp = TempDir::new().unwrap();
        let cfg = make_config(&tmp);
        let cache = PatentCache::new(&cfg).unwrap();
        let orch = FetcherOrchestrator::new(cfg, cache);
        // Should have built sources
        assert!(!orch.sources.is_empty());
    }

    #[test]
    fn test_get_sources_for_us_patent() {
        let tmp = TempDir::new().unwrap();
        let cfg = make_config(&tmp);
        let cache = PatentCache::new(&cfg).unwrap();
        let orch = FetcherOrchestrator::new(cfg, cache);
        let patent = crate::id_canon::canonicalize("US7654321");
        let sources = orch.get_sources_for(&patent);
        let names: Vec<&str> = sources.iter().map(|s| s.source_name()).collect();
        assert!(names.contains(&"USPTO"));
        assert!(names.contains(&"EPO_OPS")); // supports all
    }

    #[test]
    fn test_get_sources_for_wo_patent() {
        let tmp = TempDir::new().unwrap();
        let cfg = make_config(&tmp);
        let cache = PatentCache::new(&cfg).unwrap();
        let orch = FetcherOrchestrator::new(cfg, cache);
        let patent = crate::id_canon::canonicalize("WO2024123456");
        let sources = orch.get_sources_for(&patent);
        let names: Vec<&str> = sources.iter().map(|s| s.source_name()).collect();
        assert!(names.contains(&"WIPO_Scrape"));
        assert!(!names.contains(&"USPTO")); // US only
    }

    #[test]
    fn test_get_sources_for_au_patent() {
        let tmp = TempDir::new().unwrap();
        let cfg = make_config(&tmp);
        let cache = PatentCache::new(&cfg).unwrap();
        let orch = FetcherOrchestrator::new(cfg, cache);
        let patent = crate::id_canon::canonicalize("AU2023123456");
        let sources = orch.get_sources_for(&patent);
        let names: Vec<&str> = sources.iter().map(|s| s.source_name()).collect();
        assert!(names.contains(&"IP_Australia"));
        assert!(!names.contains(&"USPTO")); // US only
        assert!(names.contains(&"EPO_OPS")); // supports all
    }

    #[test]
    fn test_get_sources_for_ca_patent() {
        let tmp = TempDir::new().unwrap();
        let cfg = make_config(&tmp);
        let cache = PatentCache::new(&cfg).unwrap();
        let orch = FetcherOrchestrator::new(cfg, cache);
        let patent = crate::id_canon::canonicalize("CA3012345");
        let sources = orch.get_sources_for(&patent);
        let names: Vec<&str> = sources.iter().map(|s| s.source_name()).collect();
        assert!(names.contains(&"CIPO"));
        assert!(!names.contains(&"USPTO"));
    }

    #[test]
    fn test_source_priority_ordering() {
        let tmp = TempDir::new().unwrap();
        let mut cfg = make_config(&tmp);
        // Reverse the priority: put Espacenet first
        cfg.source_priority = vec![
            "Espacenet".into(),
            "USPTO".into(),
            "EPO_OPS".into(),
            "web_search".into(),
        ];
        let cache = PatentCache::new(&cfg).unwrap();
        let orch = FetcherOrchestrator::new(cfg, cache);
        let patent = crate::id_canon::canonicalize("US7654321");
        let sources = orch.get_sources_for(&patent);
        let names: Vec<&str> = sources.iter().map(|s| s.source_name()).collect();
        // Espacenet should come before USPTO since it's first in priority
        let esp_pos = names.iter().position(|n| *n == "Espacenet").unwrap();
        let uspto_pos = names.iter().position(|n| *n == "USPTO").unwrap();
        assert!(
            esp_pos < uspto_pos,
            "Espacenet should be before USPTO in custom priority order"
        );
    }

    #[tokio::test]
    async fn test_orchestrator_cache_hit() {
        let tmp = TempDir::new().unwrap();
        let cfg = make_config(&tmp);
        let cache = PatentCache::new(&cfg).unwrap();

        // Pre-populate cache
        let meta = PatentMetadata {
            canonical_id: "US7654321".into(),
            jurisdiction: "US".into(),
            doc_type: "patent".into(),
            title: Some("Cached Patent".into()),
            abstract_text: None,
            inventors: vec![],
            assignee: None,
            filing_date: None,
            publication_date: None,
            grant_date: None,
            fetched_at: "2024-01-01T00:00:00Z".into(),
            legal_status: None,
        };
        let txt_path = tmp.path().join("test.txt");
        std::fs::write(&txt_path, "test content").unwrap();
        let artifacts = ArtifactSet {
            pdf: None,
            txt: Some(txt_path),
            md: None,
            images: vec![],
        };
        cache
            .store("US7654321", &artifacts, &meta, None)
            .unwrap();

        let orch = FetcherOrchestrator::new(cfg, cache);
        let patent = crate::id_canon::canonicalize("US7654321");
        let result = orch.fetch(&patent, tmp.path()).await;
        assert!(result.from_cache);
        assert!(result.success);
    }

    #[test]
    fn test_web_search_not_in_sources() {
        // web_search should be handled separately, not in the sources list
        let tmp = TempDir::new().unwrap();
        let cfg = make_config(&tmp);
        let cache = PatentCache::new(&cfg).unwrap();
        let orch = FetcherOrchestrator::new(cfg, cache);
        let names: Vec<&str> = orch.sources.iter().map(|s| s.source_name()).collect();
        assert!(
            !names.contains(&"web_search"),
            "web_search should not be in the sources list"
        );
    }

    #[tokio::test]
    async fn test_fetch_merges_metadata_across_sources() {
        let tmp = TempDir::new().unwrap();
        let cfg = make_config(&tmp);
        let cache = PatentCache::new(&cfg).unwrap();

        let sparse = FetchResult {
            source_attempt: SourceAttempt {
                source: "sparse".into(),
                success: true,
                elapsed_ms: 1.0,
                error: None,
                metadata: None,
            },
            pdf_path: None,
            txt_path: None,
            metadata: Some(PatentMetadata {
                canonical_id: "US7654321".into(),
                jurisdiction: "US".into(),
                doc_type: "patent".into(),
                title: None,
                abstract_text: None,
                inventors: vec![],
                assignee: None,
                filing_date: None,
                publication_date: None,
                grant_date: None,
                fetched_at: "2026-01-01T00:00:00Z".into(),
                legal_status: None,
            }),
        };

        let rich = FetchResult {
            source_attempt: SourceAttempt {
                source: "rich".into(),
                success: true,
                elapsed_ms: 2.0,
                error: None,
                metadata: None,
            },
            pdf_path: None,
            txt_path: None,
            metadata: Some(PatentMetadata {
                canonical_id: "US7654321".into(),
                jurisdiction: "US".into(),
                doc_type: "patent".into(),
                title: Some("Useful patent title".into()),
                abstract_text: Some("Useful abstract".into()),
                inventors: vec!["Ada Lovelace".into()],
                assignee: Some("Patent Corp".into()),
                filing_date: Some("2020-01-01".into()),
                publication_date: Some("2021-01-01".into()),
                grant_date: None,
                fetched_at: "2026-01-01T00:00:01Z".into(),
                legal_status: None,
            }),
        };

        let orch = FetcherOrchestrator {
            config: cfg,
            cache,
            sources: vec![
                Box::new(StubSource {
                    name: "sparse",
                    jurisdictions: &["US"],
                    result: sparse,
                }),
                Box::new(StubSource {
                    name: "rich",
                    jurisdictions: &["US"],
                    result: rich,
                }),
            ],
        };

        let patent = crate::id_canon::canonicalize("US7654321");
        let result = orch.fetch(&patent, tmp.path()).await;

        assert!(result.success);
        assert_eq!(result.sources.len(), 2);
        let metadata = result.metadata.unwrap();
        assert_eq!(metadata.title.as_deref(), Some("Useful patent title"));
        assert_eq!(metadata.abstract_text.as_deref(), Some("Useful abstract"));
        assert_eq!(metadata.inventors, vec!["Ada Lovelace"]);
        assert_eq!(metadata.assignee.as_deref(), Some("Patent Corp"));
        assert_eq!(metadata.filing_date.as_deref(), Some("2020-01-01"));
        assert_eq!(metadata.publication_date.as_deref(), Some("2021-01-01"));
    }

    #[tokio::test]
    async fn test_success_reflects_web_search_fallback_result() {
        let tmp = TempDir::new().unwrap();
        let mut cfg = make_config(&tmp);
        cfg.source_priority = vec![];
        cfg.source_base_urls.insert(
            "DDG".into(),
            "http://127.0.0.1:9/unreachable-duckduckgo".into(),
        );
        let cache = PatentCache::new(&cfg).unwrap();

        let orch = FetcherOrchestrator {
            config: cfg,
            cache,
            sources: vec![],
        };

        let patent = crate::id_canon::canonicalize("US10000000");
        let result = orch.fetch_force_refresh(&patent, tmp.path()).await;

        assert!(result.success);
        assert!(result.sources.iter().any(|source| source.source == "web_search" && source.success));
    }
}
