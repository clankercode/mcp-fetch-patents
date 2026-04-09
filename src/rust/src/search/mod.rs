//! Patent search module — session management, search backends, and tool implementations.
//!
//! Mirrors the Python `patent_mcp.search` package.

pub mod browser_search;
pub mod orchestrator;
pub mod profile_manager;
pub mod searchers;
pub mod session_manager;

pub struct BrowserBackendConfig {
    pub profiles_dir: Option<std::path::PathBuf>,
    pub profile_name: String,
    pub headless: bool,
    pub timeout_ms: u32,
    pub max_pages: u32,
    pub debug_html_dir: Option<std::path::PathBuf>,
}

pub struct SearchBackends {
    pub serpapi: Option<searchers::SerpApiGooglePatentsBackend>,
    pub uspto: searchers::UsptoTextSearchBackend,
    pub epo: searchers::EpoOpsSearchBackend,
    pub session_manager: session_manager::SessionManager,
    pub browser_config: BrowserBackendConfig,
}

impl SearchBackends {
    pub fn new(
        config: &crate::config::PatentConfig,
        epo_session_cache: Option<std::sync::Arc<crate::cache::SessionCache>>,
    ) -> Self {
        Self::with_sessions_dir(config, epo_session_cache, None)
    }

    pub fn with_sessions_dir(
        config: &crate::config::PatentConfig,
        epo_session_cache: Option<std::sync::Arc<crate::cache::SessionCache>>,
        sessions_dir: Option<std::path::PathBuf>,
    ) -> Self {
        SearchBackends {
            serpapi: config.serpapi_key.as_ref().map(|key| {
                searchers::SerpApiGooglePatentsBackend::new(
                    key.clone(),
                    None,
                    std::time::Duration::from_secs_f64(config.timeout_secs),
                    None,
                )
            }),
            uspto: searchers::UsptoTextSearchBackend::new(
                None,
                std::time::Duration::from_secs_f64(config.timeout_secs),
                None,
            ),
            epo: searchers::EpoOpsSearchBackend::new(
                config.epo_client_id.clone(),
                config.epo_client_secret.clone(),
                None,
                std::time::Duration::from_secs_f64(config.timeout_secs),
                None,
                epo_session_cache,
            ),
            session_manager: session_manager::SessionManager::new(sessions_dir),
            browser_config: BrowserBackendConfig {
                profiles_dir: config.search_browser_profiles_dir.clone(),
                profile_name: config.search_browser_default_profile.clone(),
                headless: config.search_browser_headless,
                timeout_ms: (config.search_browser_timeout * 1000.0) as u32,
                max_pages: config.search_browser_max_pages as u32,
                debug_html_dir: config.search_browser_debug_html_dir.clone(),
            },
        }
    }
}
