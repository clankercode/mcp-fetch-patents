use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use reqwest::Client;

use crate::cache::PatentMetadata;
use crate::fetchers::PatentSource;
use crate::id_canon::CanonicalPatentId;

use super::{base_url, fail_result, now_iso, FetchResult};

pub struct WipoScrapeSource {
    pub client: Arc<Client>,
}

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
        config: &crate::config::PatentConfig,
    ) -> FetchResult {
        let start = std::time::Instant::now();
        let base = base_url(config, "WIPO_Scrape", "https://patentscope.wipo.int");
        let source = self.source_name();

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
        let client = self.client.clone();

        let resp = match client.get(&url).send().await {
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

        let body = match resp.text().await {
            Ok(b) => b,
            Err(e) => {
                let mut res = fail_result(source, &e.to_string());
                res.source_attempt.elapsed_ms = crate::elapsed_ms(start);
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
