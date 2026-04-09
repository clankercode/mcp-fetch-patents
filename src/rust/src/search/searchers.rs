use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use reqwest::Client;
use tracing::warn;

use crate::ranking::PatentHit;

fn validate_path_segment(input: &str, label: &str) -> Result<()> {
    if input.contains('/')
        || input.contains('\\')
        || input.contains("..")
        || input.contains('?')
        || input.contains('#')
        || input.chars().any(|c| c.is_whitespace())
    {
        return Err(anyhow!(
            "Invalid {} '{}' contains forbidden characters",
            label,
            input
        ));
    }
    Ok(())
}

pub struct SerpApiGooglePatentsBackend {
    api_key: String,
    base_url: String,
    client: Client,
}

impl SerpApiGooglePatentsBackend {
    pub fn new(api_key: String, base_url: Option<String>, client: Option<Client>) -> Self {
        Self {
            api_key,
            base_url: base_url.unwrap_or_else(|| "https://serpapi.com/search".to_string()),
            client: client.unwrap_or_else(|| {
                Client::builder()
                    .timeout(Duration::from_secs(30))
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
    ) -> Result<Vec<PatentHit>> {
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

        let resp = match self.client.get(&self.base_url).query(&params).send().await {
            Ok(r) => r,
            Err(e) => {
                warn!("SerpAPI Google Patents request failed: {}", e);
                return Ok(vec![]);
            }
        };

        if !resp.status().is_success() {
            warn!(
                "SerpAPI Google Patents HTTP error {}",
                resp.status().as_u16()
            );
            return Ok(vec![]);
        }

        let data: serde_json::Value = match resp.json().await {
            Ok(d) => d,
            Err(e) => {
                warn!("SerpAPI Google Patents JSON parse error: {}", e);
                return Ok(vec![]);
            }
        };

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

    fn map_result(item: &serde_json::Value) -> Option<PatentHit> {
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

        let inventors_raw = item.get("inventor");
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

        let url = item
            .get("pdf")
            .or_else(|| item.get("link"))
            .and_then(|v| v.as_str())
            .map(String::from);

        Some(PatentHit {
            patent_id: patent_id.to_string(),
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
            source: "SerpAPI_Google_Patents".to_string(),
            relevance: "unknown".to_string(),
            note: String::new(),
            prior_art: None,
            url,
        })
    }
}

pub struct UsptoTextSearchBackend {
    base_url: String,
    client: Client,
}

impl UsptoTextSearchBackend {
    pub fn new(base_url: Option<String>, client: Option<Client>) -> Self {
        Self {
            base_url: base_url
                .unwrap_or_else(|| "https://ppubs.uspto.gov/ppubs-api/v1".to_string()),
            client: client.unwrap_or_else(|| {
                Client::builder()
                    .timeout(Duration::from_secs(30))
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
    ) -> Result<Vec<PatentHit>> {
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

    fn map_doc(doc: &serde_json::Value) -> Option<PatentHit> {
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
            patent_id: patent_id.to_string(),
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
            source: "USPTO_PPUBS".to_string(),
            relevance: "unknown".to_string(),
            note: String::new(),
            prior_art: None,
            url: None,
        })
    }
}

struct TokenState {
    token: Option<String>,
    expires_at: Option<Instant>,
}

pub struct EpoOpsSearchBackend {
    client_id: Option<String>,
    client_secret: Option<String>,
    base_url: String,
    auth_url: String,
    token_state: Mutex<TokenState>,
    client: Client,
}

impl EpoOpsSearchBackend {
    pub fn new(
        client_id: Option<String>,
        client_secret: Option<String>,
        base_url: Option<String>,
        client: Option<Client>,
    ) -> Self {
        let base = base_url
            .unwrap_or_else(|| "https://ops.epo.org/3.2/rest-services".to_string());
        let auth_url = {
            let trimmed = base.trim_end_matches('/');
            let root = trimmed
                .strip_suffix("/rest-services")
                .unwrap_or(trimmed);
            format!("{}/auth/accesstoken", root)
        };
        Self {
            client_id,
            client_secret,
            base_url: base,
            auth_url,
            token_state: Mutex::new(TokenState {
                token: None,
                expires_at: None,
            }),
            client: client.unwrap_or_else(|| {
                Client::builder()
                    .timeout(Duration::from_secs(30))
                    .build()
                    .unwrap_or_else(|_| Client::new())
            }),
        }
    }

    pub async fn get_oauth_token(&self) -> Result<Option<String>> {
        let client_id = match &self.client_id {
            Some(id) => id.clone(),
            None => return Ok(None),
        };
        let client_secret = match &self.client_secret {
            Some(s) => s.clone(),
            None => return Ok(None),
        };

        {
            let state = self.token_state.lock().unwrap_or_else(|e| e.into_inner());
            if let (Some(ref token), Some(expires_at)) = (&state.token, state.expires_at) {
                if Instant::now() < expires_at - Duration::from_secs(60) {
                    return Ok(Some(token.clone()));
                }
            }
        }

        let form_data = [
            ("grant_type", "client_credentials".to_string()),
            ("client_id", client_id),
            ("client_secret", client_secret),
        ];

        let resp = match self.client
            .post(&self.auth_url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .form(&form_data)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                warn!("EPO OPS OAuth request failed: {}", e);
                return Ok(None);
            }
        };

        if !resp.status().is_success() {
            warn!("EPO OPS OAuth failed: HTTP {}", resp.status().as_u16());
            return Ok(None);
        }

        let data: serde_json::Value = match resp.json().await {
            Ok(d) => d,
            Err(e) => {
                warn!("EPO OPS OAuth JSON parse error: {}", e);
                return Ok(None);
            }
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
            let mut state = self.token_state.lock().unwrap_or_else(|e| e.into_inner());
            state.token = Some(t.clone());
            state.expires_at = Some(Instant::now() + Duration::from_secs(expires_in as u64));
        }

        Ok(token)
    }

    #[tracing::instrument(skip_all)]
    pub async fn search(
        &self,
        query: &str,
        date_from: Option<&str>,
        date_to: Option<&str>,
        max_results: usize,
    ) -> Result<Vec<PatentHit>> {
        let mut cql = query.to_string();
        if date_from.is_some() || date_to.is_some() {
            let mut date_clauses = Vec::new();
            if let Some(df) = date_from {
                date_clauses.push(format!("pd>={}", df.replace('-', "")));
            }
            if let Some(dt) = date_to {
                date_clauses.push(format!("pd<={}", dt.replace('-', "")));
            }
            cql = format!("({}) AND {}", cql, date_clauses.join(" AND "));
        }

        let token = self.get_oauth_token().await.ok().flatten();
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("Accept", "application/json".parse().unwrap());
        if let Some(ref t) = token {
            if let Ok(val) = reqwest::header::HeaderValue::from_str(&format!("Bearer {}", t)) {
                headers.insert(reqwest::header::AUTHORIZATION, val);
            }
        }

        let url = format!("{}/published-data/search", self.base_url);
        let range = format!("1-{}", max_results);
        let params = [("q", cql.as_str()), ("Range", range.as_str())];

        let resp = match self.client
            .get(&url)
            .headers(headers)
            .query(&params)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                warn!("EPO OPS search request failed: {}", e);
                return Ok(vec![]);
            }
        };

        if !resp.status().is_success() {
            warn!("EPO OPS search HTTP error {}", resp.status().as_u16());
            return Ok(vec![]);
        }

        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        if content_type.contains("json") {
            let data: serde_json::Value = match resp.json().await {
                Ok(d) => d,
                Err(e) => {
                    warn!("EPO OPS search JSON parse error: {}", e);
                    return Ok(vec![]);
                }
            };
            Ok(Self::parse_json_response(&data))
        } else {
            let text = match resp.text().await {
                Ok(t) => t,
                Err(e) => {
                    warn!("EPO OPS search body read error: {}", e);
                    return Ok(vec![]);
                }
            };
            Ok(Self::parse_xml_search_response(&text))
        }
    }

    pub async fn search_by_classification(
        &self,
        cpc_code: &str,
        include_subclasses: bool,
        date_from: Option<&str>,
        date_to: Option<&str>,
        max_results: usize,
    ) -> Result<Vec<PatentHit>> {
        validate_path_segment(cpc_code, "classification code")?;
        let code_expr = if include_subclasses {
            format!("cpc={}/*", cpc_code)
        } else {
            format!("cpc={}", cpc_code)
        };
        self.search(&code_expr, date_from, date_to, max_results)
            .await
    }

    #[tracing::instrument(skip(self))]
    pub async fn get_citations(
        &self,
        patent_id: &str,
        direction: &str,
    ) -> Result<Vec<String>> {
        validate_path_segment(patent_id, "patent_id")?;
        let token = self.get_oauth_token().await.ok().flatten();
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("Accept", "application/json".parse().unwrap());
        if let Some(ref t) = token {
            if let Ok(val) = reqwest::header::HeaderValue::from_str(&format!("Bearer {}", t)) {
                headers.insert(reqwest::header::AUTHORIZATION, val);
            }
        }

        if direction == "backward" {
            let url = format!(
                "{}/published-data/citation/epodoc/{}",
                self.base_url, patent_id
            );

            let resp = match self.client.get(&url).headers(headers).send().await {
                Ok(r) => r,
                Err(e) => {
                    warn!(
                        "EPO OPS citation fetch failed for {}: {}",
                        patent_id, e
                    );
                    return Ok(vec![]);
                }
            };

            if !resp.status().is_success() {
                warn!(
                    "EPO OPS citation HTTP error {} for {}",
                    resp.status().as_u16(),
                    patent_id
                );
                return Ok(vec![]);
            }

            let content_type = resp
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();

            if content_type.contains("json") {
                let data: serde_json::Value = match resp.json().await {
                    Ok(d) => d,
                    Err(e) => {
                        warn!("EPO OPS citation JSON parse error: {}", e);
                        return Ok(vec![]);
                    }
                };
                Ok(Self::extract_ids_from_json(&data))
            } else {
                let text = match resp.text().await {
                    Ok(t) => t,
                    Err(e) => {
                        warn!("EPO OPS citation body read error: {}", e);
                        return Ok(vec![]);
                    }
                };
                Ok(Self::parse_xml_citations_response(&text))
            }
        } else {
            let cql = format!("ct={}", patent_id);
            let hits = self.search(&cql, None, None, 100).await?;
            Ok(hits.into_iter().map(|h| h.patent_id).collect())
        }
    }

    #[tracing::instrument(skip(self))]
    pub async fn get_family(
        &self,
        patent_id: &str,
    ) -> Result<Vec<serde_json::Value>> {
        validate_path_segment(patent_id, "patent_id")?;
        let token = self.get_oauth_token().await.ok().flatten();
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("Accept", "application/json".parse().unwrap());
        if let Some(ref t) = token {
            if let Ok(val) = reqwest::header::HeaderValue::from_str(&format!("Bearer {}", t)) {
                headers.insert(reqwest::header::AUTHORIZATION, val);
            }
        }

        let url = format!(
            "{}/family/publication/epodoc/{}",
            self.base_url, patent_id
        );

        let resp = match self.client.get(&url).headers(headers).send().await {
            Ok(r) => r,
            Err(e) => {
                warn!("EPO OPS family fetch failed for {}: {}", patent_id, e);
                return Ok(vec![]);
            }
        };

        if !resp.status().is_success() {
            warn!(
                "EPO OPS family HTTP error {} for {}",
                resp.status().as_u16(),
                patent_id
            );
            return Ok(vec![]);
        }

        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        if content_type.contains("json") {
            let data: serde_json::Value = match resp.json().await {
                Ok(d) => d,
                Err(e) => {
                    warn!("EPO OPS family JSON parse error: {}", e);
                    return Ok(vec![]);
                }
            };
            Ok(Self::parse_family_json(&data))
        } else {
            let text = match resp.text().await {
                Ok(t) => t,
                Err(e) => {
                    warn!("EPO OPS family body read error: {}", e);
                    return Ok(vec![]);
                }
            };
            Ok(Self::parse_xml_family_response(&text))
        }
    }

    fn parse_json_response(data: &serde_json::Value) -> Vec<PatentHit> {
        let mut hits = Vec::new();
        let ops_data = data
            .get("ops:world-patent-data")
            .unwrap_or(data)
            .get("ops:biblio-search")
            .unwrap_or(data)
            .get("ops:search-result");

        let exchange_docs = match ops_data {
            Some(sr) => sr.get("exchange-documents"),
            None => None,
        };

        let docs_vec = match exchange_docs {
            Some(v) if v.is_array() => v.as_array().unwrap().clone(),
            Some(v) if v.is_object() => vec![v.clone()],
            _ => vec![],
        };

        for doc_wrapper in &docs_vec {
            let doc = doc_wrapper
                .get("exchange-document")
                .unwrap_or(doc_wrapper);
            if let Some(hit) = Self::map_ops_json_doc(doc) {
                hits.push(hit);
            }
        }
        hits
    }

    fn map_ops_json_doc(doc: &serde_json::Value) -> Option<PatentHit> {
        let patent_id_raw = doc
            .get("@doc-number")
            .or_else(|| doc.get("doc-number"))
            .and_then(|v| v.as_str());
        let country = doc
            .get("@country")
            .or_else(|| doc.get("country"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let kind = doc
            .get("@kind")
            .or_else(|| doc.get("kind"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let patent_id = match patent_id_raw {
            Some(id) => format!("{}{}{}", country, id, kind),
            None => return None,
        };
        if patent_id.trim().is_empty() {
            return None;
        }

        let biblio = doc.get("bibliographic-data").cloned().unwrap_or(serde_json::json!({}));

        let title = {
            let title_data = biblio.get("invention-title");
            let title_items: Vec<&serde_json::Value> = match title_data {
                Some(v) if v.is_array() => v.as_array().unwrap().iter().collect(),
                Some(v) if v.is_object() => vec![v],
                _ => vec![],
            };
            let mut result: Option<String> = None;
            for t in &title_items {
                if let Some(obj) = t.as_object() {
                    let lang = obj
                        .get("@lang")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let val = obj
                        .get("$")
                        .or_else(|| obj.get("#text"))
                        .and_then(|v| v.as_str());
                    if let Some(v) = val {
                        if lang == "en" {
                            result = Some(v.to_string());
                            break;
                        }
                        if result.is_none() {
                            result = Some(v.to_string());
                        }
                    }
                }
            }
            result
        };

        let date = {
            let pub_refs = biblio.get("publication-reference");
            let refs: Vec<&serde_json::Value> = match pub_refs {
                Some(v) if v.is_array() => v.as_array().unwrap().iter().collect(),
                Some(v) if v.is_object() => vec![v],
                _ => vec![],
            };
            let mut result: Option<String> = None;
            for r in &refs {
                let doc_id_list = r.get("document-id");
                let ids: Vec<&serde_json::Value> = match doc_id_list {
                    Some(v) if v.is_array() => v.as_array().unwrap().iter().collect(),
                    Some(v) if v.is_object() => vec![v],
                    _ => vec![],
                };
                for did in &ids {
                    let d = did.get("date");
                    if let Some(dv) = d {
                        let val = if dv.is_object() {
                            dv.get("$")
                                .or_else(|| dv.get("#text"))
                                .and_then(|v| v.as_str())
                                .map(String::from)
                        } else {
                            dv.as_str().map(String::from)
                        };
                        if let Some(v) = val {
                            result = Some(v);
                            break;
                        }
                    }
                }
                if result.is_some() {
                    break;
                }
            }
            result
        };

        let inventors = {
            let parties = biblio.get("parties").cloned().unwrap_or(serde_json::json!({}));
            let inv_section = parties
                .get("inventors")
                .and_then(|v| v.get("inventor"));
            let inv_list: Vec<&serde_json::Value> = match inv_section {
                Some(v) if v.is_array() => v.as_array().unwrap().iter().collect(),
                Some(v) if v.is_object() => vec![v],
                _ => vec![],
            };
            let mut names = Vec::new();
            for inv in &inv_list {
                let name_data = inv
                    .get("inventor-name")
                    .and_then(|v| v.get("name"));
                if let Some(nd) = name_data {
                    if nd.is_object() {
                        let name = nd
                            .get("$")
                            .or_else(|| nd.get("#text"))
                            .and_then(|v| v.as_str());
                        if let Some(n) = name {
                            names.push(n.to_string());
                        }
                    } else if let Some(n) = nd.as_str() {
                        names.push(n.to_string());
                    }
                }
            }
            names
        };

        let assignee = {
            let parties = biblio.get("parties").cloned().unwrap_or(serde_json::json!({}));
            let app_section = parties
                .get("applicants")
                .and_then(|v| v.get("applicant"));
            let app_list: Vec<&serde_json::Value> = match app_section {
                Some(v) if v.is_array() => v.as_array().unwrap().iter().collect(),
                Some(v) if v.is_object() => vec![v],
                _ => vec![],
            };
            let mut result: Option<String> = None;
            for app in &app_list {
                let name_data = app
                    .get("applicant-name")
                    .and_then(|v| v.get("name"));
                if let Some(nd) = name_data {
                    if nd.is_object() {
                        let val = nd
                            .get("$")
                            .or_else(|| nd.get("#text"))
                            .and_then(|v| v.as_str());
                        if let Some(v) = val {
                            result = Some(v.to_string());
                            break;
                        }
                    } else if let Some(v) = nd.as_str() {
                        result = Some(v.to_string());
                        break;
                    }
                }
            }
            result
        };

        Some(PatentHit {
            patent_id,
            title,
            date,
            assignee,
            inventors,
            abstract_text: None,
            source: "EPO_OPS".to_string(),
            relevance: "unknown".to_string(),
            note: String::new(),
            prior_art: None,
            url: None,
        })
    }

    fn extract_ids_from_json(data: &serde_json::Value) -> Vec<String> {
        let mut ids = Vec::new();
        let world_data = data
            .get("ops:world-patent-data")
            .unwrap_or(data);
        let citation_list = world_data.get("ops:citation");
        let citations: Vec<&serde_json::Value> = match citation_list {
            Some(v) if v.is_array() => v.as_array().unwrap().iter().collect(),
            Some(v) if v.is_object() => vec![v],
            _ => vec![],
        };
        for c in &citations {
            let doc_id = c
                .get("patcit")
                .and_then(|v| v.get("document-id"));
            if let Some(di) = doc_id {
                let num = di.get("doc-number");
                let val = match num {
                    Some(n) if n.is_object() => n
                        .get("$")
                        .or_else(|| n.get("#text"))
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    Some(n) => n.as_str().map(String::from),
                    None => None,
                };
                let country = di.get("country");
                let cc = match country {
                    Some(c) if c.is_object() => c.get("$").and_then(|v| v.as_str()).unwrap_or(""),
                    Some(c) => c.as_str().unwrap_or(""),
                    None => "",
                };
                if let Some(v) = val {
                    ids.push(format!("{}{}", cc, v));
                }
            }
        }
        ids
    }

    fn parse_family_json(data: &serde_json::Value) -> Vec<serde_json::Value> {
        let mut members = Vec::new();
        let world_data = data
            .get("ops:world-patent-data")
            .unwrap_or(data);
        let family_data = world_data
            .get("ops:patent-family")
            .cloned()
            .unwrap_or(serde_json::json!({}));
        let family_members = family_data.get("ops:family-member");
        let members_list: Vec<&serde_json::Value> = match family_members {
            Some(v) if v.is_array() => v.as_array().unwrap().iter().collect(),
            Some(v) if v.is_object() => vec![v],
            _ => vec![],
        };
        for m in &members_list {
            let pub_refs = m.get("publication-reference");
            let refs: Vec<&serde_json::Value> = match pub_refs {
                Some(v) if v.is_array() => v.as_array().unwrap().iter().collect(),
                Some(v) if v.is_object() => vec![v],
                _ => vec![],
            };
            for r in &refs {
                let doc_ids = r.get("document-id");
                let ids: Vec<&serde_json::Value> = match doc_ids {
                    Some(v) if v.is_array() => v.as_array().unwrap().iter().collect(),
                    Some(v) if v.is_object() => vec![v],
                    _ => vec![],
                };
                for did in &ids {
                    let country = did.get("country");
                    let cc = match country {
                        Some(c) if c.is_object() => {
                            c.get("$").and_then(|v| v.as_str()).unwrap_or("")
                        }
                        Some(c) => c.as_str().unwrap_or(""),
                        None => "",
                    };
                    let num = did.get("doc-number");
                    let n = match num {
                        Some(v) if v.is_object() => {
                            v.get("$").and_then(|v| v.as_str()).unwrap_or("")
                        }
                        Some(v) => v.as_str().unwrap_or(""),
                        None => "",
                    };
                    let kind = did.get("kind");
                    let k = match kind {
                        Some(v) if v.is_object() => {
                            v.get("$").and_then(|v| v.as_str()).unwrap_or("")
                        }
                        Some(v) => v.as_str().unwrap_or(""),
                        None => "",
                    };
                    let date_obj = did.get("date");
                    let d = match date_obj {
                        Some(v) if v.is_object() => {
                            v.get("$").and_then(|v| v.as_str()).unwrap_or("")
                        }
                        Some(v) => v.as_str().unwrap_or(""),
                        None => "",
                    };
                    if !n.is_empty() {
                        members.push(serde_json::json!({
                            "patent_id": format!("{}{}{}", cc, n, k),
                            "country": cc,
                            "doc_type": k,
                            "date": d,
                        }));
                    }
                }
            }
        }
        members
    }

    fn parse_xml_search_response(body: &str) -> Vec<PatentHit> {
        use quick_xml::Reader;
        use quick_xml::events::Event;

        let mut reader = Reader::from_str(body);
        reader.config_mut().trim_text(true);

        let mut hits = Vec::new();
        let mut in_doc_id = false;
        let mut country = String::new();
        let mut doc_number = String::new();
        let mut kind = String::new();
        let mut current_tag = String::new();
        let mut buf = Vec::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                    let local = local_name(e.name().as_ref());
                    match local.as_str() {
                        "document-id" => {
                            in_doc_id = true;
                            country.clear();
                            doc_number.clear();
                            kind.clear();
                        }
                        "country" | "doc-number" | "kind" if in_doc_id => {
                            current_tag = local.clone();
                        }
                        _ => {}
                    }
                }
                Ok(Event::Text(e)) => {
                    if in_doc_id && !current_tag.is_empty() {
                        let val = String::from_utf8_lossy(e.as_ref());
                        let v = val.trim().to_string();
                        match current_tag.as_str() {
                            "country" => country = v,
                            "doc-number" => doc_number = v,
                            "kind" => kind = v,
                            _ => {}
                        }
                    }
                }
                Ok(Event::End(e)) => {
                    let local = local_name(e.name().as_ref());
                    if local == "document-id" && in_doc_id {
                        in_doc_id = false;
                        current_tag.clear();
                        if !doc_number.is_empty() {
                            let patent_id = format!("{}{}{}", country, doc_number, kind);
                            if !patent_id.trim().is_empty() {
                                hits.push(PatentHit {
                                    patent_id,
                                    title: None,
                                    date: None,
                                    assignee: None,
                                    inventors: vec![],
                                    abstract_text: None,
                                    source: "EPO_OPS".to_string(),
                                    relevance: "unknown".to_string(),
                                    note: String::new(),
                                    prior_art: None,
                                    url: None,
                                });
                            }
                        }
                    }
                    if local == current_tag {
                        current_tag.clear();
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => {
                    warn!("EPO OPS search XML parse error: {}", e);
                    break;
                }
                _ => {}
            }
            buf.clear();
        }
        hits
    }

    fn parse_xml_citations_response(body: &str) -> Vec<String> {
        use quick_xml::Reader;
        use quick_xml::events::Event;

        let mut reader = Reader::from_str(body);
        reader.config_mut().trim_text(true);

        let mut ids = Vec::new();
        let mut in_patcit_doc_id = false;
        let mut country = String::new();
        let mut doc_number = String::new();
        let mut current_tag = String::new();
        let mut buf = Vec::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                    let local = local_name(e.name().as_ref());
                    match local.as_str() {
                        "document-id" => {
                            in_patcit_doc_id = true;
                            country.clear();
                            doc_number.clear();
                        }
                        "country" | "doc-number" if in_patcit_doc_id => {
                            current_tag = local.clone();
                        }
                        _ => {}
                    }
                }
                Ok(Event::Text(e)) => {
                    if in_patcit_doc_id && !current_tag.is_empty() {
                        let val = String::from_utf8_lossy(e.as_ref());
                        let v = val.trim().to_string();
                        match current_tag.as_str() {
                            "country" => country = v,
                            "doc-number" => doc_number = v,
                            _ => {}
                        }
                    }
                }
                Ok(Event::End(e)) => {
                    let local = local_name(e.name().as_ref());
                    if local == "document-id" && in_patcit_doc_id {
                        in_patcit_doc_id = false;
                        current_tag.clear();
                        if !doc_number.is_empty() {
                            ids.push(format!("{}{}", country, doc_number));
                        }
                    }
                    if local == current_tag {
                        current_tag.clear();
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => {
                    warn!("EPO OPS citation XML parse error: {}", e);
                    break;
                }
                _ => {}
            }
            buf.clear();
        }
        ids
    }

    fn parse_xml_family_response(body: &str) -> Vec<serde_json::Value> {
        use quick_xml::Reader;
        use quick_xml::events::Event;

        let mut reader = Reader::from_str(body);
        reader.config_mut().trim_text(true);

        let mut members = Vec::new();
        let mut in_doc_id = false;
        let mut country = String::new();
        let mut doc_number = String::new();
        let mut kind = String::new();
        let mut date = String::new();
        let mut current_tag = String::new();
        let mut buf = Vec::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                    let local = local_name(e.name().as_ref());
                    match local.as_str() {
                        "document-id" => {
                            in_doc_id = true;
                            country.clear();
                            doc_number.clear();
                            kind.clear();
                            date.clear();
                        }
                        "country" | "doc-number" | "kind" | "date" if in_doc_id => {
                            current_tag = local.clone();
                        }
                        _ => {}
                    }
                }
                Ok(Event::Text(e)) => {
                    if in_doc_id && !current_tag.is_empty() {
                        let val = String::from_utf8_lossy(e.as_ref());
                        let v = val.trim().to_string();
                        match current_tag.as_str() {
                            "country" => country = v,
                            "doc-number" => doc_number = v,
                            "kind" => kind = v,
                            "date" => date = v,
                            _ => {}
                        }
                    }
                }
                Ok(Event::End(e)) => {
                    let local = local_name(e.name().as_ref());
                    if local == "document-id" && in_doc_id {
                        in_doc_id = false;
                        current_tag.clear();
                        if !doc_number.is_empty() {
                            members.push(serde_json::json!({
                                "patent_id": format!("{}{}{}", country, doc_number, kind),
                                "country": country,
                                "doc_type": kind,
                                "date": date,
                            }));
                        }
                    }
                    if local == current_tag {
                        current_tag.clear();
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => {
                    warn!("EPO OPS family XML parse error: {}", e);
                    break;
                }
                _ => {}
            }
            buf.clear();
        }
        members
    }
}

fn local_name(raw: &[u8]) -> String {
    let name = String::from_utf8_lossy(raw);
    match name.rfind(':') {
        Some(pos) => name[pos + 1..].to_string(),
        None => name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_serpapi_organic_result(
        patent_id: &str,
        title: &str,
    ) -> serde_json::Value {
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
        assert_eq!(
            hit.abstract_text.as_deref(),
            Some("A test abstract")
        );
        assert_eq!(hit.source, "SerpAPI_Google_Patents");
        assert_eq!(
            hit.url.as_deref(),
            Some("https://example.com/test.pdf")
        );
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
    fn epo_ops_parse_json_response_basic() {
        let data = serde_json::json!({
            "ops:world-patent-data": {
                "ops:biblio-search": {
                    "ops:search-result": {
                        "exchange-documents": [
                            {
                                "exchange-document": {
                                    "@doc-number": "1234567",
                                    "@country": "US",
                                    "@kind": "B2",
                                    "bibliographic-data": {
                                        "invention-title": [{"@lang": "en", "$": "EPO Test Title"}],
                                        "publication-reference": {
                                            "document-id": {
                                                "date": {"$": "20200615"}
                                            }
                                        },
                                        "parties": {
                                            "inventors": {
                                                "inventor": {
                                                    "inventor-name": {
                                                        "name": {"$": "Test Inventor"}
                                                    }
                                                }
                                            },
                                            "applicants": {
                                                "applicant": {
                                                    "applicant-name": {
                                                        "name": {"$": "Test Applicant"}
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        ]
                    }
                }
            }
        });
        let hits = EpoOpsSearchBackend::parse_json_response(&data);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].patent_id, "US1234567B2");
        assert_eq!(hits[0].title.as_deref(), Some("EPO Test Title"));
        assert_eq!(hits[0].date.as_deref(), Some("20200615"));
        assert_eq!(hits[0].inventors, vec!["Test Inventor"]);
        assert_eq!(hits[0].assignee.as_deref(), Some("Test Applicant"));
        assert_eq!(hits[0].source, "EPO_OPS");
    }

    #[test]
    fn epo_ops_parse_json_response_empty() {
        let hits = EpoOpsSearchBackend::parse_json_response(&serde_json::json!({}));
        assert!(hits.is_empty());
    }

    #[test]
    fn epo_ops_extract_ids_from_json() {
        let data = serde_json::json!({
            "ops:world-patent-data": {
                "ops:citation": [
                    {
                        "patcit": {
                            "document-id": {
                                "doc-number": {"$": "1111111"},
                                "country": {"$": "US"}
                            }
                        }
                    },
                    {
                        "patcit": {
                            "document-id": {
                                "doc-number": "2222222",
                                "country": "EP"
                            }
                        }
                    }
                ]
            }
        });
        let ids = EpoOpsSearchBackend::extract_ids_from_json(&data);
        assert_eq!(ids.len(), 2);
        assert_eq!(ids[0], "US1111111");
        assert_eq!(ids[1], "EP2222222");
    }

    #[test]
    fn epo_ops_extract_ids_empty() {
        let ids = EpoOpsSearchBackend::extract_ids_from_json(&serde_json::json!({}));
        assert!(ids.is_empty());
    }

    #[test]
    fn epo_ops_parse_family_json() {
        let data = serde_json::json!({
            "ops:world-patent-data": {
                "ops:patent-family": {
                    "ops:family-member": [
                        {
                            "publication-reference": {
                                "document-id": [
                                    {
                                        "country": {"$": "US"},
                                        "doc-number": {"$": "1234567"},
                                        "kind": {"$": "A1"},
                                        "date": {"$": "20200115"}
                                    },
                                    {
                                        "country": "EP",
                                        "doc-number": "7654321",
                                        "kind": "B1",
                                        "date": "20210620"
                                    }
                                ]
                            }
                        }
                    ]
                }
            }
        });
        let members = EpoOpsSearchBackend::parse_family_json(&data);
        assert_eq!(members.len(), 2);
        assert_eq!(members[0]["patent_id"].as_str().unwrap(), "US1234567A1");
        assert_eq!(members[0]["country"].as_str().unwrap(), "US");
        assert_eq!(members[1]["patent_id"].as_str().unwrap(), "EP7654321B1");
        assert_eq!(members[1]["country"].as_str().unwrap(), "EP");
    }

    #[test]
    fn epo_ops_parse_family_json_empty() {
        let members = EpoOpsSearchBackend::parse_family_json(&serde_json::json!({}));
        assert!(members.is_empty());
    }

    #[test]
    fn epo_ops_map_doc_no_doc_number_returns_none() {
        let doc = serde_json::json!({
            "@country": "US",
            "@kind": "A1",
        });
        assert!(EpoOpsSearchBackend::map_ops_json_doc(&doc).is_none());
    }

    #[test]
    fn epo_ops_map_doc_title_fallback_to_non_english() {
        let doc = serde_json::json!({
            "doc-number": "9999999",
            "country": "DE",
            "kind": "A1",
            "bibliographic-data": {
                "invention-title": [
                    {"@lang": "de", "$": "Deutscher Titel"},
                    {"@lang": "fr", "$": "Titre Francais"}
                ]
            }
        });
        let hit = EpoOpsSearchBackend::map_ops_json_doc(&doc).unwrap();
        assert_eq!(hit.patent_id, "DE9999999A1");
        assert_eq!(hit.title.as_deref(), Some("Deutscher Titel"));
    }

    #[test]
    fn epo_ops_constructor_default_urls() {
        let backend = EpoOpsSearchBackend::new(None, None, None, None);
        assert_eq!(
            backend.base_url,
            "https://ops.epo.org/3.2/rest-services"
        );
        assert_eq!(
            backend.auth_url,
            "https://ops.epo.org/3.2/auth/accesstoken"
        );
    }

    #[test]
    fn epo_ops_constructor_custom_base_url() {
        let backend = EpoOpsSearchBackend::new(
            None,
            None,
            Some("https://custom.epo.org/rest-services".to_string()),
            None,
        );
        assert_eq!(backend.base_url, "https://custom.epo.org/rest-services");
        assert_eq!(
            backend.auth_url,
            "https://custom.epo.org/auth/accesstoken"
        );
    }

    #[test]
    fn epo_ops_constructor_custom_base_url_no_suffix() {
        let backend = EpoOpsSearchBackend::new(
            None,
            None,
            Some("https://custom.epo.org/api".to_string()),
            None,
        );
        assert_eq!(backend.auth_url, "https://custom.epo.org/api/auth/accesstoken");
    }

    #[test]
    fn epo_ops_get_oauth_token_no_credentials() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let backend = EpoOpsSearchBackend::new(None, None, None, None);
            let token = backend.get_oauth_token().await.unwrap();
            assert!(token.is_none());
        });
    }

    #[test]
    fn serpapi_constructor_default_url() {
        let backend = SerpApiGooglePatentsBackend::new("test-key".to_string(), None, None);
        assert_eq!(backend.api_key, "test-key");
        assert_eq!(backend.base_url, "https://serpapi.com/search");
    }

    #[test]
    fn uspto_constructor_default_url() {
        let backend = UsptoTextSearchBackend::new(None, None);
        assert_eq!(
            backend.base_url,
            "https://ppubs.uspto.gov/ppubs-api/v1"
        );
    }

    #[test]
    fn epo_ops_parse_xml_search_response_basic() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ops:world-patent-data xmlns:ops="http://ops.epo.org" xmlns:ex="http://www.epo.org/exchange">
  <ops:biblio-search total-result-count="2">
    <ops:search-result>
      <ops:publication-reference data-format="docdb">
        <document-id>
          <country>US</country>
          <doc-number>1234567</doc-number>
          <kind>B2</kind>
        </document-id>
      </ops:publication-reference>
      <ops:publication-reference data-format="docdb">
        <document-id>
          <country>EP</country>
          <doc-number>3456789</doc-number>
          <kind>A1</kind>
        </document-id>
      </ops:publication-reference>
    </ops:search-result>
  </ops:biblio-search>
</ops:world-patent-data>"#;
        let hits = EpoOpsSearchBackend::parse_xml_search_response(xml);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].patent_id, "US1234567B2");
        assert_eq!(hits[0].source, "EPO_OPS");
        assert_eq!(hits[1].patent_id, "EP3456789A1");
    }

    #[test]
    fn epo_ops_parse_xml_search_response_empty() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ops:world-patent-data xmlns:ops="http://ops.epo.org"></ops:world-patent-data>"#;
        let hits = EpoOpsSearchBackend::parse_xml_search_response(xml);
        assert!(hits.is_empty());
    }

    #[test]
    fn epo_ops_parse_xml_search_response_no_kind() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ops:world-patent-data xmlns:ops="http://ops.epo.org">
  <ops:biblio-search>
    <ops:search-result>
      <ops:publication-reference data-format="docdb">
        <document-id>
          <country>WO</country>
          <doc-number>2020012345</doc-number>
        </document-id>
      </ops:publication-reference>
    </ops:search-result>
  </ops:biblio-search>
</ops:world-patent-data>"#;
        let hits = EpoOpsSearchBackend::parse_xml_search_response(xml);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].patent_id, "WO2020012345");
    }

    #[test]
    fn epo_ops_parse_xml_citations_response_basic() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ops:world-patent-data xmlns:ops="http://ops.epo.org" xmlns:ex="http://www.epo.org/exchange">
  <ex:patent-family>
    <ex:family-member>
      <ex:bibliographic-data>
        <ex:references-cited>
          <ex:citation>
            <ex:patcit>
              <ex:document-id>
                <ex:country>US</ex:country>
                <ex:doc-number>1111111</ex:doc-number>
                <ex:kind>A1</ex:kind>
              </ex:document-id>
            </ex:patcit>
          </ex:citation>
          <ex:citation>
            <ex:patcit>
              <ex:document-id>
                <ex:country>EP</ex:country>
                <ex:doc-number>2222222</ex:doc-number>
              </ex:document-id>
            </ex:patcit>
          </ex:citation>
        </ex:references-cited>
      </ex:bibliographic-data>
    </ex:family-member>
  </ex:patent-family>
</ops:world-patent-data>"#;
        let ids = EpoOpsSearchBackend::parse_xml_citations_response(xml);
        assert_eq!(ids.len(), 2);
        assert_eq!(ids[0], "US1111111");
        assert_eq!(ids[1], "EP2222222");
    }

    #[test]
    fn epo_ops_parse_xml_citations_response_empty() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ops:world-patent-data xmlns:ops="http://ops.epo.org"></ops:world-patent-data>"#;
        let ids = EpoOpsSearchBackend::parse_xml_citations_response(xml);
        assert!(ids.is_empty());
    }

    #[test]
    fn epo_ops_parse_xml_family_response_basic() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ops:world-patent-data xmlns:ops="http://ops.epo.org" xmlns:ex="http://www.epo.org/exchange">
  <ops:patent-family>
    <ops:family-member>
      <ex:bibliographic-data xmlns:ex="http://www.epo.org/exchange">
        <ex:publication-reference>
          <ex:document-id>
            <ex:country>EP</ex:country>
            <ex:doc-number>1234567</ex:doc-number>
            <ex:kind>A1</ex:kind>
            <ex:date>20200115</ex:date>
          </ex:document-id>
        </ex:publication-reference>
        <ex:invention-title lang="en">Some Title</ex:invention-title>
      </ex:bibliographic-data>
    </ops:family-member>
    <ops:family-member>
      <ex:bibliographic-data xmlns:ex="http://www.epo.org/exchange">
        <ex:publication-reference>
          <ex:document-id>
            <ex:country>US</ex:country>
            <ex:doc-number>7654321</ex:doc-number>
            <ex:kind>B2</ex:kind>
            <ex:date>20210620</ex:date>
          </ex:document-id>
        </ex:publication-reference>
      </ex:bibliographic-data>
    </ops:family-member>
  </ops:patent-family>
</ops:world-patent-data>"#;
        let members = EpoOpsSearchBackend::parse_xml_family_response(xml);
        assert_eq!(members.len(), 2);
        assert_eq!(members[0]["patent_id"].as_str().unwrap(), "EP1234567A1");
        assert_eq!(members[0]["country"].as_str().unwrap(), "EP");
        assert_eq!(members[0]["date"].as_str().unwrap(), "20200115");
        assert_eq!(members[1]["patent_id"].as_str().unwrap(), "US7654321B2");
        assert_eq!(members[1]["country"].as_str().unwrap(), "US");
        assert_eq!(members[1]["date"].as_str().unwrap(), "20210620");
    }

    #[test]
    fn epo_ops_parse_xml_family_response_empty() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ops:world-patent-data xmlns:ops="http://ops.epo.org"></ops:world-patent-data>"#;
        let members = EpoOpsSearchBackend::parse_xml_family_response(xml);
        assert!(members.is_empty());
    }

    #[test]
    fn epo_ops_parse_xml_family_multiple_doc_ids() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ops:world-patent-data xmlns:ops="http://ops.epo.org" xmlns:ex="http://www.epo.org/exchange">
  <ops:patent-family>
    <ops:family-member>
      <ex:bibliographic-data>
        <ex:publication-reference>
          <ex:document-id>
            <ex:country>EP</ex:country>
            <ex:doc-number>1111111</ex:doc-number>
            <ex:kind>A1</ex:kind>
          </ex:document-id>
          <ex:document-id>
            <ex:country>US</ex:country>
            <ex:doc-number>2222222</ex:doc-number>
            <ex:kind>B1</ex:kind>
          </ex:document-id>
        </ex:publication-reference>
      </ex:bibliographic-data>
    </ops:family-member>
  </ops:patent-family>
</ops:world-patent-data>"#;
        let members = EpoOpsSearchBackend::parse_xml_family_response(xml);
        assert_eq!(members.len(), 2);
        assert_eq!(members[0]["patent_id"].as_str().unwrap(), "EP1111111A1");
        assert_eq!(members[1]["patent_id"].as_str().unwrap(), "US2222222B1");
    }

    #[test]
    fn epo_ops_map_doc_array_format_inventors() {
        let doc = serde_json::json!({
            "@doc-number": "12345678",
            "@country": "US",
            "@kind": "B2",
            "bibliographic-data": {
                "invention-title": [{"@lang": "en", "$": "Array Inventors Test"}],
                "publication-reference": {
                    "document-id": {
                        "date": {"$": "20210615"}
                    }
                },
                "parties": {
                    "inventors": {
                        "inventor": [
                            {"inventor-name": {"name": {"$": "Alice Smith"}}},
                            {"inventor-name": {"name": {"$": "Bob Jones"}}}
                        ]
                    },
                    "applicants": {
                        "applicant": [
                            {"applicant-name": {"name": {"$": "Acme Corp"}}}
                        ]
                    }
                }
            }
        });
        let hit = EpoOpsSearchBackend::map_ops_json_doc(&doc).unwrap();
        assert_eq!(hit.patent_id, "US12345678B2");
        assert_eq!(hit.inventors, vec!["Alice Smith", "Bob Jones"]);
        assert_eq!(hit.assignee.as_deref(), Some("Acme Corp"));
    }

    #[test]
    fn epo_ops_map_doc_title_text_fallback() {
        let doc = serde_json::json!({
            "@doc-number": "9999999",
            "@country": "EP",
            "@kind": "A1",
            "bibliographic-data": {
                "invention-title": {"@lang": "de", "#text": "Deutscher Titel"},
                "parties": {}
            }
        });
        let hit = EpoOpsSearchBackend::map_ops_json_doc(&doc).unwrap();
        assert_eq!(hit.title.as_deref(), Some("Deutscher Titel"));
    }

    #[test]
    fn epo_ops_map_doc_date_text_fallback() {
        let doc = serde_json::json!({
            "@doc-number": "5555555",
            "@country": "US",
            "@kind": "A",
            "bibliographic-data": {
                "invention-title": [{"@lang": "en", "$": "Date Text Test"}],
                "publication-reference": {
                    "document-id": {
                        "date": {"#text": "2019-06-15"}
                    }
                },
                "parties": {}
            }
        });
        let hit = EpoOpsSearchBackend::map_ops_json_doc(&doc).unwrap();
        assert_eq!(hit.date.as_deref(), Some("2019-06-15"));
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

    #[test]
    fn test_local_name_strips_namespace() {
        assert_eq!(local_name(b"ex:country"), "country");
        assert_eq!(local_name(b"country"), "country");
        assert_eq!(local_name(b"ops:world-patent-data"), "world-patent-data");
    }
}
