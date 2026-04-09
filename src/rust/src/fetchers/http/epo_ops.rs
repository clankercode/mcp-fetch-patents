use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use reqwest::Client;
use tracing::{debug, warn};

use crate::cache::{PatentMetadata, SessionCache};
use crate::fetchers::PatentSource;
use crate::id_canon::CanonicalPatentId;

use super::{base_url, fail_result, metadata_has_useful_fields, now_iso, FetchResult};

pub struct EpoOpsSource {
    pub session_cache: Arc<SessionCache>,
    pub client: Arc<Client>,
}

impl EpoOpsSource {
    async fn get_token(&self, config: &crate::config::PatentConfig) -> Option<String> {
        let (client_id, client_secret) = match (&config.epo_client_id, &config.epo_client_secret) {
            (Some(id), Some(secret)) => (id.clone(), secret.clone()),
            _ => return None,
        };

        if let Some(cached) = self.session_cache.get("EPO_OPS") {
            return Some(cached);
        }

        let base = base_url(config, "EPO_OPS", "https://ops.epo.org");
        let url = format!("{}/3.2/auth/accesstoken", base);

        let form_data = [
            ("grant_type", "client_credentials".to_string()),
            ("client_id", client_id),
            ("client_secret", client_secret),
        ];

        let resp = match self
            .client
            .post(&url)
            .timeout(Duration::from_secs(30))
            .form(&form_data)
            .send()
            .await
        {
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
            self.session_cache.set_with_expiry("EPO_OPS", t, expires_at);
        }
        token
    }

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
                    let text = e.decode().unwrap_or_default().trim().to_string();
                    if !text.is_empty() {
                        if in_invention_title && (is_english_title || title.is_none()) {
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
        &[]
    }

    async fn fetch(
        &self,
        patent: &CanonicalPatentId,
        output_dir: &Path,
        config: &crate::config::PatentConfig,
    ) -> FetchResult {
        let start = std::time::Instant::now();
        let base = base_url(config, "EPO_OPS", "https://ops.epo.org");
        let source = self.source_name();

        let token = self.get_token(config).await;
        let client = self.client.clone();

        let mut headers = reqwest::header::HeaderMap::new();
        if let Some(ref t) = token {
            if let Ok(val) = reqwest::header::HeaderValue::from_str(&format!("Bearer {}", t)) {
                headers.insert(reqwest::header::AUTHORIZATION, val);
            }
        }

        let pub_id = format!("{}.{}", patent.jurisdiction, patent.number);
        let biblio_url = format!(
            "{}/3.2/rest-services/published-data/publication/epodoc/{}/biblio",
            base, pub_id
        );

        let resp = match client
            .get(&biblio_url)
            .header("Accept-Language", "en")
            .headers(headers.clone())
            .send()
            .await
        {
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

        if resp.status().as_u16() == 401 {
            let mut res = fail_result(source, "EPO OPS auth failed");
            res.source_attempt.elapsed_ms = crate::elapsed_ms(start);
            return res;
        }

        let xml_text = match resp.text().await {
            Ok(t) => t,
            Err(e) => {
                let mut res = fail_result(source, &e.to_string());
                res.source_attempt.elapsed_ms = crate::elapsed_ms(start);
                return res;
            }
        };

        let meta = match self.parse_biblio_xml(&xml_text, patent) {
            Some(m) => m,
            None => {
                let mut res = fail_result(source, "Failed to parse EPO OPS biblio XML");
                res.source_attempt.elapsed_ms = crate::elapsed_ms(start);
                return res;
            }
        };

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

        if pdf_path.is_none() && !metadata_has_useful_fields(&meta) {
            let mut res = fail_result(source, "EPO OPS returned no usable metadata");
            res.source_attempt.elapsed_ms = crate::elapsed_ms(start);
            return res;
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
            txt_path: None,
            metadata: Some(meta),
        }
    }
}
