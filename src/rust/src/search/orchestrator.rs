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
    pub backend_requested: String,
    pub backend_effective: String,
    pub fallback_used: bool,
}

fn resolve_effective_backend(requested_backend: &str, configured_default: &str) -> String {
    if requested_backend != "auto" {
        return requested_backend.to_string();
    }
    if configured_default == "auto" {
        "browser".to_string()
    } else {
        configured_default.to_string()
    }
}

fn classify_attempt_error(error: &str) -> (&'static str, bool, Option<u16>) {
    let lower = error.to_ascii_lowercase();
    let http_status = if lower.contains("429") {
        Some(429)
    } else if lower.contains("403") {
        Some(403)
    } else {
        None
    };

    if http_status.is_some()
        || lower.contains("rate limited")
        || lower.contains("rate limit")
        || lower.contains("unusual traffic")
        || lower.contains("captcha")
        || lower.contains("not a robot")
    {
        ("rate_limited", true, http_status)
    } else if lower.contains("unavailable")
        || lower.contains("not installed")
        || lower.contains("profile lock")
        || lower.contains("profile busy")
        || lower.contains("websocket")
        || lower.contains("connection closed")
    {
        ("unavailable", false, http_status)
    } else {
        ("error", false, http_status)
    }
}

fn build_query_attempt(
    source: &str,
    query: &str,
    variant_type: &str,
    status: &str,
    result_count: usize,
    error: Option<&str>,
    http_status: Option<u16>,
    rate_limited: bool,
) -> Value {
    let mut value = serde_json::json!({
        "source": source,
        "query": query,
        "variant_type": variant_type,
        "status": status,
        "used": true,
        "rate_limited": rate_limited,
        "result_count": result_count,
    });
    if let Some(err) = error {
        value["error"] = serde_json::json!(err);
    }
    if let Some(status_code) = http_status {
        value["http_status"] = serde_json::json!(status_code);
    }
    value
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

        let requested_backend = opts.backend.clone();
        let effective_backend =
            resolve_effective_backend(&requested_backend, &self.config.search_backend_default);

        if effective_backend == "browser" {
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
                self.backends.browser_pool.clone(),
                &opts.profile_name,
                browser_cfg.timeout_ms,
                browser_cfg.max_pages,
                debug_dir,
                browser_cfg.profiles_dir.clone(),
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
                        queries_run.push(build_query_attempt(
                            "Google_Patents_Browser",
                            &variant.query,
                            &variant.variant_type,
                            "success",
                            count,
                            None,
                            None,
                            false,
                        ));
                        hits_by_query.insert(variant.query.clone(), hits);
                    }
                    Ok(_) => {
                        queries_run.push(build_query_attempt(
                            "Google_Patents_Browser",
                            &variant.query,
                            &variant.variant_type,
                            "empty",
                            0,
                            None,
                            None,
                            false,
                        ));
                    }
                    Err(e) => {
                        warn!("Browser search failed for '{}': {}", variant.query, e);
                        let error_text = e.to_string();
                        let (status, rate_limited, http_status) =
                            classify_attempt_error(&error_text);
                        queries_run.push(build_query_attempt(
                            "Google_Patents_Browser",
                            &variant.query,
                            &variant.variant_type,
                            status,
                            0,
                            Some(&error_text),
                            http_status,
                            rate_limited,
                        ));
                    }
                }
            }
        }

        let serp_variants: Vec<_> = intent
            .query_variants
            .iter()
            .filter(|v| !hits_by_query.contains_key(&v.query))
            .collect();
        let should_try_serpapi = effective_backend == "serpapi"
            || (requested_backend == "auto" && !serp_variants.is_empty());
        let mut fallback_used = false;
        if should_try_serpapi {
            if let Some(ref serp) = self.backends.serpapi {
                fallback_used = effective_backend == "browser" && requested_backend == "auto";
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
                    match result {
                        Ok(hits) => {
                            let count = hits.len();
                            let status = if count == 0 { "empty" } else { "success" };
                            queries_run.push(build_query_attempt(
                                "Google_Patents_SerpAPI",
                                &variant.query,
                                &variant.variant_type,
                                status,
                                count,
                                None,
                                None,
                                false,
                            ));
                            hits_by_query.insert(variant.query.clone(), hits);
                        }
                        Err(e) => {
                            warn!("SerpAPI search failed: {}", e);
                            let error_text = e.to_string();
                            let (status, rate_limited, http_status) =
                                classify_attempt_error(&error_text);
                            queries_run.push(build_query_attempt(
                                "Google_Patents_SerpAPI",
                                &variant.query,
                                &variant.variant_type,
                                status,
                                0,
                                Some(&error_text),
                                http_status,
                                rate_limited,
                            ));
                        }
                    }
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

        let elapsed_ms = crate::elapsed_ms(start);

        Ok(SearchResult {
            scored,
            intent,
            queries_run,
            enriched_ids,
            elapsed_ms,
            backend_requested: requested_backend,
            backend_effective: effective_backend,
            fallback_used,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_options_default_fields() {
        let opts = SearchOptions {
            max_results: 25,
            backend: "auto".to_string(),
            enrich_top_n: 5,
            profile_name: "default".to_string(),
            debug: false,
            date_cutoff: None,
        };
        assert_eq!(opts.max_results, 25);
        assert_eq!(opts.backend, "auto");
        assert!(opts.date_cutoff.is_none());
    }

    #[test]
    fn search_result_fields() {
        let intent = crate::planner::NaturalLanguagePlanner.plan("wireless charging", None, None);
        assert!(!intent.query_variants.is_empty());

        let result = SearchResult {
            scored: vec![],
            intent,
            queries_run: vec![serde_json::json!({"source": "test", "query": "q1"})],
            enriched_ids: vec![],
            elapsed_ms: 42.5,
            backend_requested: "auto".to_string(),
            backend_effective: "browser".to_string(),
            fallback_used: false,
        };
        assert!(result.scored.is_empty());
        assert_eq!(result.queries_run.len(), 1);
        assert_eq!(result.elapsed_ms, 42.5);
    }

    #[test]
    fn planner_ranker_pipeline() {
        let planner = crate::planner::NaturalLanguagePlanner;
        let intent = planner.plan("neural network for image classification", None, None);

        assert!(!intent.concepts.is_empty());
        assert!(!intent.query_variants.is_empty());

        let mut hits_by_query = std::collections::HashMap::new();
        hits_by_query.insert(
            intent.query_variants[0].query.clone(),
            vec![crate::ranking::PatentHit::new(
                "US12345678".to_string(),
                crate::search::searchers::SOURCE_USPTO,
            )],
        );

        let ranker = crate::ranking::SearchRanker;
        let scored = ranker.rank(&hits_by_query, &intent);
        assert_eq!(scored.len(), 1);
        assert_eq!(scored[0].hit.patent_id, "US12345678");
        assert!(scored[0].score >= 0.0);
    }

    #[test]
    fn effective_backend_auto_resolves() {
        assert_eq!(resolve_effective_backend("auto", "browser"), "browser");
        assert_eq!(resolve_effective_backend("auto", "auto"), "browser");
        assert_eq!(resolve_effective_backend("serpapi", "browser"), "serpapi");
    }

    #[test]
    fn classify_attempt_error_marks_rate_limits() {
        let (status, rate_limited, http_status) =
            classify_attempt_error("SerpAPI Google Patents rate limited: HTTP 429");
        assert_eq!(status, "rate_limited");
        assert!(rate_limited);
        assert_eq!(http_status, Some(429));

        let (status, rate_limited, _) =
            classify_attempt_error("Google Patents browser unavailable: profile lock failed");
        assert_eq!(status, "unavailable");
        assert!(!rate_limited);

        let (status, rate_limited, _) =
            classify_attempt_error("Google Patents browser websocket error: connection closed");
        assert_eq!(status, "unavailable");
        assert!(!rate_limited);
    }
}
