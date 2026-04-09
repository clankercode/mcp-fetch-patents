use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use reqwest::Client;

use crate::cache::PatentMetadata;
use crate::fetchers::PatentSource;
use crate::id_canon::CanonicalPatentId;

use super::{base_url, fail_result, metadata_has_useful_fields, now_iso, FetchResult};

pub struct EspacenetSource {
    pub client: Arc<Client>,
}

#[async_trait]
impl PatentSource for EspacenetSource {
    fn source_name(&self) -> &str {
        "Espacenet"
    }

    fn supported_jurisdictions(&self) -> &[&str] {
        &[]
    }

    async fn fetch(
        &self,
        patent: &CanonicalPatentId,
        _output_dir: &Path,
        config: &crate::config::PatentConfig,
    ) -> FetchResult {
        let start = std::time::Instant::now();
        let base = base_url(config, "Espacenet", "https://worldwide.espacenet.com");
        let source = self.source_name();

        let url = format!("{}/patent/{}", base, patent.canonical);
        let client = self.client.clone();

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

        let document = scraper::Html::parse_document(&body);

        let title = {
            let h2_sel = scraper::Selector::parse("h2.title").ok();
            let h1_sel = scraper::Selector::parse("h1").ok();
            h2_sel
                .and_then(|s| document.select(&s).next())
                .or_else(|| h1_sel.and_then(|s| document.select(&s).next()))
                .map(|el| el.text().collect::<String>().trim().to_string())
                .filter(|t| !t.is_empty())
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

        if !metadata_has_useful_fields(&meta) {
            let mut res = fail_result(source, "Espacenet returned no usable metadata");
            res.source_attempt.elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
            return res;
        }

        FetchResult {
            source_attempt: crate::cache::SourceAttempt {
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
