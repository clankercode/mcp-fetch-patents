use std::time::Duration;

use reqwest::Client;
use tracing::warn;

use crate::ranking::PatentHit;

pub struct SerpApiGooglePatentsBackend {
    pub api_key: String,
    pub base_url: String,
    pub client: Client,
}

impl SerpApiGooglePatentsBackend {
    pub fn new(
        api_key: String,
        base_url: Option<String>,
        timeout: Duration,
        client: Option<Client>,
    ) -> Self {
        Self {
            api_key,
            base_url: base_url.unwrap_or_else(|| "https://serpapi.com/search".to_string()),
            client: client.unwrap_or_else(|| {
                Client::builder()
                    .timeout(timeout)
                    .build()
                    .unwrap_or_else(|_| Client::new())
            }),
        }
    }

    #[tracing::instrument(skip_all)]
    #[allow(clippy::too_many_arguments)]
    pub async fn search(
        &self,
        query: &str,
        date_from: Option<&str>,
        date_to: Option<&str>,
        assignee: Option<&str>,
        inventor: Option<&str>,
        patent_type: Option<&str>,
        max_results: usize,
    ) -> anyhow::Result<Vec<PatentHit>> {
        let mut params = vec![
            ("engine", "google_patents".to_string()),
            ("q", query.to_string()),
            ("api_key", self.api_key.clone()),
            ("num", max_results.to_string()),
        ];

        if let Some(df) = date_from {
            params.push(("after_priority_date", df.replace('-', "/")));
        }
        if let Some(dt) = date_to {
            params.push(("before_priority_date", dt.replace('-', "/")));
        }
        if let Some(a) = assignee {
            params.push(("assignee", a.to_string()));
        }
        if let Some(inv) = inventor {
            params.push(("inventor", inv.to_string()));
        }
        if let Some(pt) = patent_type {
            params.push(("type", pt.to_string()));
        }

        let resp = self
            .client
            .get(&self.base_url)
            .query(&params)
            .send()
            .await
            .map_err(|e| {
                warn!("SerpAPI Google Patents request failed: {}", e);
                anyhow::anyhow!("SerpAPI Google Patents request failed: {}", e)
            })?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            warn!("SerpAPI Google Patents HTTP error {}", status);
            return Err(anyhow::anyhow!(
                "SerpAPI Google Patents HTTP error {}",
                status
            ));
        }

        let data: serde_json::Value = resp.json().await.map_err(|e| {
            warn!("SerpAPI Google Patents JSON parse error: {}", e);
            anyhow::anyhow!("SerpAPI Google Patents JSON parse error: {}", e)
        })?;

        let organic = data
            .get("organic_results")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut hits = Vec::new();
        for item in &organic {
            if let Some(hit) = Self::map_result(item) {
                hits.push(hit);
            }
        }
        Ok(hits)
    }

    pub fn map_result(item: &serde_json::Value) -> Option<PatentHit> {
        let patent_id = item
            .get("patent_id")
            .or_else(|| item.get("result_id"))
            .or_else(|| item.get("id"))
            .and_then(|v| v.as_str())?;

        let date = item
            .get("grant_date")
            .or_else(|| item.get("filing_date"))
            .or_else(|| item.get("priority_date"))
            .and_then(|v| v.as_str())
            .map(String::from);

        let inventors = item
            .get("inventor")
            .map(super::string_or_array_to_vec)
            .unwrap_or_default();

        let url = item
            .get("pdf")
            .or_else(|| item.get("link"))
            .and_then(|v| v.as_str())
            .map(String::from);

        Some(PatentHit {
            title: item.get("title").and_then(|v| v.as_str()).map(String::from),
            date,
            assignee: item
                .get("assignee")
                .and_then(|v| v.as_str())
                .map(String::from),
            inventors,
            abstract_text: item
                .get("snippet")
                .or_else(|| item.get("abstract"))
                .and_then(|v| v.as_str())
                .map(String::from),
            url,
            ..PatentHit::new(patent_id.to_string(), super::SOURCE_SERPAPI)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn make_serpapi_organic_result(patent_id: &str, title: &str) -> serde_json::Value {
        serde_json::json!({
            "patent_id": patent_id,
            "title": title,
            "grant_date": "2020-01-15",
            "assignee": "Test Corp",
            "inventor": ["Alice", "Bob"],
            "snippet": "A test abstract",
            "pdf": "https://example.com/test.pdf",
        })
    }

    #[test]
    fn serpapi_map_result_basic() {
        let item = make_serpapi_organic_result("US1234567", "Test Patent");
        let hit = SerpApiGooglePatentsBackend::map_result(&item).unwrap();
        assert_eq!(hit.patent_id, "US1234567");
        assert_eq!(hit.title.as_deref(), Some("Test Patent"));
        assert_eq!(hit.date.as_deref(), Some("2020-01-15"));
        assert_eq!(hit.assignee.as_deref(), Some("Test Corp"));
        assert_eq!(hit.inventors, vec!["Alice", "Bob"]);
        assert_eq!(hit.abstract_text.as_deref(), Some("A test abstract"));
        assert_eq!(hit.source, "SerpAPI_Google_Patents");
        assert_eq!(hit.url.as_deref(), Some("https://example.com/test.pdf"));
    }

    #[test]
    fn serpapi_map_result_no_patent_id_returns_none() {
        let item = serde_json::json!({
            "title": "No ID",
        });
        assert!(SerpApiGooglePatentsBackend::map_result(&item).is_none());
    }

    #[test]
    fn serpapi_map_result_fallback_id_fields() {
        let item = serde_json::json!({
            "result_id": "EP9999999",
            "title": "Fallback",
        });
        let hit = SerpApiGooglePatentsBackend::map_result(&item).unwrap();
        assert_eq!(hit.patent_id, "EP9999999");
    }

    #[test]
    fn serpapi_map_result_string_inventor() {
        let item = serde_json::json!({
            "id": "US111",
            "title": "Single",
            "inventor": "Solo Inventor",
        });
        let hit = SerpApiGooglePatentsBackend::map_result(&item).unwrap();
        assert_eq!(hit.inventors, vec!["Solo Inventor"]);
    }

    #[test]
    fn serpapi_map_result_date_fallback() {
        let item = serde_json::json!({
            "patent_id": "US222",
            "title": "T",
            "filing_date": "2019-06-01",
        });
        let hit = SerpApiGooglePatentsBackend::map_result(&item).unwrap();
        assert_eq!(hit.date.as_deref(), Some("2019-06-01"));
    }

    #[test]
    fn serpapi_constructor_default_url() {
        let backend = SerpApiGooglePatentsBackend::new(
            "test-key".to_string(),
            None,
            Duration::from_secs(30),
            None,
        );
        assert_eq!(backend.api_key, "test-key");
        assert_eq!(backend.base_url, "https://serpapi.com/search");
    }

    #[test]
    fn serpapi_map_result_priority_date_fallback() {
        let item = serde_json::json!({
            "patent_id": "US9876543",
            "title": "Priority Date Test",
            "priority_date": "2018-03-20",
            "inventor": ["Carol"],
        });
        let hit = SerpApiGooglePatentsBackend::map_result(&item).unwrap();
        assert_eq!(hit.date.as_deref(), Some("2018-03-20"));
    }
}
