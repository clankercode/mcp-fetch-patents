use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::Client;
use tracing::warn;

use crate::cache::{PatentMetadata, SessionCache};
use crate::fetchers::PatentSource;
use crate::id_canon::CanonicalPatentId;

use super::{base_url, fail_result, now_iso, FetchResult};

pub struct PpubsSource {
    pub session_cache: Arc<SessionCache>,
    pub client: Arc<Client>,
}

impl PpubsSource {
    async fn get_session_token(&self, config: &crate::config::PatentConfig) -> Option<String> {
        if let Some(cached) = self.session_cache.get("PPUBS") {
            return Some(cached);
        }
        let base = base_url(config, "USPTO", "https://ppubs.uspto.gov");
        let url = format!("{}/ppubs-api/v1/session", base);

        match self
            .client
            .post(&url)
            .timeout(Duration::from_secs(30))
            .json(&serde_json::json!({}))
            .send()
            .await
        {
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
        config: &crate::config::PatentConfig,
    ) -> FetchResult {
        let start = std::time::Instant::now();
        let base = base_url(config, "USPTO", "https://ppubs.uspto.gov");
        let source = self.source_name();

        let token = self.get_session_token(config).await;
        let client = self.client.clone();

        let search_url = format!("{}/ppubs-api/v1/patent", base);
        let mut req = client
            .get(&search_url)
            .header("Accept-Language", "en")
            .query(&[("patentNumber", &patent.number)]);
        if let Some(ref t) = token {
            req = req.header("Authorization", format!("Bearer {}", t));
        }

        let resp = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                let mut res = fail_result(source, &e.to_string());
                res.source_attempt.elapsed_ms = crate::elapsed_ms(start);
                return res;
            }
        };

        if resp.status().as_u16() == 404 {
            let mut res = fail_result(source, "not_found");
            res.source_attempt.elapsed_ms = crate::elapsed_ms(start);
            return res;
        }

        let data: serde_json::Value = match resp.json().await {
            Ok(d) => d,
            Err(e) => {
                let mut res = fail_result(source, &e.to_string());
                res.source_attempt.elapsed_ms = crate::elapsed_ms(start);
                return res;
            }
        };

        let patents = data
            .get("patents")
            .or_else(|| data.get("results"))
            .and_then(|v| v.as_array());
        let doc = match patents.and_then(|arr| arr.first()) {
            Some(d) => d,
            None => {
                let mut res = fail_result(source, "not_found");
                res.source_attempt.elapsed_ms = crate::elapsed_ms(start);
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

        let mut txt_path: Option<PathBuf> = None;
        if !txt_content.is_empty() {
            let _ = std::fs::create_dir_all(output_dir);
            let p = output_dir.join(format!("{}.txt", patent.canonical));
            if std::fs::write(&p, &txt_content).is_ok() {
                txt_path = Some(p);
            }
        }

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
            source_attempt: crate::cache::SourceAttempt {
                source: source.into(),
                success: true,
                elapsed_ms: crate::elapsed_ms(start),
                error: None,
                metadata: None,
            },
            pdf_path,
            txt_path,
            metadata: Some(meta),
        }
    }
}
