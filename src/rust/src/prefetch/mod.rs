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
