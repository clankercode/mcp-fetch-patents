use std::path::Path;

use async_trait::async_trait;

use crate::cache::PatentMetadata;
use crate::fetchers::PatentSource;
use crate::id_canon::CanonicalPatentId;

use super::{fail_result, now_iso, FetchResult};

pub struct BigQuerySource;

#[async_trait]
impl PatentSource for BigQuerySource {
    fn source_name(&self) -> &str {
        "BigQuery"
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
        let source = self.source_name();

        let _project = match &config.bigquery_project {
            Some(p) if !p.is_empty() => p.clone(),
            _ => {
                let mut res = fail_result(source, "BigQuery not configured: no project");
                res.source_attempt.elapsed_ms = crate::elapsed_ms(start);
                return res;
            }
        };

        let pub_number = format!("{}-{}", patent.jurisdiction, patent.number);

        let script = r#"
import json, sys
try:
    project = sys.argv[1]
    pub_number = sys.argv[2]
    from google.cloud import bigquery
    client = bigquery.Client(project=project)
    query = f"""SELECT
  publication_number,
  title_localized,
  abstract_localized,
  inventor_harmonized,
  assignee_harmonized,
  filing_date,
  publication_date,
  grant_date
FROM `patents-public-data.patents.publications`
WHERE publication_number LIKE @pub_number
LIMIT 5"""
    job_config = bigquery.QueryJobConfig(
        query_parameters=[
            bigquery.ScalarQueryParameter("pub_number", "STRING", pub_number + "%"),
        ]
    )
    rows = list(client.query(query, job_config=job_config).result())
    if not rows:
        print(json.dumps({"error": "not_found"}))
        sys.exit(0)
    row = dict(rows[0])
    for k, v in row.items():
        if hasattr(v, 'isoformat'):
            row[k] = v.isoformat()
        elif isinstance(v, (list, tuple)):
            row[k] = [dict(i) if hasattr(i, 'items') else i for i in v]
    print(json.dumps(row))
except ImportError:
    print(json.dumps({"error": "google-cloud-bigquery not installed"}))
except Exception as e:
    print(json.dumps({"error": str(e)}))
"#;

        let result = tokio::task::spawn_blocking(move || {
            std::process::Command::new("python3")
                .args(["-c", script, &_project, &pub_number])
                .output()
        })
        .await;

        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let json_line = stdout
                    .lines()
                    .rfind(|l| l.trim_start().starts_with('{'))
                    .unwrap_or("");

                match serde_json::from_str::<serde_json::Value>(json_line) {
                    Ok(data) => {
                        if let Some(err) = data.get("error").and_then(|v| v.as_str()) {
                            let mut res = fail_result(source, err);
                            res.source_attempt.elapsed_ms = crate::elapsed_ms(start);
                            return res;
                        }

                        let title = data
                            .get("title_localized")
                            .and_then(|v| v.as_array())
                            .and_then(|arr| {
                                arr.iter()
                                    .find(|t| {
                                        t.get("language")
                                            .and_then(|l| l.as_str())
                                            .map(|l| l == "en")
                                            .unwrap_or(false)
                                    })
                                    .or_else(|| arr.first())
                            })
                            .and_then(|t| t.get("text").and_then(|v| v.as_str()))
                            .map(String::from);

                        let abstract_text = data
                            .get("abstract_localized")
                            .and_then(|v| v.as_array())
                            .and_then(|arr| {
                                arr.iter().find(|a| {
                                    a.get("language")
                                        .and_then(|l| l.as_str())
                                        .map(|l| l == "en")
                                        .unwrap_or(false)
                                })
                            })
                            .and_then(|a| a.get("text").and_then(|v| v.as_str()))
                            .map(String::from);

                        let inventors: Vec<String> = data
                            .get("inventor_harmonized")
                            .and_then(|v| v.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|i| {
                                        i.get("name").and_then(|n| n.as_str()).map(String::from)
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();

                        let assignee = data
                            .get("assignee_harmonized")
                            .and_then(|v| v.as_array())
                            .and_then(|arr| arr.first())
                            .and_then(|a| a.get("name").and_then(|n| n.as_str()))
                            .map(String::from);

                        fn parse_bq_date(v: &serde_json::Value) -> Option<String> {
                            let s = v.as_str().or_else(|| v.as_i64().map(|_| ""))?;
                            if s.is_empty() {
                                return v.as_i64().map(|n| {
                                    let ns = n.to_string();
                                    if ns.len() == 8 {
                                        format!("{}-{}-{}", &ns[..4], &ns[4..6], &ns[6..])
                                    } else {
                                        ns
                                    }
                                });
                            }
                            if s.len() == 8 && s.chars().all(|c| c.is_ascii_digit()) {
                                Some(format!("{}-{}-{}", &s[..4], &s[4..6], &s[6..]))
                            } else {
                                Some(s.to_string())
                            }
                        }

                        let filing_date = data.get("filing_date").and_then(parse_bq_date);
                        let publication_date = data.get("publication_date").and_then(parse_bq_date);
                        let grant_date = data.get("grant_date").and_then(parse_bq_date);

                        let meta = PatentMetadata {
                            canonical_id: patent.canonical.clone(),
                            jurisdiction: patent.jurisdiction.clone(),
                            doc_type: patent.doc_type.clone(),
                            title,
                            abstract_text,
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
                    Err(e) => {
                        let mut res =
                            fail_result(source, &format!("Failed to parse BigQuery output: {}", e));
                        res.source_attempt.elapsed_ms = crate::elapsed_ms(start);
                        res
                    }
                }
            }
            Ok(Err(e)) => {
                let mut res = fail_result(source, &format!("Python subprocess error: {}", e));
                res.source_attempt.elapsed_ms = crate::elapsed_ms(start);
                res
            }
            Err(e) => {
                let mut res = fail_result(source, &format!("Spawn error: {}", e));
                res.source_attempt.elapsed_ms = crate::elapsed_ms(start);
                res
            }
        }
    }
}
