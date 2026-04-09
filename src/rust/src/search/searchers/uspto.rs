use std::time::Duration;

use reqwest::Client;
use tracing::warn;

use crate::ranking::PatentHit;

pub struct UsptoTextSearchBackend {
    pub base_url: String,
    pub client: Client,
}

impl UsptoTextSearchBackend {
    pub fn new(base_url: Option<String>, timeout: Duration, client: Option<Client>) -> Self {
        Self {
            base_url: base_url
                .unwrap_or_else(|| "https://ppubs.uspto.gov/ppubs-api/v1".to_string()),
            client: client.unwrap_or_else(|| {
                Client::builder()
                    .timeout(timeout)
                    .build()
                    .unwrap_or_else(|_| Client::new())
            }),
        }
    }

    #[tracing::instrument(skip_all)]
    pub async fn search(
        &self,
        query: &str,
        date_from: Option<&str>,
        date_to: Option<&str>,
        max_results: usize,
    ) -> anyhow::Result<Vec<PatentHit>> {
        let mut body = serde_json::json!({
            "query": query,
            "sources": ["US-PGPUB", "USPAT"],
            "hits": max_results,
            "start": 0,
        });

        if date_from.is_some() || date_to.is_some() {
            body["dateRangeField"] = serde_json::json!("applicationDate");
        }
        if let Some(df) = date_from {
            body["startDate"] = serde_json::json!(df);
        }
        if let Some(dt) = date_to {
            body["endDate"] = serde_json::json!(dt);
        }

        let url = format!("{}/query", self.base_url.trim_end_matches('/'));

        let resp = match self.client.post(&url).json(&body).send().await {
            Ok(r) => r,
            Err(e) => {
                warn!("USPTO PPUBS text search request failed: {}", e);
                return Ok(vec![]);
            }
        };

        if !resp.status().is_success() {
            warn!(
                "USPTO PPUBS text search HTTP error {}",
                resp.status().as_u16()
            );
            return Ok(vec![]);
        }

        let data: serde_json::Value = match resp.json().await {
            Ok(d) => d,
            Err(e) => {
                warn!("USPTO PPUBS JSON parse error: {}", e);
                return Ok(vec![]);
            }
        };

        let patents = data
            .get("patents")
            .or_else(|| data.get("results"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut hits = Vec::new();
        for doc in &patents {
            if let Some(hit) = Self::map_doc(doc) {
                hits.push(hit);
            }
        }
        Ok(hits)
    }

    pub fn map_doc(doc: &serde_json::Value) -> Option<PatentHit> {
        let patent_id = doc
            .get("patentNumber")
            .or_else(|| doc.get("patent_number"))
            .or_else(|| doc.get("documentId"))
            .and_then(|v| v.as_str())?;

        let date = doc
            .get("grantDate")
            .or_else(|| doc.get("grant_date"))
            .or_else(|| doc.get("publicationDate"))
            .or_else(|| doc.get("publication_date"))
            .or_else(|| doc.get("filingDate"))
            .or_else(|| doc.get("filing_date"))
            .and_then(|v| v.as_str())
            .map(String::from);

        let inventors_raw = doc.get("inventors");
        let inventors: Vec<String> = match inventors_raw {
            Some(v) if v.is_string() => vec![v.as_str().unwrap().to_string()],
            Some(v) if v.is_array() => v
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|i| i.as_str().map(String::from))
                .collect(),
            _ => vec![],
        };

        Some(PatentHit {
            title: doc.get("title").and_then(|v| v.as_str()).map(String::from),
            date,
            assignee: doc
                .get("assignee")
                .and_then(|v| v.as_str())
                .map(String::from),
            inventors,
            abstract_text: doc
                .get("abstract")
                .and_then(|v| v.as_str())
                .map(String::from),
            ..PatentHit::new(patent_id.to_string(), super::SOURCE_USPTO)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn uspto_map_doc_basic() {
        let doc = serde_json::json!({
            "patentNumber": "US9876543",
            "title": "USPTO Patent",
            "grantDate": "2021-03-20",
            "assignee": "USPTO Corp",
            "inventors": ["Carol"],
            "abstract": "USPTO abstract text",
        });
        let hit = UsptoTextSearchBackend::map_doc(&doc).unwrap();
        assert_eq!(hit.patent_id, "US9876543");
        assert_eq!(hit.title.as_deref(), Some("USPTO Patent"));
        assert_eq!(hit.date.as_deref(), Some("2021-03-20"));
        assert_eq!(hit.assignee.as_deref(), Some("USPTO Corp"));
        assert_eq!(hit.inventors, vec!["Carol"]);
        assert_eq!(hit.source, "USPTO_PPUBS");
    }

    #[test]
    fn uspto_map_doc_no_patent_id_returns_none() {
        let doc = serde_json::json!({"title": "No number"});
        assert!(UsptoTextSearchBackend::map_doc(&doc).is_none());
    }

    #[test]
    fn uspto_map_doc_fallback_id_fields() {
        let doc = serde_json::json!({
            "documentId": "EP5555555",
            "title": "Doc ID Fallback",
        });
        let hit = UsptoTextSearchBackend::map_doc(&doc).unwrap();
        assert_eq!(hit.patent_id, "EP5555555");
    }

    #[test]
    fn uspto_map_doc_date_fallback_chain() {
        let doc = serde_json::json!({
            "patentNumber": "US333",
            "publicationDate": "2022-07-15",
        });
        let hit = UsptoTextSearchBackend::map_doc(&doc).unwrap();
        assert_eq!(hit.date.as_deref(), Some("2022-07-15"));
    }

    #[test]
    fn uspto_map_doc_string_inventors() {
        let doc = serde_json::json!({
            "patentNumber": "US444",
            "inventors": "Single Name",
        });
        let hit = UsptoTextSearchBackend::map_doc(&doc).unwrap();
        assert_eq!(hit.inventors, vec!["Single Name"]);
    }

    #[test]
    fn uspto_constructor_default_url() {
        let backend = UsptoTextSearchBackend::new(None, Duration::from_secs(30), None);
        assert_eq!(backend.base_url, "https://ppubs.uspto.gov/ppubs-api/v1");
    }
}
