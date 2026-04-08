//! BrowserSource — calls the Python `playwright_runner` (google_patents scraper)
//! via subprocess with JSON stdout protocol.

use std::path::{Path, PathBuf};
use std::time::Instant;

use async_trait::async_trait;
use serde::Deserialize;
use tracing::warn;

use crate::cache::{PatentMetadata, SourceAttempt};
use crate::config::PatentConfig;
use crate::fetchers::{FetchResult, PatentSource};
use crate::id_canon::CanonicalPatentId;

/// JSON shape returned by `python3 -m patent_mcp.scrapers.google_patents`.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct BrowserFetchResultJson {
    canonical_id: String,
    success: bool,
    source: Option<String>,
    pdf_path: Option<String>,
    txt_path: Option<String>,
    title: Option<String>,
    #[serde(alias = "abstract")]
    abstract_text: Option<String>,
    #[serde(default)]
    inventors: Vec<String>,
    assignee: Option<String>,
    filing_date: Option<String>,
    publication_date: Option<String>,
    elapsed_ms: f64,
    error: Option<String>,
}

/// Fetches patent data via the Python Playwright-based Google Patents scraper.
pub struct BrowserSource;

impl BrowserSource {
    /// Run the Python Google Patents scraper as a subprocess and parse the result.
    async fn fetch_inner(
        &self,
        patent: &CanonicalPatentId,
        output_dir: &Path,
        config: &PatentConfig,
    ) -> FetchResult {
        let canon_id = patent.canonical.clone();
        let out_dir = output_dir.to_path_buf();
        let timeout_secs = config.timeout_secs;

        let start = Instant::now();

        let result = tokio::task::spawn_blocking(move || {
            run_browser_subprocess(&canon_id, &out_dir, timeout_secs)
        })
        .await;

        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;

        match result {
            Ok(Ok(json_result)) => map_json_to_fetch_result(json_result, patent),
            Ok(Err(err)) => {
                warn!(
                    patent = %patent.canonical,
                    error = %err,
                    "BrowserSource subprocess failed"
                );
                FetchResult {
                    source_attempt: SourceAttempt {
                        source: "Google_Patents".to_string(),
                        success: false,
                        elapsed_ms,
                        error: Some(err),
                        metadata: None,
                    },
                    pdf_path: None,
                    txt_path: None,
                    metadata: None,
                }
            }
            Err(join_err) => {
                warn!(
                    patent = %patent.canonical,
                    error = %join_err,
                    "BrowserSource spawn_blocking panicked"
                );
                FetchResult {
                    source_attempt: SourceAttempt {
                        source: "Google_Patents".to_string(),
                        success: false,
                        elapsed_ms,
                        error: Some(format!("Task join error: {}", join_err)),
                        metadata: None,
                    },
                    pdf_path: None,
                    txt_path: None,
                    metadata: None,
                }
            }
        }
    }
}

#[async_trait]
impl PatentSource for BrowserSource {
    fn source_name(&self) -> &str {
        "Google_Patents"
    }

    fn supported_jurisdictions(&self) -> &[&str] {
        &[] // handles all jurisdictions
    }

    async fn fetch(
        &self,
        patent: &CanonicalPatentId,
        output_dir: &Path,
        config: &PatentConfig,
    ) -> FetchResult {
        self.fetch_inner(patent, output_dir, config).await
    }
}

/// Runs the Python subprocess synchronously (called inside `spawn_blocking`).
fn run_browser_subprocess(
    canonical_id: &str,
    output_dir: &Path,
    timeout_secs: f64,
) -> Result<BrowserFetchResultJson, String> {
    let output = std::process::Command::new("python3")
        .args([
            "-m",
            "patent_mcp.scrapers.google_patents",
            canonical_id,
            &output_dir.to_string_lossy(),
        ])
        .env("PATENT_TIMEOUT", format!("{}", timeout_secs))
        .output()
        .map_err(|e| format!("Failed to spawn python3: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "Browser subprocess exited with {}: {}",
            output.status,
            stderr.trim()
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Python may emit log lines before the JSON; find the last line starting with '{'
    let json_line = stdout
        .lines()
        .rfind(|l| l.trim_start().starts_with('{'))
        .ok_or_else(|| {
            let stderr = String::from_utf8_lossy(&output.stderr);
            format!(
                "No JSON line found in subprocess stdout. stderr: {}",
                stderr.trim()
            )
        })?;

    serde_json::from_str::<BrowserFetchResultJson>(json_line).map_err(|e| {
        format!(
            "Failed to parse browser JSON output: {} — line: {}",
            e, json_line
        )
    })
}

/// Maps a successfully parsed `BrowserFetchResultJson` to a `FetchResult`.
fn map_json_to_fetch_result(
    json: BrowserFetchResultJson,
    patent: &CanonicalPatentId,
) -> FetchResult {
    let metadata = if json.title.is_some()
        || json.abstract_text.is_some()
        || !json.inventors.is_empty()
        || json.assignee.is_some()
    {
        Some(PatentMetadata {
            canonical_id: patent.canonical.clone(),
            jurisdiction: patent.jurisdiction.clone(),
            doc_type: patent.doc_type.clone(),
            title: json.title,
            abstract_text: json.abstract_text,
            inventors: json.inventors,
            assignee: json.assignee,
            filing_date: json.filing_date,
            publication_date: json.publication_date,
            grant_date: None,
            fetched_at: chrono::Utc::now().to_rfc3339(),
            legal_status: None,
        })
    } else {
        None
    };

    FetchResult {
        source_attempt: SourceAttempt {
            source: "Google_Patents".to_string(),
            success: json.success,
            elapsed_ms: json.elapsed_ms,
            error: json.error,
            metadata: None,
        },
        pdf_path: json.pdf_path.map(PathBuf::from),
        txt_path: json.txt_path.map(PathBuf::from),
        metadata,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_browser_result_json() {
        let json = r#"{"canonical_id":"US7654321","success":true,"source":"google_patents","pdf_path":null,"txt_path":null,"title":"Test Patent","abstract":"Test abstract","inventors":["Alice","Bob"],"assignee":"Test Corp","filing_date":"2024-01-01","publication_date":"2024-06-01","elapsed_ms":1234.5,"error":null}"#;
        let parsed: BrowserFetchResultJson = serde_json::from_str(json).unwrap();
        assert!(parsed.success);
        assert_eq!(parsed.title, Some("Test Patent".to_string()));
        assert_eq!(parsed.inventors, vec!["Alice", "Bob"]);
    }

    #[test]
    fn test_parse_browser_result_failure() {
        let json = r#"{"canonical_id":"US9999999","success":false,"source":"google_patents","pdf_path":null,"txt_path":null,"title":null,"abstract":null,"inventors":[],"assignee":null,"filing_date":null,"publication_date":null,"elapsed_ms":500.0,"error":"Playwright not installed"}"#;
        let parsed: BrowserFetchResultJson = serde_json::from_str(json).unwrap();
        assert!(!parsed.success);
        assert_eq!(parsed.error, Some("Playwright not installed".to_string()));
    }

    #[test]
    fn test_extract_json_from_mixed_output() {
        // Python may emit log lines before the JSON
        let output = "INFO: Starting browser...\nWARNING: slow connection\n{\"canonical_id\":\"US1234\",\"success\":true,\"source\":\"google_patents\",\"pdf_path\":null,\"txt_path\":null,\"title\":\"T\",\"abstract\":null,\"inventors\":[],\"assignee\":null,\"filing_date\":null,\"publication_date\":null,\"elapsed_ms\":0.0,\"error\":null}";
        let json_line = output
            .lines()
            .rfind(|l| l.trim_start().starts_with('{'))
            .unwrap();
        let parsed: BrowserFetchResultJson = serde_json::from_str(json_line).unwrap();
        assert!(parsed.success);
    }
}
