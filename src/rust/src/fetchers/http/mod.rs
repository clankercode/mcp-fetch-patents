use chrono::Utc;

use crate::cache::PatentMetadata;
use crate::config::PatentConfig;
use crate::fetchers::FetchResult;

mod bigquery;
mod cipo;
mod epo_ops;
mod espacenet;
mod ip_australia;
mod uspto_ppubs;
mod wipo;

pub use bigquery::BigQuerySource;
pub use cipo::CipoScrapeSource;
pub use epo_ops::EpoOpsSource;
pub use espacenet::EspacenetSource;
pub use ip_australia::IpAustraliaSource;
pub use uspto_ppubs::PpubsSource;
pub use wipo::WipoScrapeSource;

fn now_iso() -> String {
    Utc::now().to_rfc3339()
}

fn base_url(config: &PatentConfig, key: &str, default: &str) -> String {
    config
        .source_base_urls
        .get(key)
        .cloned()
        .unwrap_or_else(|| default.to_string())
}

fn fail_result(source: &str, error: &str) -> FetchResult {
    FetchResult {
        source_attempt: crate::cache::SourceAttempt {
            source: source.into(),
            success: false,
            elapsed_ms: 0.0,
            error: Some(error.into()),
            metadata: None,
        },
        pdf_path: None,
        txt_path: None,
        metadata: None,
    }
}

fn metadata_has_useful_fields(meta: &PatentMetadata) -> bool {
    meta.title.is_some()
        || meta.abstract_text.is_some()
        || !meta.inventors.is_empty()
        || meta.assignee.is_some()
        || meta.filing_date.is_some()
        || meta.publication_date.is_some()
        || meta.grant_date.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::Arc;

    use reqwest::Client;

    use crate::cache::SessionCache;
    use crate::fetchers::PatentSource;

    fn test_config() -> PatentConfig {
        PatentConfig {
            cache_local_dir: std::path::PathBuf::from("/tmp/patent-test"),
            cache_global_db: std::path::PathBuf::from("/tmp/patent-test/global.db"),
            source_priority: vec![],
            concurrency: 5,
            fetch_all_sources: false,
            timeout_secs: 30.0,
            converters_order: vec![],
            converters_disabled: vec![],
            source_base_urls: HashMap::new(),
            epo_client_id: None,
            epo_client_secret: None,
            lens_api_key: None,
            serpapi_key: None,
            bing_key: None,
            bigquery_project: None,
            activity_journal: None,
            search_browser_profiles_dir: None,
            search_browser_default_profile: "default".into(),
            search_browser_headless: true,
            search_browser_timeout: 60.0,
            search_browser_max_pages: 3,
            search_browser_idle_timeout: 1800.0,
            search_browser_debug_html_dir: None,
            search_backend_default: "browser".into(),
            search_enrich_top_n: 5,
        }
    }

    fn test_client() -> Arc<Client> {
        Arc::new(Client::builder().build().unwrap())
    }

    #[test]
    fn test_espacenet_can_fetch_all_jurisdictions() {
        let source = EspacenetSource {
            client: test_client(),
        };
        let us_patent = crate::id_canon::canonicalize("US7654321");
        let ep_patent = crate::id_canon::canonicalize("EP1234567");
        assert!(source.can_fetch(&us_patent));
        assert!(source.can_fetch(&ep_patent));
    }

    #[test]
    fn test_wipo_only_handles_wo() {
        let source = WipoScrapeSource {
            client: test_client(),
        };
        let wo_patent = crate::id_canon::canonicalize("WO2024123456");
        let us_patent = crate::id_canon::canonicalize("US7654321");
        assert!(source.can_fetch(&wo_patent));
        assert!(!source.can_fetch(&us_patent));
    }

    #[test]
    fn test_ip_australia_only_handles_au() {
        let source = IpAustraliaSource {
            client: test_client(),
        };
        let au_patent = crate::id_canon::canonicalize("AU2023123456");
        let us_patent = crate::id_canon::canonicalize("US7654321");
        assert!(source.can_fetch(&au_patent));
        assert!(!source.can_fetch(&us_patent));
    }

    #[test]
    fn test_cipo_only_handles_ca() {
        let source = CipoScrapeSource {
            client: test_client(),
        };
        let ca_patent = crate::id_canon::canonicalize("CA1234567");
        let us_patent = crate::id_canon::canonicalize("US7654321");
        assert!(source.can_fetch(&ca_patent));
        assert!(!source.can_fetch(&us_patent));
    }

    #[test]
    fn test_epo_ops_can_fetch_all() {
        let source = EpoOpsSource {
            session_cache: Arc::new(SessionCache::new()),
            client: test_client(),
        };
        let us_patent = crate::id_canon::canonicalize("US7654321");
        assert!(source.can_fetch(&us_patent));
    }

    #[test]
    fn test_ppubs_only_handles_us() {
        let source = PpubsSource {
            session_cache: Arc::new(SessionCache::new()),
            client: test_client(),
        };
        let us_patent = crate::id_canon::canonicalize("US7654321");
        let ep_patent = crate::id_canon::canonicalize("EP1234567");
        assert!(source.can_fetch(&us_patent));
        assert!(!source.can_fetch(&ep_patent));
    }

    #[test]
    fn test_bigquery_no_project_returns_error() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let source = BigQuerySource;
            let patent = crate::id_canon::canonicalize("US7654321");
            let config = test_config();
            let result = source.fetch(&patent, Path::new("/tmp"), &config).await;
            assert!(!result.source_attempt.success);
            assert!(result
                .source_attempt
                .error
                .as_ref()
                .unwrap()
                .contains("not configured"));
        });
    }

    #[test]
    fn test_now_iso_format() {
        let ts = now_iso();
        assert!(ts.contains("T"));
        assert!(ts.len() > 10);
    }

    #[test]
    fn test_metadata_has_useful_fields_requires_real_content() {
        let empty = PatentMetadata {
            canonical_id: "US7654321".into(),
            jurisdiction: "US".into(),
            doc_type: "patent".into(),
            title: None,
            abstract_text: None,
            inventors: vec![],
            assignee: None,
            filing_date: None,
            publication_date: None,
            grant_date: None,
            fetched_at: now_iso(),
            legal_status: None,
        };
        assert!(!metadata_has_useful_fields(&empty));

        let titled = PatentMetadata {
            title: Some("Useful title".into()),
            ..empty
        };
        assert!(metadata_has_useful_fields(&titled));
    }
}
