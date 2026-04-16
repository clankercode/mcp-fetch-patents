use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use tracing::warn;
use crate::config::PrefetchConfig;
use crate::cooldown::SourceCooldown;
use crate::id_canon::CanonicalPatentId;
use crate::rate_limit::RateLimiter;

pub struct PrefetchQueue {
    jobs: Mutex<VecDeque<CanonicalPatentId>>,
    seen: Mutex<HashSet<String>>,
    rate_limiter: Arc<RateLimiter>,
    cooldown: Arc<SourceCooldown>,
    config: PrefetchConfig,
}

impl PrefetchQueue {
    pub fn new(config: &PrefetchConfig) -> Arc<Self> {
        Arc::new(PrefetchQueue {
            jobs: Mutex::new(VecDeque::new()),
            seen: Mutex::new(HashSet::new()),
            rate_limiter: Arc::new(RateLimiter::new(
                config.max_concurrent,
                config.hold_time_secs,
            )),
            cooldown: Arc::new(SourceCooldown::new()),
            config: config.clone(),
        })
    }

    pub fn enqueue(&self, ids: Vec<CanonicalPatentId>) {
        let mut jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
        let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
        for id in ids {
            if seen.insert(id.canonical.clone()) {
                jobs.push_back(id);
            }
        }
    }

    pub fn rate_limiter(&self) -> Arc<RateLimiter> {
        Arc::clone(&self.rate_limiter)
    }

    pub fn cooldown(&self) -> Arc<SourceCooldown> {
        Arc::clone(&self.cooldown)
    }

    pub async fn drain(
        &self,
        cache: &crate::cache::PatentCache,
        fetcher: &crate::fetchers::FetcherOrchestrator,
        output_base: &std::path::Path,
    ) {
        let mut processed = 0;
        loop {
            if processed >= self.config.limit {
                break;
            }
            let id = {
                let mut jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
                jobs.pop_front()
            };
            let Some(id) = id else {
                break;
            };
            let _ = self.seen.lock().map(|mut s| s.remove(&id.canonical));

            let source_name = fetcher.primary_source_for(&id);
            if !self.cooldown.is_cool(&source_name) {
                let wait = self.cooldown.wait_duration(&source_name).unwrap_or_default();
                if wait > std::time::Duration::ZERO {
                    tokio::time::sleep(wait).await;
                }
            }

            self.rate_limiter.acquire().await;

            let results = fetcher.fetch_batch(std::slice::from_ref(&id), output_base).await;
            for r in &results {
                if r.success && r.files.contains_key("pdf") {
                    let _ = cache.set_full_text_status(&r.canonical_id, crate::cache::FullTextStatus::Fetched);
                } else if r.success {
                    let _ = cache.set_full_text_status(&r.canonical_id, crate::cache::FullTextStatus::Failed("no_pdf".into()));
                } else {
                    let err_str = r.error.clone().unwrap_or_default();
                    if err_str.contains("429") || err_str.to_lowercase().contains("rate limit") {
                        self.cooldown.mark_rate_limited(&source_name);
                        warn!("Prefetch hit rate limit for {:?}: {}", id, err_str);
                    }
                    let _ = cache.set_full_text_status(&r.canonical_id, crate::cache::FullTextStatus::Failed(err_str));
                }
            }
            processed += 1;
        }
    }
}

impl Clone for PrefetchQueue {
    fn clone(&self) -> Self {
        PrefetchQueue {
            jobs: Mutex::new(VecDeque::new()),
            seen: Mutex::new(HashSet::new()),
            rate_limiter: Arc::clone(&self.rate_limiter),
            cooldown: Arc::clone(&self.cooldown),
            config: self.config.clone(),
        }
    }
}

use std::sync::Mutex;

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    use crate::cache::{PatentCache, PatentMetadata};
    use crate::config::PatentConfig;
    use crate::fetchers::{FetcherOrchestrator, FetchResult, PatentSource};
    use crate::id_canon::CanonicalPatentId;

    struct PdfSource;

    #[async_trait::async_trait]
    impl PatentSource for PdfSource {
        fn source_name(&self) -> &str {
            "StubPDF"
        }
        fn supported_jurisdictions(&self) -> &[&str] {
            &[]
        }
        async fn fetch(
            &self,
            patent: &CanonicalPatentId,
            output_dir: &std::path::Path,
            _config: &crate::config::PatentConfig,
        ) -> FetchResult {
            let pdf_path = output_dir.join(format!("{}.pdf", patent.canonical));
            std::fs::create_dir_all(output_dir).unwrap();
            std::fs::write(&pdf_path, b"%PDF-1.4 test").unwrap();
            FetchResult {
                source_attempt: crate::cache::SourceAttempt {
                    source: "StubPDF".into(),
                    success: true,
                    elapsed_ms: 1.0,
                    error: None,
                    metadata: None,
                },
                pdf_path: Some(pdf_path),
                txt_path: None,
                metadata: Some(PatentMetadata {
                    canonical_id: patent.canonical.clone(),
                    jurisdiction: patent.jurisdiction.clone(),
                    doc_type: "patent".into(),
                    title: Some("Test".into()),
                    abstract_text: None,
                    inventors: vec![],
                    assignee: None,
                    filing_date: None,
                    publication_date: None,
                    grant_date: None,
                    fetched_at: chrono::Utc::now().to_rfc3339(),
                    legal_status: None,
                }),
            }
        }
    }

    fn make_test_config(tmp: &TempDir) -> PatentConfig {
        PatentConfig {
            cache_local_dir: tmp.path().join("cache").join("patents"),
            cache_global_db: tmp.path().join("global").join("index.db"),
            source_priority: vec!["StubPDF".into()],
            concurrency: 5,
            fetch_all_sources: false,
            timeout_secs: 30.0,
            converters_order: vec![],
            converters_disabled: vec!["marker".into()],
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
            search_backend_default: "browser".into(),
            search_enrich_top_n: 5,
            prefetch: crate::config::PrefetchConfig::default(),
        }
    }

    #[tokio::test]
    async fn prefetch_downloads_pdf_and_updates_status() {
        let tmp = TempDir::new().unwrap();
        let cfg = make_test_config(&tmp);
        let cache = std::sync::Arc::new(PatentCache::new(&cfg).unwrap());

        let fetcher = FetcherOrchestrator::with_sources(
            cfg.clone(),
            cache.clone(),
            vec![Box::new(PdfSource)],
        );

        let queue = PrefetchQueue::new(
            &crate::config::PrefetchConfig {
                full_text: true,
                limit: 2,
                max_concurrent: 1,
                hold_time_secs: 1,
            },
        );

        let patent = CanonicalPatentId {
            raw: "US1234567".into(),
            canonical: "US1234567".into(),
            jurisdiction: "US".into(),
            number: "1234567".into(),
            kind_code: None,
            doc_type: "patent".into(),
            filing_year: None,
            errors: vec![],
        };

        let pdf_path = cfg.cache_local_dir.join("US1234567").join("US1234567.pdf");
        println!("[smoke] Cache dir before drain: {}", cfg.cache_local_dir.display());
        println!("[smoke] PDF expected at: {}", pdf_path.display());
        println!("[smoke] DB status before: {:?}", cache.get_full_text_status("US1234567"));

        queue.enqueue(vec![patent.clone()]);
        queue.drain(
            &cache, &fetcher, &cfg.cache_local_dir).await;

        println!("[smoke] PDF exists after drain: {}", pdf_path.exists());
        if pdf_path.exists() {
            let meta = std::fs::metadata(&pdf_path).unwrap();
            println!("[smoke] PDF size: {} bytes", meta.len());
        }
        println!("[smoke] DB status after: {:?}", cache.get_full_text_status("US1234567"));

        assert!(pdf_path.exists(), "PDF should have been prefetched");

        let status = cache.get_full_text_status("US1234567").unwrap();
        assert_eq!(status, crate::cache::FullTextStatus::Fetched);
    }
}
