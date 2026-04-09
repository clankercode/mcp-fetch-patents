use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;
use tracing::warn;

use crate::config::PatentConfig;
use crate::ranking::{ScoredHit, SearchIntent};

pub struct SearchOrchestrator<'a> {
    pub config: &'a PatentConfig,
    pub backends: &'a super::SearchBackends,
    pub fetch_orchestrator: &'a crate::fetchers::FetcherOrchestrator,
}

impl<'a> SearchOrchestrator<'a> {
    pub fn new(
        config: &'a PatentConfig,
        backends: &'a super::SearchBackends,
        fetch_orchestrator: &'a crate::fetchers::FetcherOrchestrator,
    ) -> Self {
        Self {
            config,
            backends,
            fetch_orchestrator,
        }
    }
}

pub struct SearchOptions {
    pub max_results: usize,
    pub backend: String,
    pub enrich_top_n: usize,
    pub profile_name: String,
    pub debug: bool,
    pub date_cutoff: Option<String>,
}

pub struct SearchResult {
    pub scored: Vec<ScoredHit>,
    pub intent: SearchIntent,
    pub queries_run: Vec<Value>,
    pub enriched_ids: Vec<String>,
    pub elapsed_ms: f64,
}

impl<'a> SearchOrchestrator<'a> {
    pub async fn natural_language_search(
        &self,
        description: &str,
        jurisdictions: Option<&[String]>,
        opts: SearchOptions,
    ) -> Result<SearchResult> {
        let start = std::time::Instant::now();

        let planner = crate::planner::NaturalLanguagePlanner;
        let intent = planner.plan(description, opts.date_cutoff.as_deref(), jurisdictions);

        let mut hits_by_query: HashMap<String, Vec<crate::ranking::PatentHit>> = HashMap::new();
        let mut queries_run: Vec<Value> = Vec::new();

        let effective_backend = if opts.backend == "auto" {
            self.config.search_backend_default.clone()
        } else {
            opts.backend.clone()
        };

        if effective_backend == "browser" || effective_backend == "auto" {
            let browser_cfg = &self.backends.browser_config;
            let debug_dir = if opts.debug {
                browser_cfg
                    .debug_html_dir
                    .clone()
                    .or_else(|| Some(".patent-debug".into()))
            } else {
                browser_cfg.debug_html_dir.clone()
            };
            let browser = crate::search::browser_search::GooglePatentsBrowserSearch::new(
                browser_cfg.profiles_dir.clone(),
                &opts.profile_name,
                browser_cfg.headless,
                browser_cfg.timeout_ms,
                browser_cfg.max_pages,
                debug_dir,
            );
            for variant in &intent.query_variants {
                if hits_by_query.contains_key(&variant.query) {
                    continue;
                }
                match browser
                    .search(
                        &variant.query,
                        opts.date_cutoff.as_deref(),
                        None,
                        opts.max_results,
                    )
                    .await
                {
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
                    Ok(_) => {
                        queries_run.push(serde_json::json!({
                            "source": "Google_Patents_Browser",
                            "query": variant.query,
                            "variant_type": variant.variant_type,
                            "result_count": 0,
                        }));
                    }
                    Err(e) => {
                        warn!("Browser search failed for '{}': {}", variant.query, e);
                        queries_run.push(serde_json::json!({
                            "source": "Google_Patents_Browser",
                            "query": variant.query,
                            "variant_type": variant.variant_type,
                            "result_count": 0,
                            "error": e.to_string(),
                        }));
                    }
                }
            }
        }

        let original_backend = opts.backend.as_str();
        if effective_backend == "serpapi"
            || (effective_backend == "auto")
            || (original_backend == "auto" && hits_by_query.is_empty())
        {
            if let Some(ref serp) = self.backends.serpapi {
                let serp_variants: Vec<_> = intent
                    .query_variants
                    .iter()
                    .filter(|v| !hits_by_query.contains_key(&v.query))
                    .collect();
                let serp_futures: Vec<_> = serp_variants
                    .iter()
                    .map(|variant| {
                        serp.search(
                            &variant.query,
                            None,
                            opts.date_cutoff.as_deref(),
                            None,
                            None,
                            None,
                            opts.max_results,
                        )
                    })
                    .collect();
                let serp_results = futures::future::join_all(serp_futures).await;
                for (variant, result) in serp_variants.iter().zip(serp_results) {
                    let hits = result.unwrap_or_else(|e| {
                        warn!("SerpAPI search failed: {}", e);
                        vec![]
                    });
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
        scored.truncate(opts.max_results);

        let mut enriched_ids: Vec<String> = Vec::new();
        if opts.enrich_top_n > 0 && !scored.is_empty() {
            let scored_canonical: Vec<(usize, crate::id_canon::CanonicalPatentId)> = scored
                .iter()
                .take(opts.enrich_top_n)
                .enumerate()
                .filter_map(|(i, s)| {
                    let cid = crate::id_canon::canonicalize(&s.hit.patent_id);
                    if cid.canonical.is_empty() {
                        None
                    } else {
                        Some((i, cid))
                    }
                })
                .collect();
            if !scored_canonical.is_empty() {
                let patent_ids: Vec<crate::id_canon::CanonicalPatentId> = scored_canonical
                    .iter()
                    .map(|(_, cid)| cid.clone())
                    .collect();
                let output_base = &self.config.cache_local_dir;
                let results = self
                    .fetch_orchestrator
                    .fetch_batch(&patent_ids, output_base)
                    .await;
                let result_map: HashMap<String, &crate::fetchers::OrchestratorResult> = results
                    .iter()
                    .filter(|r| r.success && r.metadata.is_some())
                    .map(|r| (r.canonical_id.clone(), r))
                    .collect();
                for (i, cid) in &scored_canonical {
                    if let Some(result) = result_map.get(&cid.canonical) {
                        if let Some(ref meta) = result.metadata {
                            let s = &mut scored[*i];
                            if s.hit.title.is_none() {
                                s.hit.title = meta.title.clone();
                            }
                            if s.hit.abstract_text.is_none() {
                                s.hit.abstract_text = meta.abstract_text.clone();
                            }
                            if s.hit.assignee.is_none() {
                                s.hit.assignee = meta.assignee.clone();
                            }
                            if s.hit.inventors.is_empty() && !meta.inventors.is_empty() {
                                s.hit.inventors = meta.inventors.clone();
                            }
                            if s.hit.date.is_none() {
                                s.hit.date = meta.publication_date.clone();
                            }
                            enriched_ids.push(cid.canonical.clone());
                        }
                    }
                }
            }
        }

        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;

        Ok(SearchResult {
            scored,
            intent,
            queries_run,
            enriched_ids,
            elapsed_ms,
        })
    }
}
