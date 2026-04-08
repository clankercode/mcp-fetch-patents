//! Web search fallback source — last resort when all structured sources fail.
//! Mirrors Python patent_mcp.fetchers.web_search module.

use std::collections::HashSet;
use std::path::Path;
use std::time::Instant;

use reqwest::Client;
use serde_json::json;
use tracing::debug;

use crate::cache::SourceAttempt;
use crate::config::PatentConfig;
use crate::fetchers::FetchResult;
use crate::id_canon::CanonicalPatentId;

// ---------------------------------------------------------------------------
// Domain confidence sets
// ---------------------------------------------------------------------------

const HIGH_CONFIDENCE_DOMAINS: &[&str] = &[
    "patents.google.com",
    "ppubs.uspto.gov",
    "ops.epo.org",
    "patentscope.wipo.int",
    "worldwide.espacenet.com",
    "patents.justia.com",
    "lens.org",
    "freepatentsonline.com",
];

const MEDIUM_CONFIDENCE_DOMAINS: &[&str] = &[
    "patentyogi.com",
    "patent.ifixit.com",
];

// ---------------------------------------------------------------------------
// Query generation
// ---------------------------------------------------------------------------

/// Generate a ranked list of search queries for a patent.
/// Mirrors Python `patent_mcp.fetchers.web_search.generate_queries`.
pub fn generate_queries(patent: &CanonicalPatentId) -> Vec<String> {
    let cid = &patent.canonical;
    let mut queries = vec![format!("\"{}\" patent PDF", cid)];

    match patent.jurisdiction.as_str() {
        "US" => {
            queries.push(format!("{} patent full text", cid));
            queries.push(format!("site:patents.google.com {}", cid));
            queries.push(format!("site:ppubs.uspto.gov {}", cid));
        }
        "EP" => {
            queries.push(format!("{} patent European Patent Office", cid));
            queries.push(format!("site:epo.org {}", cid));
            queries.push(format!("site:worldwide.espacenet.com {}", cid));
        }
        "WO" => {
            queries.push(format!("{} PCT international patent", cid));
            queries.push(format!("site:patentscope.wipo.int {}", cid));
        }
        _ => {
            queries.push(format!("{} patent full text PDF", cid));
            queries.push(format!("{} {} patent office", cid, patent.jurisdiction));
        }
    }

    queries
}

// ---------------------------------------------------------------------------
// URL confidence scoring
// ---------------------------------------------------------------------------

/// Score a URL's relevance to a patent: "high", "medium", or "low".
/// Mirrors Python `patent_mcp.fetchers.web_search.score_url_confidence`.
pub fn score_url_confidence(url: &str, canonical_id: &str) -> &'static str {
    // Extract domain from URL using simple string parsing
    let domain = extract_domain(url);

    // If the canonical ID appears in the URL (case-insensitive), high confidence
    if url.to_uppercase().contains(&canonical_id.to_uppercase()) {
        return "high";
    }
    if HIGH_CONFIDENCE_DOMAINS.contains(&domain.as_str()) {
        return "high";
    }
    if MEDIUM_CONFIDENCE_DOMAINS.contains(&domain.as_str()) {
        return "medium";
    }
    if domain.contains("patent") {
        return "medium";
    }
    "low"
}

/// Extract domain from a URL, stripping "www." prefix.
/// Mirrors Python `urlparse(url).netloc.lower().lstrip("www.")`.
fn extract_domain(url: &str) -> String {
    // Find the netloc portion: after "://" and before the next "/" or end
    let after_scheme = if let Some(idx) = url.find("://") {
        &url[idx + 3..]
    } else {
        url
    };

    // Take everything before the next "/" (or end)
    let netloc = if let Some(idx) = after_scheme.find('/') {
        &after_scheme[..idx]
    } else {
        after_scheme
    };

    // Strip port if present
    let host = if let Some(idx) = netloc.rfind(':') {
        // Only strip if what follows looks like a port number
        if netloc[idx + 1..].chars().all(|c| c.is_ascii_digit()) {
            &netloc[..idx]
        } else {
            netloc
        }
    } else {
        netloc
    };

    // Strip userinfo if present (user:pass@host)
    let host = if let Some(idx) = host.rfind('@') {
        &host[idx + 1..]
    } else {
        host
    };

    let lowered = host.to_lowercase();
    // Match Python's .lstrip("www.") — strips any leading characters that are in
    // the set {'w', '.'}, not just the literal prefix "www.".
    // e.g. "www.example.com" → "example.com", "worldwide.foo.com" → "orldwide.foo.com"
    // This intentionally mirrors Python's lstrip behavior (even though it's a quirk).
    let chars_to_strip: &[char] = &['w', '.'];
    lowered
        .trim_start_matches(|c| chars_to_strip.contains(&c))
        .to_string()
}

// ---------------------------------------------------------------------------
// DuckDuckGo backend
// ---------------------------------------------------------------------------

struct DuckDuckGoBackend;

impl DuckDuckGoBackend {
    async fn search(config: &PatentConfig, query: &str) -> Vec<String> {
        let base = config
            .source_base_urls
            .get("DDG")
            .map(|s| s.as_str())
            .unwrap_or("https://api.duckduckgo.com/");
        let client = Client::new();
        match client
            .get(base)
            .query(&[("q", query), ("format", "json"), ("no_html", "1")])
            .timeout(std::time::Duration::from_secs(15))
            .send()
            .await
        {
            Ok(resp) => {
                let resp = match resp.error_for_status() {
                    Ok(r) => r,
                    Err(e) => {
                        debug!("DDG search returned error status: {}", e);
                        return vec![];
                    }
                };
                if let Ok(data) = resp.json::<serde_json::Value>().await {
                    let mut urls = Vec::new();
                    // Extract from "Results" array
                    if let Some(results) = data.get("Results").and_then(|r| r.as_array()) {
                        for r in results {
                            if let Some(url) = r
                                .get("FirstURL")
                                .or(r.get("url"))
                                .and_then(|u| u.as_str())
                            {
                                urls.push(url.to_string());
                            }
                        }
                    }
                    // Extract from "RelatedTopics" array
                    if let Some(topics) = data.get("RelatedTopics").and_then(|t| t.as_array()) {
                        for t in topics {
                            if let Some(url) = t.get("FirstURL").and_then(|u| u.as_str()) {
                                urls.push(url.to_string());
                            }
                        }
                    }
                    urls
                } else {
                    vec![]
                }
            }
            Err(e) => {
                debug!("DDG search failed: {}", e);
                vec![]
            }
        }
    }
}

// ---------------------------------------------------------------------------
// SerpAPI backend
// ---------------------------------------------------------------------------

struct SerpApiBackend;

impl SerpApiBackend {
    async fn search(config: &PatentConfig, query: &str) -> Vec<String> {
        let api_key = match &config.serpapi_key {
            Some(k) => k,
            None => return vec![],
        };
        let base = config
            .source_base_urls
            .get("SerpAPI")
            .map(|s| s.as_str())
            .unwrap_or("https://serpapi.com/search");
        let client = Client::new();
        match client
            .get(base)
            .query(&[
                ("q", query),
                ("api_key", api_key.as_str()),
                ("engine", "google"),
            ])
            .timeout(std::time::Duration::from_secs(20))
            .send()
            .await
        {
            Ok(resp) => {
                let resp = match resp.error_for_status() {
                    Ok(r) => r,
                    Err(e) => {
                        debug!("SerpAPI search returned error status: {}", e);
                        return vec![];
                    }
                };
                if let Ok(data) = resp.json::<serde_json::Value>().await {
                    data.get("organic_results")
                        .and_then(|r| r.as_array())
                        .map(|results| {
                            results
                                .iter()
                                .filter_map(|r| {
                                    r.get("link").and_then(|l| l.as_str()).map(String::from)
                                })
                                .collect()
                        })
                        .unwrap_or_default()
                } else {
                    vec![]
                }
            }
            Err(e) => {
                debug!("SerpAPI search failed: {}", e);
                vec![]
            }
        }
    }
}

// ---------------------------------------------------------------------------
// WebSearchFallbackSource
// ---------------------------------------------------------------------------

/// Last-resort web search fallback; returns URLs only, never writes files.
/// Mirrors Python `patent_mcp.fetchers.web_search.WebSearchFallbackSource`.
pub struct WebSearchFallbackSource;

impl WebSearchFallbackSource {
    pub fn source_name() -> &'static str {
        "web_search"
    }

    pub async fn fetch(
        patent: &CanonicalPatentId,
        _output_dir: &Path,
        config: &PatentConfig,
    ) -> FetchResult {
        let start = Instant::now();
        let queries = generate_queries(patent);

        let mut all_urls = Vec::new();
        // Limit to first 2 queries (matches Python)
        for q in queries.iter().take(2) {
            let urls = DuckDuckGoBackend::search(config, q).await;
            if urls.is_empty() && config.serpapi_key.is_some() {
                let fallback_urls = SerpApiBackend::search(config, q).await;
                all_urls.extend(fallback_urls);
            } else {
                all_urls.extend(urls);
            }
        }

        // Deduplicate and score
        let mut seen = HashSet::new();
        let scored: Vec<serde_json::Value> = all_urls
            .into_iter()
            .filter(|url| seen.insert(url.clone()))
            .map(|url| {
                let confidence = score_url_confidence(&url, &patent.canonical);
                json!({"url": url, "confidence": confidence})
            })
            .collect();

        let elapsed = start.elapsed().as_secs_f64() * 1000.0;

        FetchResult {
            source_attempt: SourceAttempt {
                source: "web_search".into(),
                success: true, // web search always "succeeds" if it runs
                elapsed_ms: elapsed,
                error: None,
                metadata: Some({
                    let mut map = std::collections::HashMap::new();
                    map.insert("urls".to_string(), serde_json::Value::Array(scored));
                    map.insert(
                        "note".to_string(),
                        json!("Web search fallback — no structured sources returned results. URLs returned for manual review or agent use."),
                    );
                    map.insert("formats_retrieved".to_string(), json!([]));
                    map
                }),
            },
            pdf_path: None,
            txt_path: None,
            metadata: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_queries_us() {
        let patent = crate::id_canon::canonicalize("US7654321");
        let queries = generate_queries(&patent);
        assert!(queries[0].contains("US7654321"));
        assert!(queries[0].contains("patent PDF"));
        // US-specific queries
        assert!(queries
            .iter()
            .any(|q| q.contains("site:patents.google.com")));
        assert!(queries.iter().any(|q| q.contains("site:ppubs.uspto.gov")));
    }

    #[test]
    fn test_generate_queries_ep() {
        let patent = crate::id_canon::canonicalize("EP1234567");
        let queries = generate_queries(&patent);
        assert!(queries[0].contains("EP1234567"));
        assert!(queries
            .iter()
            .any(|q| q.contains("European Patent Office")));
        assert!(queries.iter().any(|q| q.contains("site:epo.org")));
    }

    #[test]
    fn test_generate_queries_wo() {
        let patent = crate::id_canon::canonicalize("WO2024123456");
        let queries = generate_queries(&patent);
        assert!(queries
            .iter()
            .any(|q| q.contains("PCT international patent")));
        assert!(queries
            .iter()
            .any(|q| q.contains("site:patentscope.wipo.int")));
    }

    #[test]
    fn test_generate_queries_generic() {
        let patent = crate::id_canon::canonicalize("AU2023123456");
        let queries = generate_queries(&patent);
        assert!(queries
            .iter()
            .any(|q| q.contains("AU") && q.contains("patent office")));
    }

    #[test]
    fn test_score_url_confidence_high_domain() {
        assert_eq!(
            score_url_confidence(
                "https://patents.google.com/patent/US7654321",
                "US7654321"
            ),
            "high"
        );
        assert_eq!(
            score_url_confidence(
                "https://ppubs.uspto.gov/patent/US7654321",
                "US7654321"
            ),
            "high"
        );
    }

    #[test]
    fn test_score_url_confidence_id_in_url() {
        assert_eq!(
            score_url_confidence("https://example.com/US7654321", "US7654321"),
            "high"
        );
    }

    #[test]
    fn test_score_url_confidence_medium() {
        assert_eq!(
            score_url_confidence("https://patentyogi.com/some-article", "US7654321"),
            "medium"
        );
        // Any domain with "patent" in it
        assert_eq!(
            score_url_confidence("https://mypatentsite.com/article", "US7654321"),
            "medium"
        );
    }

    #[test]
    fn test_score_url_confidence_low() {
        assert_eq!(
            score_url_confidence("https://example.com/article", "US7654321"),
            "low"
        );
    }

    #[test]
    fn test_extract_domain_simple() {
        assert_eq!(extract_domain("https://patents.google.com/patent/US123"), "patents.google.com");
        assert_eq!(extract_domain("https://www.example.com/page"), "example.com");
        assert_eq!(extract_domain("http://patentyogi.com/article"), "patentyogi.com");
    }

    #[test]
    fn test_extract_domain_with_port() {
        assert_eq!(extract_domain("https://example.com:8080/page"), "example.com");
    }
}
