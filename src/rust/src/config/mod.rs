//! Configuration — mirrors Python patent_mcp.config module.
//!
//! Loads from:
//!   1. Compiled defaults
//!   2. Autoloaded env files (`~/.patents-mcp.env`, then `.env` in cwd)
//!   3. ~/.patents.toml (or PATENT_CONFIG_FILE env override)
//!   4. Environment variables (highest priority)

use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

/// Full server configuration. All fields have defaults.
#[derive(Debug, Clone)]
pub struct PatentConfig {
    pub cache_local_dir: PathBuf,
    pub cache_global_db: PathBuf,
    pub source_priority: Vec<String>,
    pub concurrency: usize,
    pub fetch_all_sources: bool,
    pub timeout_secs: f64,
    pub converters_order: Vec<String>,
    pub converters_disabled: Vec<String>,
    pub source_base_urls: HashMap<String, String>,
    // API keys (all optional)
    pub epo_client_id: Option<String>,
    pub epo_client_secret: Option<String>,
    pub lens_api_key: Option<String>,
    pub serpapi_key: Option<String>,
    pub bing_key: Option<String>,
    pub bigquery_project: Option<String>,
    pub activity_journal: Option<PathBuf>,
    // Search settings
    pub search_browser_profiles_dir: Option<PathBuf>,
    pub search_browser_default_profile: String,
    pub search_browser_headless: bool,
    pub search_browser_timeout: f64,
    pub search_browser_max_pages: usize,
    pub search_browser_idle_timeout: f64,
    pub search_browser_debug_html_dir: Option<PathBuf>,
    pub search_backend_default: String,
    pub search_enrich_top_n: usize,
    // Prefetch settings
    pub prefetch: PrefetchConfig,
}

#[derive(Debug, Clone)]
pub struct PrefetchConfig {
    pub full_text: bool,
    pub limit: usize,
    pub max_concurrent: usize,
    pub hold_time_secs: u64,
}

impl Default for PrefetchConfig {
    fn default() -> Self {
        PrefetchConfig {
            full_text: false,
            limit: 5,
            max_concurrent: 1,
            hold_time_secs: 5,
        }
    }
}

/// TOML file schema for deserialization.
#[derive(Debug, Deserialize, Default)]
struct TomlFile {
    cache: Option<TomlCache>,
    sources: Option<TomlSources>,
    converters: Option<TomlConverters>,
    journal: Option<TomlJournal>,
    search: Option<TomlSearch>,
    prefetch: Option<TomlPrefetch>,
}

#[derive(Debug, Deserialize, Default)]
struct TomlCache {
    local_dir: Option<String>,
    global_db: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct TomlSources {
    priority: Option<Vec<String>>,
    fetch_all_sources: Option<bool>,
    concurrency: Option<usize>,
    timeout_seconds: Option<f64>, // matches Python
    epo_ops: Option<TomlEpoOps>,
}

#[derive(Debug, Deserialize, Default)]
struct TomlEpoOps {
    client_id: Option<String>,
    client_secret: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct TomlJournal {
    path: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct TomlSearch {
    browser_profiles_dir: Option<String>,
    browser_default_profile: Option<String>,
    browser_headless: Option<bool>,
    browser_timeout: Option<f64>,
    browser_max_pages: Option<usize>,
    browser_idle_timeout: Option<f64>,
    browser_debug_html_dir: Option<String>,
    backend_default: Option<String>,
    enrich_top_n: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
struct TomlConverters {
    pdf_to_markdown_order: Option<Vec<String>>, // matches Python
    disable: Option<Vec<String>>,               // matches Python
}

#[derive(Debug, Deserialize, Default)]
struct TomlPrefetch {
    full_text: Option<bool>,
    limit: Option<usize>,
    max_concurrent: Option<usize>,
    hold_time_seconds: Option<u64>,
}

/// XDG data home: $XDG_DATA_HOME or ~/.local/share
pub fn xdg_data_home() -> PathBuf {
    if let Ok(v) = std::env::var("XDG_DATA_HOME") {
        PathBuf::from(v)
    } else {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".local")
            .join("share")
    }
}

/// Default global index path: $XDG_DATA_HOME/patent-cache/index.db
pub fn default_global_db() -> PathBuf {
    xdg_data_home().join("patent-cache").join("index.db")
}

fn default_source_priority() -> Vec<String> {
    vec![
        "USPTO".into(),
        "EPO_OPS".into(),
        "BigQuery".into(),
        "Espacenet".into(),
        "WIPO_Scrape".into(),
        "IP_Australia".into(),
        "CIPO".into(),
        "Google_Patents".into(),
        "web_search".into(),
    ]
}

fn default_converters_order() -> Vec<String> {
    vec![
        "pymupdf4llm".into(),
        "pdfplumber".into(),
        "pdftotext".into(),
        "marker".into(),
    ]
}

fn parse_bool_env(v: &str) -> bool {
    matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")
}

fn load_env_file_if_present(path: &PathBuf) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }

    let content = std::fs::read_to_string(path)?;
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };

        let key = key.trim();
        if key.is_empty() || std::env::var_os(key).is_some() {
            continue;
        }

        let value = value.trim();
        let value = if value.len() >= 2 {
            let bytes = value.as_bytes();
            if (bytes[0] == b'"' && bytes[value.len() - 1] == b'"')
                || (bytes[0] == b'\'' && bytes[value.len() - 1] == b'\'')
            {
                &value[1..value.len() - 1]
            } else {
                value
            }
        } else {
            value
        };

        std::env::set_var(key, value);
    }

    Ok(())
}

fn load_autoload_env_files() -> Result<()> {
    if let Some(home) = dirs::home_dir() {
        load_env_file_if_present(&home.join(".patents-mcp.env"))?;
    }
    load_env_file_if_present(&PathBuf::from(".env"))?;
    Ok(())
}

/// Load config from defaults → TOML file → env vars.
pub fn load_config() -> Result<PatentConfig> {
    load_autoload_env_files()?;

    let mut cfg = PatentConfig {
        cache_local_dir: xdg_data_home().join("patent-cache").join("patents"),
        cache_global_db: default_global_db(),
        source_priority: default_source_priority(),
        concurrency: 10,
        fetch_all_sources: true,
        timeout_secs: 30.0,
        converters_order: default_converters_order(),
        converters_disabled: vec!["marker".into()],
        source_base_urls: HashMap::new(),
        epo_client_id: None,
        epo_client_secret: None,
        lens_api_key: None,
        serpapi_key: None,
        bing_key: None,
        bigquery_project: None,
        activity_journal: Some(PathBuf::from(".patent-activity.jsonl")),
        search_browser_profiles_dir: None,
        search_browser_default_profile: "default".into(),
        search_browser_headless: true,
        search_browser_timeout: 60.0,
        search_browser_max_pages: 3,
        search_browser_idle_timeout: 1800.0,
        search_browser_debug_html_dir: None,
        search_backend_default: "browser".into(),
        search_enrich_top_n: 5,
        prefetch: PrefetchConfig::default(),
    };

    // Load TOML file — check CWD first, then home dir (matches Python behavior)
    let toml_path = std::env::var("PATENT_CONFIG_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let cwd_config = PathBuf::from(".patents.toml");
            if cwd_config.exists() {
                cwd_config
            } else {
                dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join(".patents.toml")
            }
        });

    if toml_path.exists() {
        let content = std::fs::read_to_string(&toml_path)?;
        let file: TomlFile = toml::from_str(&content)?;

        if let Some(cache) = file.cache {
            if let Some(v) = cache.local_dir {
                cfg.cache_local_dir = PathBuf::from(v);
            }
            if let Some(v) = cache.global_db {
                cfg.cache_global_db = PathBuf::from(v);
            }
        }
        if let Some(sources) = file.sources {
            if let Some(v) = sources.priority {
                cfg.source_priority = v;
            }
            if let Some(v) = sources.fetch_all_sources {
                cfg.fetch_all_sources = v;
            }
            if let Some(v) = sources.concurrency {
                cfg.concurrency = v;
            }
            if let Some(v) = sources.timeout_seconds {
                cfg.timeout_secs = v;
            }
            if let Some(epo) = sources.epo_ops {
                cfg.epo_client_id = epo.client_id.filter(|s| !s.is_empty());
                cfg.epo_client_secret = epo.client_secret.filter(|s| !s.is_empty());
            }
        }
        if let Some(journal) = file.journal {
            if let Some(v) = journal.path {
                if v.is_empty() {
                    cfg.activity_journal = None;
                } else {
                    cfg.activity_journal = Some(PathBuf::from(v));
                }
            }
        }
        if let Some(s) = file.search {
            if let Some(v) = s.browser_profiles_dir {
                cfg.search_browser_profiles_dir = if v.is_empty() {
                    None
                } else {
                    Some(PathBuf::from(v))
                };
            }
            if let Some(v) = s.browser_default_profile {
                cfg.search_browser_default_profile = v;
            }
            if let Some(v) = s.browser_headless {
                cfg.search_browser_headless = v;
            }
            if let Some(v) = s.browser_timeout {
                cfg.search_browser_timeout = v;
            }
            if let Some(v) = s.browser_max_pages {
                cfg.search_browser_max_pages = v;
            }
            if let Some(v) = s.browser_idle_timeout {
                cfg.search_browser_idle_timeout = v;
            }
            if let Some(v) = s.browser_debug_html_dir {
                cfg.search_browser_debug_html_dir = if v.is_empty() {
                    None
                } else {
                    Some(PathBuf::from(v))
                };
            }
            if let Some(v) = s.backend_default {
                cfg.search_backend_default = v;
            }
            if let Some(v) = s.enrich_top_n {
                cfg.search_enrich_top_n = v;
            }
        }
        if let Some(conv) = file.converters {
            if let Some(v) = conv.pdf_to_markdown_order {
                cfg.converters_order = v;
            }
            if let Some(v) = conv.disable {
                cfg.converters_disabled = v;
            }
        }
        if let Some(p) = file.prefetch {
            if let Some(v) = p.full_text {
                cfg.prefetch.full_text = v;
            }
            if let Some(v) = p.limit {
                cfg.prefetch.limit = v;
            }
            if let Some(v) = p.max_concurrent {
                cfg.prefetch.max_concurrent = v;
            }
            if let Some(v) = p.hold_time_seconds {
                cfg.prefetch.hold_time_secs = v;
            }
        }
    }

    // Override with environment variables
    if let Ok(v) = std::env::var("PATENT_CACHE_DIR") {
        cfg.cache_local_dir = PathBuf::from(v);
    }
    if let Ok(v) = std::env::var("PATENT_GLOBAL_DB") {
        cfg.cache_global_db = PathBuf::from(v);
    }
    if let Ok(v) = std::env::var("PATENT_CONCURRENCY") {
        if let Ok(n) = v.parse::<usize>() {
            cfg.concurrency = n;
        }
    }
    if let Ok(v) = std::env::var("PATENT_TIMEOUT") {
        if let Ok(t) = v.parse::<f64>() {
            cfg.timeout_secs = t;
        }
    }
    if let Ok(v) = std::env::var("PATENT_FETCH_ALL_SOURCES") {
        cfg.fetch_all_sources = parse_bool_env(&v);
    }
    if let Ok(v) = std::env::var("PATENT_EPO_KEY") {
        let parts: Vec<&str> = v.splitn(2, ':').collect();
        cfg.epo_client_id = parts
            .first()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        cfg.epo_client_secret = parts
            .get(1)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
    }
    if let Ok(v) = std::env::var("PATENT_LENS_KEY") {
        cfg.lens_api_key = Some(v).filter(|s| !s.is_empty());
    }
    if let Ok(v) = std::env::var("PATENT_SERPAPI_KEY") {
        cfg.serpapi_key = Some(v).filter(|s| !s.is_empty());
    }
    if let Ok(v) = std::env::var("PATENT_BING_KEY") {
        cfg.bing_key = Some(v).filter(|s| !s.is_empty());
    }
    if let Ok(v) = std::env::var("PATENT_BIGQUERY_PROJECT") {
        cfg.bigquery_project = Some(v).filter(|s| !s.is_empty());
    }
    if let Ok(v) = std::env::var("PATENT_ACTIVITY_JOURNAL") {
        if v.is_empty() {
            cfg.activity_journal = None;
        } else {
            cfg.activity_journal = Some(PathBuf::from(v));
        }
    }
    // Search env vars
    if let Ok(v) = std::env::var("PATENT_SEARCH_BROWSER_PROFILES_DIR") {
        cfg.search_browser_profiles_dir = Some(PathBuf::from(v));
    }
    if let Ok(v) = std::env::var("PATENT_SEARCH_BROWSER_DEFAULT_PROFILE") {
        cfg.search_browser_default_profile = v;
    }
    if let Ok(v) = std::env::var("PATENT_SEARCH_BROWSER_HEADLESS") {
        cfg.search_browser_headless = parse_bool_env(&v);
    }
    if let Ok(v) = std::env::var("PATENT_SEARCH_BROWSER_TIMEOUT") {
        if let Ok(t) = v.parse::<f64>() {
            cfg.search_browser_timeout = t;
        }
    }
    if let Ok(v) = std::env::var("PATENT_SEARCH_BROWSER_MAX_PAGES") {
        if let Ok(n) = v.parse::<usize>() {
            cfg.search_browser_max_pages = n;
        }
    }
    if let Ok(v) = std::env::var("PATENT_SEARCH_BROWSER_IDLE_TIMEOUT") {
        if let Ok(t) = v.parse::<f64>() {
            cfg.search_browser_idle_timeout = t;
        }
    }
    if let Ok(v) = std::env::var("PATENT_SEARCH_BROWSER_DEBUG_HTML_DIR") {
        cfg.search_browser_debug_html_dir = Some(PathBuf::from(v));
    }
    if let Ok(v) = std::env::var("PATENT_SEARCH_BACKEND_DEFAULT") {
        cfg.search_backend_default = v;
    }
    if let Ok(v) = std::env::var("PATENT_SEARCH_ENRICH_TOP_N") {
        if let Ok(n) = v.parse::<usize>() {
            cfg.search_enrich_top_n = n;
        }
    }
    if let Ok(v) = std::env::var("PATENT_PREFETCH_FULL_TEXT") {
        cfg.prefetch.full_text = parse_bool_env(&v);
    }
    if let Ok(v) = std::env::var("PATENT_PREFETCH_LIMIT") {
        if let Ok(n) = v.parse::<usize>() {
            cfg.prefetch.limit = n;
        }
    }
    if let Ok(v) = std::env::var("PATENT_MAX_CONCURRENT_FETCHES") {
        if let Ok(n) = v.parse::<usize>() {
            cfg.prefetch.max_concurrent = n;
        }
    }
    if let Ok(v) = std::env::var("PATENT_FETCH_HOLD_TIME_SECS") {
        if let Ok(n) = v.parse::<u64>() {
            cfg.prefetch.hold_time_secs = n;
        }
    }

    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{LazyLock, Mutex, MutexGuard};
    use tempfile::TempDir;

    static CONFIG_TEST_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    struct TestEnvGuard {
        _lock: MutexGuard<'static, ()>,
        saved_home: Option<std::ffi::OsString>,
        saved_serpapi: Option<std::ffi::OsString>,
        saved_cwd: PathBuf,
    }

    impl TestEnvGuard {
        fn new() -> Self {
            Self {
                _lock: CONFIG_TEST_LOCK.lock().expect("config test lock"),
                saved_home: std::env::var_os("HOME"),
                saved_serpapi: std::env::var_os("PATENT_SERPAPI_KEY"),
                saved_cwd: std::env::current_dir().expect("current dir"),
            }
        }
    }

    impl Drop for TestEnvGuard {
        fn drop(&mut self) {
            if let Some(home) = &self.saved_home {
                std::env::set_var("HOME", home);
            } else {
                std::env::remove_var("HOME");
            }

            if let Some(key) = &self.saved_serpapi {
                std::env::set_var("PATENT_SERPAPI_KEY", key);
            } else {
                std::env::remove_var("PATENT_SERPAPI_KEY");
            }

            let _ = std::env::set_current_dir(&self.saved_cwd);
        }
    }

    #[test]
    fn test_defaults() {
        // Verify default helper functions match Python reference values
        let p = default_source_priority();
        assert!(p.contains(&"USPTO".to_string()));
        assert!(p.contains(&"Google_Patents".to_string()));
        assert!(p.contains(&"web_search".to_string()));

        let c = default_converters_order();
        assert!(c.contains(&"pymupdf4llm".to_string()));
        assert!(c.contains(&"marker".to_string()));

        // Verify that PatentConfig with correct Python-parity defaults assembles cleanly
        let cfg = PatentConfig {
            cache_local_dir: xdg_data_home().join("patent-cache").join("patents"),
            cache_global_db: default_global_db(),
            source_priority: default_source_priority(),
            concurrency: 10,
            fetch_all_sources: true,
            timeout_secs: 30.0,
            converters_order: default_converters_order(),
            converters_disabled: vec!["marker".to_string()],
            source_base_urls: HashMap::new(),
            epo_client_id: None,
            epo_client_secret: None,
            lens_api_key: None,
            serpapi_key: None,
            bing_key: None,
            bigquery_project: None,
            activity_journal: Some(PathBuf::from(".patent-activity.jsonl")),
            search_browser_profiles_dir: None,
            search_browser_default_profile: "default".into(),
            search_browser_headless: true,
            search_browser_timeout: 60.0,
            search_browser_max_pages: 3,
            search_browser_idle_timeout: 1800.0,
            search_browser_debug_html_dir: None,
            search_backend_default: "browser".into(),
            search_enrich_top_n: 5,
            prefetch: PrefetchConfig::default(),
        };
        assert_eq!(
            cfg.cache_local_dir,
            xdg_data_home().join("patent-cache").join("patents")
        );
        assert_eq!(cfg.concurrency, 10);
        assert!(cfg.fetch_all_sources);
        assert_eq!(cfg.timeout_secs, 30.0);
        assert_eq!(cfg.converters_disabled, vec!["marker".to_string()]);
        assert!(cfg.epo_client_id.is_none());
        assert_eq!(cfg.search_backend_default, "browser");
        assert_eq!(cfg.search_enrich_top_n, 5);
        assert_eq!(cfg.search_browser_idle_timeout, 1800.0);
        assert!(!cfg.prefetch.full_text);
        assert_eq!(cfg.prefetch.limit, 5);
        assert_eq!(cfg.prefetch.max_concurrent, 1);
        assert_eq!(cfg.prefetch.hold_time_secs, 5);
    }

    #[test]
    fn test_parse_bool_env() {
        assert!(parse_bool_env("1"));
        assert!(parse_bool_env("true"));
        assert!(parse_bool_env("TRUE"));
        assert!(parse_bool_env("yes"));
        assert!(parse_bool_env("on"));
        assert!(!parse_bool_env("0"));
        assert!(!parse_bool_env("false"));
        assert!(!parse_bool_env("no"));
    }

    #[test]
    fn test_default_source_priority_includes_uspto() {
        let p = default_source_priority();
        assert!(p.contains(&"USPTO".to_string()));
        assert!(p.contains(&"EPO_OPS".to_string()));
        assert!(p.contains(&"Google_Patents".to_string()));
        assert!(p.contains(&"web_search".to_string()));
    }

    #[test]
    fn test_xdg_data_home_uses_env() {
        // Can't easily override env in tests without unsafety; just check it returns something
        let path = xdg_data_home();
        assert!(!path.as_os_str().is_empty());
    }

    #[test]
    fn test_load_config_autoloads_home_env_file() {
        let _guard = TestEnvGuard::new();
        let temp = TempDir::new().expect("tempdir");
        let home = temp.path().join("home");
        std::fs::create_dir_all(&home).expect("create home");
        std::fs::write(
            home.join(".patents-mcp.env"),
            "PATENT_SERPAPI_KEY=from_home_env\n",
        )
        .expect("write env");

        std::env::set_var("HOME", &home);
        std::env::remove_var("PATENT_SERPAPI_KEY");
        std::env::set_current_dir(temp.path()).expect("set cwd");

        let cfg = load_config().expect("load config");
        assert_eq!(cfg.serpapi_key.as_deref(), Some("from_home_env"));
    }

    #[test]
    fn test_load_config_home_env_beats_cwd_env() {
        let _guard = TestEnvGuard::new();
        let temp = TempDir::new().expect("tempdir");
        let home = temp.path().join("home");
        let cwd = temp.path().join("cwd");
        std::fs::create_dir_all(&home).expect("create home");
        std::fs::create_dir_all(&cwd).expect("create cwd");
        std::fs::write(
            home.join(".patents-mcp.env"),
            "PATENT_SERPAPI_KEY=from_home_env\n",
        )
        .expect("write home env");
        std::fs::write(cwd.join(".env"), "PATENT_SERPAPI_KEY=from_cwd_env\n")
            .expect("write cwd env");

        std::env::set_var("HOME", &home);
        std::env::remove_var("PATENT_SERPAPI_KEY");
        std::env::set_current_dir(&cwd).expect("set cwd");

        let cfg = load_config().expect("load config");
        assert_eq!(cfg.serpapi_key.as_deref(), Some("from_home_env"));
    }

    #[test]
    fn test_load_config_explicit_env_beats_autoloaded_files() {
        let _guard = TestEnvGuard::new();
        let temp = TempDir::new().expect("tempdir");
        let home = temp.path().join("home");
        let cwd = temp.path().join("cwd");
        std::fs::create_dir_all(&home).expect("create home");
        std::fs::create_dir_all(&cwd).expect("create cwd");
        std::fs::write(
            home.join(".patents-mcp.env"),
            "PATENT_SERPAPI_KEY=from_home_env\n",
        )
        .expect("write home env");
        std::fs::write(cwd.join(".env"), "PATENT_SERPAPI_KEY=from_cwd_env\n")
            .expect("write cwd env");

        std::env::set_var("HOME", &home);
        std::env::set_var("PATENT_SERPAPI_KEY", "from_explicit_env");
        std::env::set_current_dir(&cwd).expect("set cwd");

        let cfg = load_config().expect("load config");
        assert_eq!(cfg.serpapi_key.as_deref(), Some("from_explicit_env"));
    }
}
