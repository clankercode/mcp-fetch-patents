use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use reqwest::Client;

use crate::cache::PatentMetadata;
use crate::fetchers::PatentSource;
use crate::id_canon::CanonicalPatentId;

use super::{base_url, fail_result, now_iso, FetchResult};

pub struct IpAustraliaSource {
    pub client: Arc<Client>,
}

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
        config: &crate::config::PatentConfig,
    ) -> FetchResult {
        let start = std::time::Instant::now();
        let base = base_url(
            config,
            "IP_Australia",
            "https://pericles.ipaustralia.gov.au",
        );
        let source = self.source_name();

        let url = format!("{}/ols/auspat/api/v1/applications/{}", base, patent.number);
        let client = self.client.clone();

        let resp = match client
            .get(&url)
            .header("Accept", "application/json")
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

        let data: serde_json::Value = match resp.json().await {
            Ok(d) => d,
            Err(e) => {
                let mut res = fail_result(source, &e.to_string());
                res.source_attempt.elapsed_ms = crate::elapsed_ms(start);
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
            source_attempt: crate::cache::SourceAttempt {
                source: source.into(),
                success: true,
                elapsed_ms: crate::elapsed_ms(start),
                error: None,
                metadata: None,
            },
            pdf_path: None,
            txt_path: None,
            metadata: Some(meta),
        }
    }
}
