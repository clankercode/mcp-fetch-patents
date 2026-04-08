//! HTTP-based patent sources — mirrors Python patent_mcp.fetchers.http module.
//!
//! Each struct implements the `PatentSource` trait, mirroring the Python
//! `BasePatentSource` ABC.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use reqwest::Client;
use tokio::time::sleep;
use tracing::{debug, warn};

use crate::cache::{PatentMetadata, SessionCache, SourceAttempt};
use crate::config::PatentConfig;
use crate::fetchers::{FetchResult, PatentSource};
use crate::id_canon::CanonicalPatentId;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn now_iso() -> String {
    Utc::now().to_rfc3339()
}

/// Return the base URL for a source, using config override or the given default.
fn base_url(config: &PatentConfig, key: &str, default: &str) -> String {
    config
        .source_base_urls
        .get(key)
        .cloned()
        .unwrap_or_else(|| default.to_string())
}

/// Retry a cloneable request on transient failures (429, 5xx, timeout, connect error).
#[allow(dead_code)]
async fn fetch_with_retry(
    _client: &Client,
    request_builder: reqwest::RequestBuilder,
    max_attempts: u32,
) -> Result<reqwest::Response> {
    let mut last_err = None;
    for attempt in 0..max_attempts {
        if attempt > 0 {
            let delay = Duration::from_secs(1 << attempt.min(3)); // 2s, 4s, 8s
            sleep(delay).await;
        }
        match request_builder.try_clone() {
            Some(rb) => match rb.send().await {
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    if status == 429 || (500..=504).contains(&status) {
                        last_err =
                            Some(anyhow::anyhow!("HTTP {} (attempt {})", status, attempt + 1));
                        continue;
                    }
                    return Ok(resp);
                }
                Err(e) => {
                    if e.is_timeout() || e.is_connect() {
                        last_err = Some(e.into());
                        continue;
                    }
                    return Err(e.into());
                }
            },
            None => return Err(anyhow::anyhow!("Request not cloneable")),
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("All retry attempts exhausted")))
}

/// Helper to create a `FetchResult` for a failed attempt.
fn fail_result(source: &str, error: &str) -> FetchResult {
    FetchResult {
        source_attempt: SourceAttempt {
            source: source.into(),
            success: false,
            elapsed_ms: 0.0,
            error: Some(error.into()),
            metadata: None,
        },
        pdf_path: None,
        txt_path: None,
        metadata: None,
    }
}

// ---------------------------------------------------------------------------
// PatentsView Stub (deprecated)
// ---------------------------------------------------------------------------

/// PatentsView was shut down March 20, 2026. Returns helpful error.
pub struct PatentsViewStubSource;

#[async_trait]
impl PatentSource for PatentsViewStubSource {
    fn source_name(&self) -> &str {
        "PatentsView"
    }

    fn supported_jurisdictions(&self) -> &[&str] {
        &["US"]
    }

    async fn fetch(
        &self,
        _patent: &CanonicalPatentId,
        _output_dir: &Path,
        _config: &PatentConfig,
    ) -> FetchResult {
        FetchResult {
            source_attempt: SourceAttempt {
                source: "PatentsView".into(),
                success: false,
                elapsed_ms: 0.0,
                error: Some(
                    "PatentsView API was shut down March 20, 2026. Use USPTO ODP instead.".into(),
                ),
                metadata: None,
            },
            pdf_path: None,
            txt_path: None,
            metadata: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Espacenet (HTML scraping, all jurisdictions)
// ---------------------------------------------------------------------------

/// Espacenet — scrape HTML for metadata + PDF links.
pub struct EspacenetSource;

#[async_trait]
impl PatentSource for EspacenetSource {
    fn source_name(&self) -> &str {
        "Espacenet"
    }

    fn supported_jurisdictions(&self) -> &[&str] {
        &[] // supports all
    }

    async fn fetch(
        &self,
        patent: &CanonicalPatentId,
        _output_dir: &Path,
        config: &PatentConfig,
    ) -> FetchResult {
        let start = std::time::Instant::now();
        let base = base_url(config, "Espacenet", "https://worldwide.espacenet.com");
        let source = self.source_name();

        let url = format!("{}/patent/{}", base, patent.canonical);
        let client = match Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs as u64))
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                return fail_result(source, &format!("Client build error: {}", e));
            }
        };

        let resp = match client
            .get(&url)
            .header("Accept-Language", "en")
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                let mut res = fail_result(source, &e.to_string());
                res.source_attempt.elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
                return res;
            }
        };

        let status = resp.status();
        if status.as_u16() == 404 {
            let mut res = fail_result(source, "not_found");
            res.source_attempt.elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
            return res;
        }

        let body = match resp.text().await {
            Ok(b) => b,
            Err(e) => {
                let mut res = fail_result(source, &e.to_string());
                res.source_attempt.elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
                return res;
            }
        };

        // Parse HTML with scraper
        let document = scraper::Html::parse_document(&body);

        // Title: look for h2.title first, then h1
        let title = {
            let h2_sel = scraper::Selector::parse("h2.title").ok();
            let h1_sel = scraper::Selector::parse("h1").ok();
            h2_sel
                .and_then(|s| document.select(&s).next())
                .or_else(|| h1_sel.and_then(|s| document.select(&s).next()))
                .map(|el| el.text().collect::<String>().trim().to_string())
                .filter(|t| !t.is_empty())
        };

        // Look for PDF link
        let _pdf_url: Option<String> = {
            let a_sel = scraper::Selector::parse("a[href]").ok();
            a_sel.and_then(|s| {
                document.select(&s).find_map(|el| {
                    let href = el.value().attr("href")?;
                    let lower = href.to_lowercase();
                    if lower.contains(".pdf") || lower.contains("download") {
                        let full = if href.starts_with("http") {
                            href.to_string()
                        } else {
                            format!("{}{}", base, href)
                        };
                        Some(full)
                    } else {
                        None
                    }
                })
            })
        };

        let meta = PatentMetadata {
            canonical_id: patent.canonical.clone(),
            jurisdiction: patent.jurisdiction.clone(),
            doc_type: patent.doc_type.clone(),
            title,
            abstract_text: None,
            inventors: vec![],
            assignee: None,
            filing_date: None,
            publication_date: None,
            grant_date: None,
            fetched_at: now_iso(),
            legal_status: None,
        };

        FetchResult {
            source_attempt: SourceAttempt {
                source: source.into(),
                success: true,
                elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
                error: None,
                metadata: None,
            },
            pdf_path: None,
            txt_path: None,
            metadata: Some(meta),
        }
    }
}

// ---------------------------------------------------------------------------
// WIPO PatentScope scraping (WO only)
// ---------------------------------------------------------------------------

/// WIPO PatentScope — scrape for WO (PCT) patent data.
pub struct WipoScrapeSource;

#[async_trait]
impl PatentSource for WipoScrapeSource {
    fn source_name(&self) -> &str {
        "WIPO_Scrape"
    }

    fn supported_jurisdictions(&self) -> &[&str] {
        &["WO"]
    }

    async fn fetch(
        &self,
        patent: &CanonicalPatentId,
        _output_dir: &Path,
        config: &PatentConfig,
    ) -> FetchResult {
        let start = std::time::Instant::now();
        let base = base_url(config, "WIPO_Scrape", "https://patentscope.wipo.int");
        let source = self.source_name();

        // Convert number (e.g. "2024/123456" or "2024123456") to WO/YEAR/SERIAL
        let number = &patent.number;
        let wo_id = if number.contains('/') {
            format!("WO/{}", number)
        } else if number.len() >= 10 {
            let year = &number[..4];
            let serial = &number[4..];
            format!("WO/{}/{}", year, serial)
        } else {
            patent.canonical.clone()
        };

        let url = format!("{}/search/en/detail.jsf?docId={}", base, wo_id);
        let client = match Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs as u64))
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
        {
            Ok(c) => c,
            Err(e) => return fail_result(source, &format!("Client build error: {}", e)),
        };

        let resp = match client.get(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                let mut res = fail_result(source, &e.to_string());
                res.source_attempt.elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
                return res;
            }
        };

        if resp.status().as_u16() == 404 {
            let mut res = fail_result(source, "not_found");
            res.source_attempt.elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
            return res;
        }

        let body = match resp.text().await {
            Ok(b) => b,
            Err(e) => {
                let mut res = fail_result(source, &e.to_string());
                res.source_attempt.elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
                return res;
            }
        };

        let document = scraper::Html::parse_document(&body);
        let title = {
            let span_sel = scraper::Selector::parse("span#appTitleId").ok();
            let title_sel = scraper::Selector::parse("title").ok();
            span_sel
                .and_then(|s| document.select(&s).next())
                .or_else(|| title_sel.and_then(|s| document.select(&s).next()))
                .map(|el| el.text().collect::<String>().trim().to_string())
                .filter(|t| !t.is_empty())
        };

        let meta = PatentMetadata {
            canonical_id: patent.canonical.clone(),
            jurisdiction: "WO".into(),
            doc_type: "application".into(),
            title,
            abstract_text: None,
            inventors: vec![],
            assignee: None,
            filing_date: None,
            publication_date: None,
            grant_date: None,
            fetched_at: now_iso(),
            legal_status: None,
        };

        FetchResult {
            source_attempt: SourceAttempt {
                source: source.into(),
                success: true,
                elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
                error: None,
                metadata: None,
            },
            pdf_path: None,
            txt_path: None,
            metadata: Some(meta),
        }
    }
}

// ---------------------------------------------------------------------------
// CIPO (Canada, Google Patents fallback)
// ---------------------------------------------------------------------------

/// CIPO — scrape Canadian patent database (falls back to Google Patents).
pub struct CipoScrapeSource;

#[async_trait]
impl PatentSource for CipoScrapeSource {
    fn source_name(&self) -> &str {
        "CIPO"
    }

    fn supported_jurisdictions(&self) -> &[&str] {
        &["CA"]
    }

    async fn fetch(
        &self,
        patent: &CanonicalPatentId,
        _output_dir: &Path,
        config: &PatentConfig,
    ) -> FetchResult {
        let start = std::time::Instant::now();
        let base = base_url(config, "CIPO", "https://patents.google.com");
        let source = self.source_name();

        let url = format!("{}/patent/{}", base, patent.canonical);
        let client = match Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs as u64))
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
        {
            Ok(c) => c,
            Err(e) => return fail_result(source, &format!("Client build error: {}", e)),
        };

        let resp = match client.get(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                let mut res = fail_result(source, &e.to_string());
                res.source_attempt.elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
                return res;
            }
        };

        if resp.status().as_u16() == 404 {
            let mut res = fail_result(source, "not_found");
            res.source_attempt.elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
            return res;
        }

        let body = match resp.text().await {
            Ok(b) => b,
            Err(e) => {
                let mut res = fail_result(source, &e.to_string());
                res.source_attempt.elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
                return res;
            }
        };

        let document = scraper::Html::parse_document(&body);
        let title = {
            let h1_sel = scraper::Selector::parse("h1").ok();
            let title_sel = scraper::Selector::parse("title").ok();
            h1_sel
                .and_then(|s| document.select(&s).next())
                .or_else(|| title_sel.and_then(|s| document.select(&s).next()))
                .map(|el| el.text().collect::<String>().trim().to_string())
                .filter(|t| !t.is_empty())
        };

        let meta = PatentMetadata {
            canonical_id: patent.canonical.clone(),
            jurisdiction: "CA".into(),
            doc_type: patent.doc_type.clone(),
            title,
            abstract_text: None,
            inventors: vec![],
            assignee: None,
            filing_date: None,
            publication_date: None,
            grant_date: None,
            fetched_at: now_iso(),
            legal_status: None,
        };

        FetchResult {
            source_attempt: SourceAttempt {
                source: source.into(),
                success: true,
                elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
                error: None,
                metadata: None,
            },
            pdf_path: None,
            txt_path: None,
            metadata: Some(meta),
        }
    }
}

// ---------------------------------------------------------------------------
// IP Australia (REST JSON API)
// ---------------------------------------------------------------------------

/// IP Australia AusPat REST API.
pub struct IpAustraliaSource;

#[async_trait]
impl PatentSource for IpAustraliaSource {
    fn source_name(&self) -> &str {
        "IP_Australia"
    }

    fn supported_jurisdictions(&self) -> &[&str] {
        &["AU"]
    }

    async fn fetch(
        &self,
        patent: &CanonicalPatentId,
        _output_dir: &Path,
        config: &PatentConfig,
    ) -> FetchResult {
        let start = std::time::Instant::now();
        let base = base_url(
            config,
            "IP_Australia",
            "https://pericles.ipaustralia.gov.au",
        );
        let source = self.source_name();

        let url = format!("{}/ols/auspat/api/v1/applications/{}", base, patent.number);
        let client = match Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs as u64))
            .build()
        {
            Ok(c) => c,
            Err(e) => return fail_result(source, &format!("Client build error: {}", e)),
        };

        let resp = match client
            .get(&url)
            .header("Accept", "application/json")
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                let mut res = fail_result(source, &e.to_string());
                res.source_attempt.elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
                return res;
            }
        };

        if resp.status().as_u16() == 404 {
            let mut res = fail_result(source, "not_found");
            res.source_attempt.elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
            return res;
        }

        let data: serde_json::Value = match resp.json().await {
            Ok(d) => d,
            Err(e) => {
                let mut res = fail_result(source, &e.to_string());
                res.source_attempt.elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
                return res;
            }
        };

        let title = data.get("title").and_then(|v| v.as_str()).map(String::from);
        let inventors: Vec<String> = data
            .get("inventors")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|i| i.get("name").and_then(|n| n.as_str()).map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let assignee = data
            .get("applicant")
            .and_then(|v| v.as_str())
            .map(String::from);
        let filing_date = data
            .get("filingDate")
            .and_then(|v| v.as_str())
            .map(String::from);
        let publication_date = data
            .get("publicationDate")
            .and_then(|v| v.as_str())
            .map(String::from);
        let grant_date = data
            .get("grantDate")
            .and_then(|v| v.as_str())
            .map(String::from);

        let meta = PatentMetadata {
            canonical_id: patent.canonical.clone(),
            jurisdiction: "AU".into(),
            doc_type: patent.doc_type.clone(),
            title,
            abstract_text: None,
            inventors,
            assignee,
            filing_date,
            publication_date,
            grant_date,
            fetched_at: now_iso(),
            legal_status: None,
        };

        FetchResult {
            source_attempt: SourceAttempt {
                source: source.into(),
                success: true,
                elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
                error: None,
                metadata: None,
            },
            pdf_path: None,
            txt_path: None,
            metadata: Some(meta),
        }
    }
}

// ---------------------------------------------------------------------------
// USPTO PPUBS (session-based, HTTP/2)
// ---------------------------------------------------------------------------

/// USPTO PPUBS — US full-text patents, session cookie auth.
pub struct PpubsSource {
    pub session_cache: Arc<SessionCache>,
}

impl PpubsSource {
    /// Acquire or refresh the PPUBS session token.
    async fn get_session_token(&self, config: &PatentConfig) -> Option<String> {
        if let Some(cached) = self.session_cache.get("PPUBS") {
            return Some(cached);
        }
        let base = base_url(config, "USPTO", "https://ppubs.uspto.gov");
        let url = format!("{}/ppubs-api/v1/session", base);

        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .ok()?;

        match client.post(&url).json(&serde_json::json!({})).send().await {
            Ok(resp) => {
                if !resp.status().is_success() {
                    warn!("PPUBS session establishment failed: HTTP {}", resp.status());
                    return None;
                }
                let data: serde_json::Value = resp.json().await.ok()?;
                let token = data
                    .get("session")
                    .or_else(|| data.get("token"))
                    .or_else(|| data.get("accessToken"))
                    .and_then(|v| v.as_str())
                    .map(String::from);
                if let Some(ref t) = token {
                    self.session_cache.set("PPUBS", t, 30);
                }
                token
            }
            Err(e) => {
                warn!("PPUBS session establishment failed: {}", e);
                None
            }
        }
    }
}

#[async_trait]
impl PatentSource for PpubsSource {
    fn source_name(&self) -> &str {
        "USPTO"
    }

    fn supported_jurisdictions(&self) -> &[&str] {
        &["US"]
    }

    async fn fetch(
        &self,
        patent: &CanonicalPatentId,
        output_dir: &Path,
        config: &PatentConfig,
    ) -> FetchResult {
        let start = std::time::Instant::now();
        let base = base_url(config, "USPTO", "https://ppubs.uspto.gov");
        let source = self.source_name();

        let token = self.get_session_token(config).await;
        let client = match Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs as u64))
            .build()
        {
            Ok(c) => c,
            Err(e) => return fail_result(source, &format!("Client build error: {}", e)),
        };

        // Search for patent
        let search_url = format!("{}/ppubs-api/v1/patent", base);
        let mut req = client
            .get(&search_url)
            .query(&[("patentNumber", &patent.number)]);
        if let Some(ref t) = token {
            req = req.header("Authorization", format!("Bearer {}", t));
        }

        let resp = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                let mut res = fail_result(source, &e.to_string());
                res.source_attempt.elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
                return res;
            }
        };

        if resp.status().as_u16() == 404 {
            let mut res = fail_result(source, "not_found");
            res.source_attempt.elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
            return res;
        }

        let data: serde_json::Value = match resp.json().await {
            Ok(d) => d,
            Err(e) => {
                let mut res = fail_result(source, &e.to_string());
                res.source_attempt.elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
                return res;
            }
        };

        // Extract first patent result
        let patents = data
            .get("patents")
            .or_else(|| data.get("results"))
            .and_then(|v| v.as_array());
        let doc = match patents.and_then(|arr| arr.first()) {
            Some(d) => d,
            None => {
                let mut res = fail_result(source, "not_found");
                res.source_attempt.elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
                return res;
            }
        };

        let guid = doc
            .get("guid")
            .or_else(|| doc.get("documentId"))
            .and_then(|v| v.as_str())
            .map(String::from);
        let txt_content = doc
            .get("fullText")
            .or_else(|| doc.get("claims"))
            .or_else(|| doc.get("abstract"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let meta = PatentMetadata {
            canonical_id: doc
                .get("patentNumber")
                .and_then(|v| v.as_str())
                .unwrap_or(&patent.canonical)
                .to_string(),
            jurisdiction: "US".into(),
            doc_type: "patent".into(),
            title: doc.get("title").and_then(|v| v.as_str()).map(String::from),
            abstract_text: doc
                .get("abstract")
                .and_then(|v| v.as_str())
                .map(String::from),
            inventors: doc
                .get("inventors")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|i| i.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            assignee: doc
                .get("assignee")
                .and_then(|v| v.as_str())
                .map(String::from),
            filing_date: doc
                .get("filingDate")
                .and_then(|v| v.as_str())
                .map(String::from),
            publication_date: doc
                .get("publicationDate")
                .and_then(|v| v.as_str())
                .map(String::from),
            grant_date: doc
                .get("grantDate")
                .and_then(|v| v.as_str())
                .map(String::from),
            fetched_at: now_iso(),
            legal_status: None,
        };

        // Write txt if we got content
        let mut txt_path: Option<PathBuf> = None;
        if !txt_content.is_empty() {
            let _ = std::fs::create_dir_all(output_dir);
            let p = output_dir.join(format!("{}.txt", patent.canonical));
            if std::fs::write(&p, &txt_content).is_ok() {
                txt_path = Some(p);
            }
        }

        // Attempt PDF download if we have a guid
        let mut pdf_path: Option<PathBuf> = None;
        if let Some(ref g) = guid {
            let pdf_url = format!("{}/ppubs-api/v1/download/{}", base, g);
            let mut pdf_req = client.get(&pdf_url);
            if let Some(ref t) = token {
                pdf_req = pdf_req.header("Authorization", format!("Bearer {}", t));
            }
            if let Ok(pdf_resp) = pdf_req.send().await {
                if pdf_resp.status().is_success() {
                    let _ = std::fs::create_dir_all(output_dir);
                    let p = output_dir.join(format!("{}.pdf", patent.canonical));
                    if let Ok(bytes) = pdf_resp.bytes().await {
                        if std::fs::write(&p, &bytes).is_ok() {
                            pdf_path = Some(p);
                        }
                    }
                }
            }
        }

        FetchResult {
            source_attempt: SourceAttempt {
                source: source.into(),
                success: true,
                elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
                error: None,
                metadata: None,
            },
            pdf_path,
            txt_path,
            metadata: Some(meta),
        }
    }
}

// ---------------------------------------------------------------------------
// EPO OPS (OAuth2, XML parsing)
// ---------------------------------------------------------------------------

/// EPO Open Patent Services — bibliographic data + PDF for 100+ offices.
pub struct EpoOpsSource {
    pub session_cache: Arc<SessionCache>,
}

impl EpoOpsSource {
    /// Acquire or refresh the EPO OPS OAuth2 token.
    async fn get_token(&self, config: &PatentConfig) -> Option<String> {
        let (client_id, client_secret) = match (&config.epo_client_id, &config.epo_client_secret) {
            (Some(id), Some(secret)) => (id.clone(), secret.clone()),
            _ => return None,
        };

        if let Some(cached) = self.session_cache.get("EPO_OPS") {
            return Some(cached);
        }

        let base = base_url(config, "EPO_OPS", "https://ops.epo.org");
        let url = format!("{}/3.2/auth/accesstoken", base);

        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .ok()?;

        let form_data = [
            ("grant_type", "client_credentials".to_string()),
            ("client_id", client_id),
            ("client_secret", client_secret),
        ];

        let resp = match client.post(&url).form(&form_data).send().await {
            Ok(r) => r,
            Err(e) => {
                warn!("EPO OPS token request failed: {}", e);
                return None;
            }
        };

        if !resp.status().is_success() {
            warn!("EPO OPS auth failed: HTTP {}", resp.status());
            return None;
        }

        let data: serde_json::Value = match resp.json().await {
            Ok(d) => d,
            Err(_) => return None,
        };

        let token = data
            .get("access_token")
            .and_then(|v| v.as_str())
            .map(String::from);
        let expires_in = data
            .get("expires_in")
            .and_then(|v| v.as_i64())
            .unwrap_or(1800);
        if let Some(ref t) = token {
            let expires_at = Utc::now() + chrono::Duration::seconds(expires_in);
            self.session_cache
                .set_with_expiry("EPO_OPS", t, expires_at);
        }
        token
    }

    /// Parse EPO OPS biblio XML response into PatentMetadata.
    fn parse_biblio_xml(
        &self,
        xml_text: &str,
        patent: &CanonicalPatentId,
    ) -> Option<PatentMetadata> {
        use quick_xml::events::Event;
        use quick_xml::Reader;

        let mut reader = Reader::from_str(xml_text);

        let mut title: Option<String> = None;
        let mut inventors: Vec<String> = Vec::new();
        let mut assignee: Option<String> = None;
        let mut filing_date: Option<String> = None;
        let mut publication_date: Option<String> = None;
        let mut grant_date: Option<String> = None;

        // State tracking for XML parsing
        let mut in_invention_title = false;
        let mut is_english_title = false;
        let mut in_inventor_name = false;
        let mut in_applicant_name = false;
        let mut in_filing_date = false;
        let mut in_publication_date = false;
        let mut in_grant_date = false;
        let mut depth_inventor = 0u32;
        let mut depth_applicant = 0u32;

        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                    let local_name = e.local_name();
                    let local_name_str = std::str::from_utf8(local_name.as_ref()).unwrap_or("");
                    match local_name_str {
                        "invention-title" => {
                            in_invention_title = true;
                            // Check for lang="en"
                            is_english_title = e.attributes().flatten().any(|a| {
                                std::str::from_utf8(a.key.as_ref()).unwrap_or("") == "lang"
                                    && std::str::from_utf8(&a.value).unwrap_or("") == "en"
                            });
                        }
                        "inventor" => {
                            depth_inventor += 1;
                        }
                        "applicant" => {
                            depth_applicant += 1;
                        }
                        "name" => {
                            if depth_inventor > 0 {
                                in_inventor_name = true;
                            } else if depth_applicant > 0 {
                                in_applicant_name = true;
                            }
                        }
                        "filing-date" => {
                            in_filing_date = true;
                        }
                        "date-of-publication" => {
                            in_publication_date = true;
                        }
                        "date-of-grant" => {
                            in_grant_date = true;
                        }
                        _ => {}
                    }
                }
                Ok(Event::Text(ref e)) => {
                    let text = e.unescape().unwrap_or_default().trim().to_string();
                    if !text.is_empty() {
                        if in_invention_title
                            && (is_english_title || title.is_none())
                        {
                            title = Some(text.clone());
                        }
                        if in_inventor_name {
                            inventors.push(text.clone());
                            in_inventor_name = false;
                        }
                        if in_applicant_name && assignee.is_none() {
                            assignee = Some(text.clone());
                            in_applicant_name = false;
                        }
                        if in_filing_date {
                            filing_date = Some(text.clone());
                            in_filing_date = false;
                        }
                        if in_publication_date {
                            publication_date = Some(text.clone());
                            in_publication_date = false;
                        }
                        if in_grant_date {
                            grant_date = Some(text.clone());
                            in_grant_date = false;
                        }
                    }
                }
                Ok(Event::End(ref e)) => {
                    let local_name = e.local_name();
                    let local_name_str = std::str::from_utf8(local_name.as_ref()).unwrap_or("");
                    match local_name_str {
                        "invention-title" => {
                            in_invention_title = false;
                            is_english_title = false;
                        }
                        "inventor" => {
                            depth_inventor = depth_inventor.saturating_sub(1);
                        }
                        "applicant" => {
                            depth_applicant = depth_applicant.saturating_sub(1);
                        }
                        "name" => {
                            in_inventor_name = false;
                            in_applicant_name = false;
                        }
                        "filing-date" => in_filing_date = false,
                        "date-of-publication" => in_publication_date = false,
                        "date-of-grant" => in_grant_date = false,
                        _ => {}
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => {
                    debug!("EPO OPS XML parse error: {}", e);
                    return None;
                }
                _ => {}
            }
            buf.clear();
        }

        Some(PatentMetadata {
            canonical_id: patent.canonical.clone(),
            jurisdiction: patent.jurisdiction.clone(),
            doc_type: patent.doc_type.clone(),
            title,
            abstract_text: None,
            inventors,
            assignee,
            filing_date,
            publication_date,
            grant_date,
            fetched_at: now_iso(),
            legal_status: None,
        })
    }
}

#[async_trait]
impl PatentSource for EpoOpsSource {
    fn source_name(&self) -> &str {
        "EPO_OPS"
    }

    fn supported_jurisdictions(&self) -> &[&str] {
        &[] // supports all
    }

    async fn fetch(
        &self,
        patent: &CanonicalPatentId,
        output_dir: &Path,
        config: &PatentConfig,
    ) -> FetchResult {
        let start = std::time::Instant::now();
        let base = base_url(config, "EPO_OPS", "https://ops.epo.org");
        let source = self.source_name();

        let token = self.get_token(config).await;
        let client = match Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs as u64))
            .build()
        {
            Ok(c) => c,
            Err(e) => return fail_result(source, &format!("Client build error: {}", e)),
        };

        let mut headers = reqwest::header::HeaderMap::new();
        if let Some(ref t) = token {
            if let Ok(val) = reqwest::header::HeaderValue::from_str(&format!("Bearer {}", t)) {
                headers.insert(reqwest::header::AUTHORIZATION, val);
            }
        }

        // Fetch biblio data
        let pub_id = format!("{}.{}", patent.jurisdiction, patent.number);
        let biblio_url = format!(
            "{}/3.2/rest-services/published-data/publication/epodoc/{}/biblio",
            base, pub_id
        );

        let resp = match client
            .get(&biblio_url)
            .headers(headers.clone())
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                let mut res = fail_result(source, &e.to_string());
                res.source_attempt.elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
                return res;
            }
        };

        if resp.status().as_u16() == 404 {
            let mut res = fail_result(source, "not_found");
            res.source_attempt.elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
            return res;
        }

        if resp.status().as_u16() == 401 {
            let mut res = fail_result(source, "EPO OPS auth failed");
            res.source_attempt.elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
            return res;
        }

        let xml_text = match resp.text().await {
            Ok(t) => t,
            Err(e) => {
                let mut res = fail_result(source, &e.to_string());
                res.source_attempt.elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
                return res;
            }
        };

        let meta = match self.parse_biblio_xml(&xml_text, patent) {
            Some(m) => m,
            None => {
                let mut res = fail_result(source, "Failed to parse EPO OPS biblio XML");
                res.source_attempt.elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
                return res;
            }
        };

        // Attempt PDF download
        let mut pdf_path: Option<PathBuf> = None;
        let pdf_url = format!(
            "{}/3.2/rest-services/published-data/publication/epodoc/{}/full-cycle",
            base, pub_id
        );
        let mut pdf_headers = headers;
        pdf_headers.insert(
            reqwest::header::ACCEPT,
            reqwest::header::HeaderValue::from_static("application/pdf"),
        );
        if let Ok(pdf_resp) = client.get(&pdf_url).headers(pdf_headers).send().await {
            if pdf_resp.status().is_success() {
                let content_type = pdf_resp
                    .headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                if content_type.starts_with("application/pdf") {
                    let _ = std::fs::create_dir_all(output_dir);
                    let p = output_dir.join(format!("{}.pdf", patent.canonical));
                    if let Ok(bytes) = pdf_resp.bytes().await {
                        if std::fs::write(&p, &bytes).is_ok() {
                            pdf_path = Some(p);
                        }
                    }
                }
            }
        }

        FetchResult {
            source_attempt: SourceAttempt {
                source: source.into(),
                success: true,
                elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
                error: None,
                metadata: None,
            },
            pdf_path,
            txt_path: None,
            metadata: Some(meta),
        }
    }
}

// ---------------------------------------------------------------------------
// Google Patents (browser — deferred to B07)
// ---------------------------------------------------------------------------

/// Google Patents — browser source stub (will be implemented in B07).
pub struct GooglePatentsSource;

#[async_trait]
impl PatentSource for GooglePatentsSource {
    fn source_name(&self) -> &str {
        "Google_Patents"
    }

    fn supported_jurisdictions(&self) -> &[&str] {
        &[] // supports all
    }

    async fn fetch(
        &self,
        _patent: &CanonicalPatentId,
        _output_dir: &Path,
        _config: &PatentConfig,
    ) -> FetchResult {
        FetchResult {
            source_attempt: SourceAttempt {
                source: "Google_Patents".into(),
                success: false,
                elapsed_ms: 0.0,
                error: Some(
                    "Browser source not yet implemented; will be added in B07".into(),
                ),
                metadata: None,
            },
            pdf_path: None,
            txt_path: None,
            metadata: None,
        }
    }
}

// ---------------------------------------------------------------------------
// BigQuery (Python subprocess fallback)
// ---------------------------------------------------------------------------

/// Google BigQuery patents-public-data — optional; degrades gracefully.
pub struct BigQuerySource;

#[async_trait]
impl PatentSource for BigQuerySource {
    fn source_name(&self) -> &str {
        "BigQuery"
    }

    fn supported_jurisdictions(&self) -> &[&str] {
        &[] // supports all
    }

    async fn fetch(
        &self,
        patent: &CanonicalPatentId,
        _output_dir: &Path,
        config: &PatentConfig,
    ) -> FetchResult {
        let start = std::time::Instant::now();
        let source = self.source_name();

        // Check if BigQuery project is configured
        let _project = match &config.bigquery_project {
            Some(p) if !p.is_empty() => p.clone(),
            _ => {
                let mut res = fail_result(source, "BigQuery not configured: no project");
                res.source_attempt.elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
                return res;
            }
        };

        // Build the query
        let pub_number = format!("{}-{}", patent.jurisdiction, patent.number);
        let query = format!(
            r#"SELECT
  publication_number,
  title_localized,
  abstract_localized,
  inventor_harmonized,
  assignee_harmonized,
  filing_date,
  publication_date,
  grant_date
FROM `patents-public-data.patents.publications`
WHERE publication_number LIKE '{}%'
LIMIT 5"#,
            pub_number
        );

        // Build inline Python script
        let script = format!(
            r#"
import json, sys
try:
    from google.cloud import bigquery
    client = bigquery.Client(project="{project}")
    rows = list(client.query("""{query}""").result())
    if not rows:
        print(json.dumps({{"error": "not_found"}}))
        sys.exit(0)
    row = dict(rows[0])
    # Convert non-serializable types
    for k, v in row.items():
        if hasattr(v, 'isoformat'):
            row[k] = v.isoformat()
        elif isinstance(v, (list, tuple)):
            row[k] = [dict(i) if hasattr(i, 'items') else i for i in v]
    print(json.dumps(row))
except ImportError:
    print(json.dumps({{"error": "google-cloud-bigquery not installed"}}))
except Exception as e:
    print(json.dumps({{"error": str(e)}}))
"#,
            project = _project,
            query = query
        );

        // Run Python subprocess
        let result = tokio::task::spawn_blocking(move || {
            std::process::Command::new("python3")
                .args(["-c", &script])
                .output()
        })
        .await;

        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let json_line = stdout
                    .lines()
                    .rfind(|l| l.trim_start().starts_with('{'))
                    .unwrap_or("");

                match serde_json::from_str::<serde_json::Value>(json_line) {
                    Ok(data) => {
                        if let Some(err) = data.get("error").and_then(|v| v.as_str()) {
                            let mut res = fail_result(source, err);
                            res.source_attempt.elapsed_ms =
                                start.elapsed().as_secs_f64() * 1000.0;
                            return res;
                        }

                        // Parse the result into metadata
                        let title = data
                            .get("title_localized")
                            .and_then(|v| v.as_array())
                            .and_then(|arr| {
                                arr.iter()
                                    .find(|t| {
                                        t.get("language")
                                            .and_then(|l| l.as_str())
                                            .map(|l| l == "en")
                                            .unwrap_or(false)
                                    })
                                    .or_else(|| arr.first())
                            })
                            .and_then(|t| t.get("text").and_then(|v| v.as_str()))
                            .map(String::from);

                        let abstract_text = data
                            .get("abstract_localized")
                            .and_then(|v| v.as_array())
                            .and_then(|arr| {
                                arr.iter().find(|a| {
                                    a.get("language")
                                        .and_then(|l| l.as_str())
                                        .map(|l| l == "en")
                                        .unwrap_or(false)
                                })
                            })
                            .and_then(|a| a.get("text").and_then(|v| v.as_str()))
                            .map(String::from);

                        let inventors: Vec<String> = data
                            .get("inventor_harmonized")
                            .and_then(|v| v.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|i| {
                                        i.get("name").and_then(|n| n.as_str()).map(String::from)
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();

                        let assignee = data
                            .get("assignee_harmonized")
                            .and_then(|v| v.as_array())
                            .and_then(|arr| arr.first())
                            .and_then(|a| a.get("name").and_then(|n| n.as_str()))
                            .map(String::from);

                        fn parse_bq_date(v: &serde_json::Value) -> Option<String> {
                            let s = v.as_str().or_else(|| {
                                v.as_i64().map(|_| "")  // fallback
                            })?;
                            if s.is_empty() {
                                return v.as_i64().map(|n| {
                                    let ns = n.to_string();
                                    if ns.len() == 8 {
                                        format!("{}-{}-{}", &ns[..4], &ns[4..6], &ns[6..])
                                    } else {
                                        ns
                                    }
                                });
                            }
                            if s.len() == 8 && s.chars().all(|c| c.is_ascii_digit()) {
                                Some(format!("{}-{}-{}", &s[..4], &s[4..6], &s[6..]))
                            } else {
                                Some(s.to_string())
                            }
                        }

                        let filing_date =
                            data.get("filing_date").and_then(parse_bq_date);
                        let publication_date =
                            data.get("publication_date").and_then(parse_bq_date);
                        let grant_date =
                            data.get("grant_date").and_then(parse_bq_date);

                        let meta = PatentMetadata {
                            canonical_id: patent.canonical.clone(),
                            jurisdiction: patent.jurisdiction.clone(),
                            doc_type: patent.doc_type.clone(),
                            title,
                            abstract_text,
                            inventors,
                            assignee,
                            filing_date,
                            publication_date,
                            grant_date,
                            fetched_at: now_iso(),
                            legal_status: None,
                        };

                        FetchResult {
                            source_attempt: SourceAttempt {
                                source: source.into(),
                                success: true,
                                elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
                                error: None,
                                metadata: None,
                            },
                            pdf_path: None,
                            txt_path: None,
                            metadata: Some(meta),
                        }
                    }
                    Err(e) => {
                        let mut res =
                            fail_result(source, &format!("Failed to parse BigQuery output: {}", e));
                        res.source_attempt.elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
                        res
                    }
                }
            }
            Ok(Err(e)) => {
                let mut res =
                    fail_result(source, &format!("Python subprocess error: {}", e));
                res.source_attempt.elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
                res
            }
            Err(e) => {
                let mut res = fail_result(source, &format!("Spawn error: {}", e));
                res.source_attempt.elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
                res
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn test_config() -> PatentConfig {
        PatentConfig {
            cache_local_dir: std::path::PathBuf::from("/tmp/patent-test"),
            cache_global_db: std::path::PathBuf::from("/tmp/patent-test/global.db"),
            source_priority: vec![],
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
        }
    }

    #[test]
    fn test_patents_view_stub_returns_error() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let source = PatentsViewStubSource;
            let patent = crate::id_canon::canonicalize("US7654321");
            let config = test_config();
            let result = source.fetch(&patent, Path::new("/tmp"), &config).await;
            assert!(!result.source_attempt.success);
            assert!(result
                .source_attempt
                .error
                .as_ref()
                .unwrap()
                .contains("shut down"));
        });
    }

    #[test]
    fn test_can_fetch_jurisdiction_filtering() {
        let source = PatentsViewStubSource;
        let us_patent = crate::id_canon::canonicalize("US7654321");
        let ep_patent = crate::id_canon::canonicalize("EP1234567");
        assert!(source.can_fetch(&us_patent));
        assert!(!source.can_fetch(&ep_patent));
    }

    #[test]
    fn test_espacenet_can_fetch_all_jurisdictions() {
        let source = EspacenetSource;
        let us_patent = crate::id_canon::canonicalize("US7654321");
        let ep_patent = crate::id_canon::canonicalize("EP1234567");
        assert!(source.can_fetch(&us_patent));
        assert!(source.can_fetch(&ep_patent));
    }

    #[test]
    fn test_wipo_only_handles_wo() {
        let source = WipoScrapeSource;
        let wo_patent = crate::id_canon::canonicalize("WO2024123456");
        let us_patent = crate::id_canon::canonicalize("US7654321");
        assert!(source.can_fetch(&wo_patent));
        assert!(!source.can_fetch(&us_patent));
    }

    #[test]
    fn test_ip_australia_only_handles_au() {
        let source = IpAustraliaSource;
        let au_patent = crate::id_canon::canonicalize("AU2023123456");
        let us_patent = crate::id_canon::canonicalize("US7654321");
        assert!(source.can_fetch(&au_patent));
        assert!(!source.can_fetch(&us_patent));
    }

    #[test]
    fn test_cipo_only_handles_ca() {
        let source = CipoScrapeSource;
        let ca_patent = crate::id_canon::canonicalize("CA1234567");
        let us_patent = crate::id_canon::canonicalize("US7654321");
        assert!(source.can_fetch(&ca_patent));
        assert!(!source.can_fetch(&us_patent));
    }

    #[test]
    fn test_epo_ops_can_fetch_all() {
        let source = EpoOpsSource {
            session_cache: Arc::new(SessionCache::new()),
        };
        let us_patent = crate::id_canon::canonicalize("US7654321");
        assert!(source.can_fetch(&us_patent));
    }

    #[test]
    fn test_ppubs_only_handles_us() {
        let source = PpubsSource {
            session_cache: Arc::new(SessionCache::new()),
        };
        let us_patent = crate::id_canon::canonicalize("US7654321");
        let ep_patent = crate::id_canon::canonicalize("EP1234567");
        assert!(source.can_fetch(&us_patent));
        assert!(!source.can_fetch(&ep_patent));
    }

    #[test]
    fn test_google_patents_stub() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let source = GooglePatentsSource;
            let patent = crate::id_canon::canonicalize("US7654321");
            let config = test_config();
            let result = source.fetch(&patent, Path::new("/tmp"), &config).await;
            assert!(!result.source_attempt.success);
        });
    }

    #[test]
    fn test_bigquery_no_project_returns_error() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let source = BigQuerySource;
            let patent = crate::id_canon::canonicalize("US7654321");
            let config = test_config();
            let result = source.fetch(&patent, Path::new("/tmp"), &config).await;
            assert!(!result.source_attempt.success);
            assert!(result
                .source_attempt
                .error
                .as_ref()
                .unwrap()
                .contains("not configured"));
        });
    }

    #[test]
    fn test_now_iso_format() {
        let ts = now_iso();
        // Should be RFC3339 formatted
        assert!(ts.contains("T"));
        assert!(ts.len() > 10);
    }
}
