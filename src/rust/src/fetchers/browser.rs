//! Native Google Patents fetcher using HTML + JSON-LD.

use std::path::Path;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use reqwest::Client;
use scraper::{Html, Selector};

use crate::cache::{PatentMetadata, SourceAttempt};
use crate::config::PatentConfig;
use crate::fetchers::{FetchResult, PatentSource};
use crate::id_canon::CanonicalPatentId;

const DEFAULT_USER_AGENT: &str = concat!(
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 ",
    "(KHTML, like Gecko) Chrome/123.0.0.0 Safari/537.36 patent-mcp-server/0.1"
);

pub struct BrowserSource;

#[async_trait]
impl PatentSource for BrowserSource {
    fn source_name(&self) -> &str {
        "Google_Patents"
    }

    fn supported_jurisdictions(&self) -> &[&str] {
        &[]
    }

    async fn fetch(
        &self,
        patent: &CanonicalPatentId,
        _output_dir: &Path,
        config: &PatentConfig,
    ) -> FetchResult {
        let start = Instant::now();
        let url = format!("https://patents.google.com/patent/{}/en", patent.canonical);
        let client = match Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs as u64))
            .redirect(reqwest::redirect::Policy::limited(10))
            .user_agent(DEFAULT_USER_AGENT)
            .build()
        {
            Ok(client) => client,
            Err(error) => {
                return FetchResult {
                    source_attempt: SourceAttempt {
                        source: "Google_Patents".to_string(),
                        success: false,
                        elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
                        error: Some(format!("Client build error: {}", error)),
                        metadata: None,
                    },
                    pdf_path: None,
                    txt_path: None,
                    metadata: None,
                };
            }
        };

        let response = match client
            .get(&url)
            .header("Accept-Language", "en")
            .send()
            .await
        {
            Ok(response) => response,
            Err(error) => {
                return FetchResult {
                    source_attempt: SourceAttempt {
                        source: "Google_Patents".to_string(),
                        success: false,
                        elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
                        error: Some(error.to_string()),
                        metadata: None,
                    },
                    pdf_path: None,
                    txt_path: None,
                    metadata: None,
                };
            }
        };

        if response.status().as_u16() == 404 {
            return FetchResult {
                source_attempt: SourceAttempt {
                    source: "Google_Patents".to_string(),
                    success: false,
                    elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
                    error: Some("not_found".to_string()),
                    metadata: None,
                },
                pdf_path: None,
                txt_path: None,
                metadata: None,
            };
        }

        let html = match response.text().await {
            Ok(html) => html,
            Err(error) => {
                return FetchResult {
                    source_attempt: SourceAttempt {
                        source: "Google_Patents".to_string(),
                        success: false,
                        elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
                        error: Some(error.to_string()),
                        metadata: None,
                    },
                    pdf_path: None,
                    txt_path: None,
                    metadata: None,
                };
            }
        };

        let metadata = match parse_google_patents_metadata(&html, patent) {
            Some(metadata) => metadata,
            None => {
                return FetchResult {
                    source_attempt: SourceAttempt {
                        source: "Google_Patents".to_string(),
                        success: false,
                        elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
                        error: Some("Google Patents returned no usable metadata".to_string()),
                        metadata: None,
                    },
                    pdf_path: None,
                    txt_path: None,
                    metadata: None,
                };
            }
        };

        FetchResult {
            source_attempt: SourceAttempt {
                source: "Google_Patents".to_string(),
                success: true,
                elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
                error: None,
                metadata: None,
            },
            pdf_path: None,
            txt_path: None,
            metadata: Some(metadata),
        }
    }
}

fn parse_google_patents_metadata(html: &str, patent: &CanonicalPatentId) -> Option<PatentMetadata> {
    let document = Html::parse_document(html);

    let title = text_content(&document, r#"[itemprop="title"]"#)
        .or_else(|| meta_content(&document, r#"meta[name="DC.title"]"#));

    let abstract_text = text_content(&document, r#"section[itemprop="abstract"] .abstract"#)
        .or_else(|| meta_content(&document, r#"meta[name="DC.description"]"#))
        .or_else(|| meta_content(&document, r#"meta[name="description"]"#));

    let inventors = text_contents(&document, r#"[itemprop="inventor"]"#);

    let assignee = text_content(&document, r#"[itemprop="assigneeCurrent"]"#)
        .or_else(|| text_content(&document, r#"[itemprop="assigneeOriginal"]"#))
        .or_else(|| meta_content_with_attr(
            &document,
            r#"meta[name="DC.contributor"][scheme="assignee"]"#,
            "content",
        ));

    let filing_date = datetime_attr(&document, r#"time[itemprop="filingDate"]"#)
        .or_else(|| meta_content_with_attr(
            &document,
            r#"meta[name="DC.date"][scheme="dateSubmitted"]"#,
            "content",
        ));

    let publication_date = datetime_attr(&document, r#"time[itemprop="publicationDate"]"#)
        .or_else(|| meta_content_with_attr(
            &document,
            r#"meta[name="DC.date"][scheme="issue"]"#,
            "content",
        ));

    let legal_status = text_content(&document, r#"[itemprop="legalStatusIfi"] [itemprop="status"]"#);

    if title.is_none()
        && abstract_text.is_none()
        && assignee.is_none()
        && filing_date.is_none()
        && publication_date.is_none()
        && inventors.is_empty()
    {
        return None;
    }

    Some(PatentMetadata {
        canonical_id: patent.canonical.clone(),
        jurisdiction: patent.jurisdiction.clone(),
        doc_type: patent.doc_type.clone(),
        title,
        abstract_text,
        inventors,
        assignee,
        filing_date,
        publication_date,
        grant_date: None,
        fetched_at: chrono::Utc::now().to_rfc3339(),
        legal_status,
    })
}

fn selector(query: &str) -> Option<Selector> {
    Selector::parse(query).ok()
}

fn text_content(document: &Html, query: &str) -> Option<String> {
    let selector = selector(query)?;
    document
        .select(&selector)
        .next()
        .map(|el| el.text().collect::<String>().trim().to_string())
        .filter(|text| !text.is_empty())
}

fn text_contents(document: &Html, query: &str) -> Vec<String> {
    let Some(selector) = selector(query) else {
        return Vec::new();
    };
    document
        .select(&selector)
        .map(|el| el.text().collect::<String>().trim().to_string())
        .filter(|text| !text.is_empty())
        .collect()
}

fn meta_content(document: &Html, query: &str) -> Option<String> {
    meta_content_with_attr(document, query, "content")
}

fn meta_content_with_attr(document: &Html, query: &str, attr: &str) -> Option<String> {
    let selector = selector(query)?;
    document
        .select(&selector)
        .next()
        .and_then(|el| el.value().attr(attr))
        .map(str::trim)
        .map(str::to_string)
        .filter(|text| !text.is_empty())
}

fn datetime_attr(document: &Html, query: &str) -> Option<String> {
    meta_content_with_attr(document, query, "datetime")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_google_patents_metadata_from_microdata() {
        let html = r#"
        <html><head>
        <meta name="DC.title" content="Test Patent">
        <meta name="description" content="Test abstract">
        <meta name="citation_pdf_url" content="https://example.com/test.pdf">
        </head></html>
        <body>
          <span itemprop="title">Test Patent</span>
          <section itemprop="abstract"><div class="abstract">Test abstract</div></section>
          <dd itemprop="inventor" repeat>Ada Lovelace</dd>
          <dd itemprop="inventor" repeat>Grace Hopper</dd>
          <dd itemprop="assigneeCurrent" repeat>Test Corp</dd>
          <dd><time itemprop="filingDate" datetime="2020-01-01">2020-01-01</time></dd>
          <dd><time itemprop="publicationDate" datetime="2021-01-01">2021-01-01</time></dd>
          <dd itemprop="legalStatusIfi"><span itemprop="status">Active</span></dd>
          <a itemprop="pdfLink" href="https://example.com/test.pdf">Download PDF</a>
        </body>
        "#;
        let patent = crate::id_canon::canonicalize("US7654321");

        let metadata = parse_google_patents_metadata(html, &patent).unwrap();
        assert_eq!(metadata.title.as_deref(), Some("Test Patent"));
        assert_eq!(metadata.abstract_text.as_deref(), Some("Test abstract"));
        assert_eq!(metadata.assignee.as_deref(), Some("Test Corp"));
        assert_eq!(metadata.filing_date.as_deref(), Some("2020-01-01"));
        assert_eq!(metadata.publication_date.as_deref(), Some("2021-01-01"));
        assert_eq!(metadata.inventors, vec!["Ada Lovelace", "Grace Hopper"]);
        assert_eq!(metadata.legal_status.as_deref(), Some("Active"));
    }

    #[test]
    fn test_parse_google_patents_metadata_requires_useful_fields() {
        let html = r#"<script type="application/ld+json">{"foo":"bar"}</script>"#;
        let patent = crate::id_canon::canonicalize("US7654321");
        assert!(parse_google_patents_metadata(html, &patent).is_none());
    }
}
