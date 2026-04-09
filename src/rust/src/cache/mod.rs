//! Patent artifact cache — mirrors Python patent_mcp.cache module.
//!
//! Single global SQLite DB at `$XDG_DATA_HOME/patent-cache/index.db`.
//! Patent files stored under `$XDG_DATA_HOME/patent-cache/patents/`.

use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::config::PatentConfig;

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatentMetadata {
    pub canonical_id: String,
    pub jurisdiction: String,
    pub doc_type: String,
    pub title: Option<String>,
    // Python serializes as "abstract"; accept both field names
    #[serde(alias = "abstract", rename = "abstract_text")]
    pub abstract_text: Option<String>,
    pub inventors: Vec<String>,
    pub assignee: Option<String>,
    pub filing_date: Option<String>,
    pub publication_date: Option<String>,
    pub grant_date: Option<String>,
    pub fetched_at: String,
    pub legal_status: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SourceAttempt {
    pub source: String,
    pub success: bool,
    pub elapsed_ms: f64,
    pub error: Option<String>,
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone)]
pub struct ArtifactSet {
    pub pdf: Option<PathBuf>,
    pub txt: Option<PathBuf>,
    pub md: Option<PathBuf>,
    pub images: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct CacheResult {
    pub canonical_id: String,
    pub cache_dir: PathBuf,
    pub files: HashMap<String, PathBuf>,
    pub metadata: Option<PatentMetadata>,
    pub is_complete: bool,
}

#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub canonical_id: String,
    pub cache_dir: PathBuf,
}

// ---------------------------------------------------------------------------
// SQLite schema
// ---------------------------------------------------------------------------

const SCHEMA_SQL: &str = "
PRAGMA journal_mode=WAL;
PRAGMA foreign_keys=ON;

CREATE TABLE IF NOT EXISTS patents (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    canonical_id     TEXT NOT NULL UNIQUE,
    jurisdiction     TEXT NOT NULL,
    doc_type         TEXT NOT NULL DEFAULT 'patent',
    title            TEXT,
    abstract         TEXT,
    inventors        TEXT DEFAULT '[]',
    assignee         TEXT,
    filing_date      TEXT,
    publication_date TEXT,
    grant_date       TEXT,
    fetched_at       TEXT NOT NULL,
    legal_status     TEXT,
    status_fetched_at TEXT,
    cache_dir        TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS patent_locations (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    patent_id  TEXT NOT NULL REFERENCES patents(canonical_id) ON DELETE CASCADE,
    format     TEXT NOT NULL,
    path       TEXT NOT NULL,
    UNIQUE(patent_id, format)
);

CREATE TABLE IF NOT EXISTS fetch_sources (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    patent_id   TEXT NOT NULL REFERENCES patents(canonical_id) ON DELETE CASCADE,
    source      TEXT NOT NULL,
    success     INTEGER NOT NULL,
    elapsed_ms  REAL NOT NULL,
    error       TEXT,
    fetched_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS cache_registrations (
    cache_dir     TEXT PRIMARY KEY,
    registered_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_patent_locations_patent_id ON patent_locations(patent_id);
CREATE INDEX IF NOT EXISTS idx_patents_canonical_id ON patents(canonical_id);
";

// ---------------------------------------------------------------------------
// SessionCache (in-memory, per-process)
// ---------------------------------------------------------------------------

use std::collections::hash_map::Entry;
use std::sync::Mutex;

#[derive(Debug)]
struct SessionToken {
    token: String,
    expires_at: DateTime<Utc>,
}

/// In-memory token cache. Thread-safe via Mutex.
pub struct SessionCache {
    tokens: Mutex<HashMap<String, SessionToken>>,
}

impl SessionCache {
    pub fn new() -> Self {
        SessionCache {
            tokens: Mutex::new(HashMap::new()),
        }
    }

    pub fn get(&self, source: &str) -> Option<String> {
        let mut tokens = self.tokens.lock().unwrap_or_else(|e| e.into_inner());
        match tokens.entry(source.to_string()) {
            Entry::Occupied(e) if e.get().expires_at > Utc::now() => Some(e.get().token.clone()),
            Entry::Occupied(e) => {
                e.remove();
                None
            }
            Entry::Vacant(_) => None,
        }
    }

    pub fn set(&self, source: &str, token: &str, ttl_minutes: i64) {
        use chrono::Duration;
        let expires_at = Utc::now() + Duration::minutes(ttl_minutes);
        self.set_with_expiry(source, token, expires_at);
    }

    pub fn set_with_expiry(&self, source: &str, token: &str, expires_at: DateTime<Utc>) {
        let mut tokens = self.tokens.lock().unwrap_or_else(|e| e.into_inner());
        tokens.insert(
            source.to_string(),
            SessionToken {
                token: token.to_string(),
                expires_at,
            },
        );
    }

    pub fn invalidate(&self, source: &str) {
        self.tokens
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(source);
    }
}

impl Default for SessionCache {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// PatentCache
// ---------------------------------------------------------------------------

pub struct PatentCache {
    local_dir: PathBuf,
    global_db: PathBuf,
}

impl PatentCache {
    pub fn new(config: &PatentConfig) -> Result<Self> {
        let local_dir = config.cache_local_dir.clone();
        let global_db = config.cache_global_db.clone();

        let cache = PatentCache {
            local_dir,
            global_db: global_db.clone(),
        };

        cache.init_db(&global_db)?;

        // Suggest migration if old .patents/index.db exists in CWD
        let old_local_db = std::path::Path::new(".patents").join("index.db");
        if old_local_db.exists() {
            tracing::info!(
                "Found old .patents/index.db in CWD. Patent cache now uses {}. \
                 The old .patents/ directory can be safely deleted.",
                global_db.display()
            );
        }

        // Register cache dir in global index
        let conn = cache.connect()?;
        conn.execute(
            "INSERT OR IGNORE INTO cache_registrations(cache_dir, registered_at) VALUES (?1, ?2)",
            params![
                cache.local_dir.to_string_lossy().as_ref(),
                Utc::now().to_rfc3339()
            ],
        )?;

        Ok(cache)
    }

    fn init_db(&self, db_path: &Path) -> Result<()> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(db_path)?;
        conn.execute_batch(SCHEMA_SQL)?;
        Ok(())
    }

    fn connect(&self) -> Result<Connection> {
        let conn = Connection::open(&self.global_db)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        Ok(conn)
    }

    fn patent_dir(&self, canonical_id: &str) -> PathBuf {
        self.local_dir.join(canonical_id)
    }

    pub fn lookup(&self, canonical_id: &str) -> Result<Option<CacheResult>> {
        let results = self.lookup_batch(&[canonical_id])?;
        Ok(results.into_iter().next().unwrap_or(None))
    }

    #[allow(clippy::type_complexity)]
    pub fn lookup_batch(&self, ids: &[&str]) -> Result<Vec<Option<CacheResult>>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.connect()?;

        let placeholders: Vec<&str> = ids.iter().map(|_| "?").collect();
        let sql = format!(
            "SELECT canonical_id, jurisdiction, doc_type, title, abstract, inventors,
                    assignee, filing_date, publication_date, grant_date, fetched_at,
                    legal_status, status_fetched_at, cache_dir
             FROM patents WHERE canonical_id IN ({})",
            placeholders.join(",")
        );
        let params_vec: Vec<&dyn rusqlite::ToSql> =
            ids.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows: Vec<(
            String,
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            String,
            Option<String>,
            Option<String>,
            String,
        )> = stmt
            .query_map(params_vec.as_slice(), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, Option<String>>(9)?,
                    row.get::<_, String>(10)?,
                    row.get::<_, Option<String>>(11)?,
                    row.get::<_, Option<String>>(12)?,
                    row.get::<_, String>(13)?,
                ))
            })?
            .collect::<rusqlite::Result<_>>()?;

        let mut by_id: HashMap<
            String,
            (
                String,
                String,
                String,
                Option<String>,
                Option<String>,
                Option<String>,
                Option<String>,
                Option<String>,
                Option<String>,
                Option<String>,
                String,
                Option<String>,
                Option<String>,
                String,
            ),
        > = HashMap::with_capacity(rows.len());
        for row in rows {
            by_id.insert(row.0.clone(), row);
        }

        let loc_placeholders: Vec<&str> = ids.iter().map(|_| "?").collect();
        let loc_sql = format!(
            "SELECT patent_id, format, path FROM patent_locations WHERE patent_id IN ({})",
            loc_placeholders.join(",")
        );
        let mut loc_stmt = conn.prepare(&loc_sql)?;
        let loc_params: Vec<&dyn rusqlite::ToSql> =
            ids.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
        let loc_rows: Vec<(String, String, String)> = loc_stmt
            .query_map(loc_params.as_slice(), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<_>>()?;

        let mut locs_by_id: HashMap<String, Vec<(String, String)>> = HashMap::new();
        for (pid, fmt, path) in &loc_rows {
            locs_by_id
                .entry(pid.clone())
                .or_default()
                .push((fmt.clone(), path.clone()));
        }

        let mut results: Vec<Option<CacheResult>> = Vec::with_capacity(ids.len());
        for id in ids {
            match by_id.get(*id) {
                None => results.push(None),
                Some(row) => {
                    let (
                        cid,
                        jur,
                        doc_type,
                        title,
                        abs,
                        inventors_json,
                        assignee,
                        filing,
                        pub_date,
                        grant,
                        fetched_at,
                        legal_status,
                        _status_fetched_at,
                        cache_dir,
                    ) = row;

                    let inventors: Vec<String> = inventors_json
                        .as_deref()
                        .and_then(|s| serde_json::from_str(s).ok())
                        .unwrap_or_default();

                    let loc_rows_for = locs_by_id.get(*id).cloned().unwrap_or_default();
                    let mut files: HashMap<String, PathBuf> = HashMap::new();
                    for (fmt, path) in &loc_rows_for {
                        let p = PathBuf::from(path);
                        if p.exists() {
                            files.insert(fmt.clone(), p);
                        }
                    }

                    if !loc_rows_for.is_empty() && files.is_empty() {
                        results.push(None);
                        continue;
                    }

                    let is_complete = files.len() == loc_rows_for.len();
                    let metadata = PatentMetadata {
                        canonical_id: cid.clone(),
                        jurisdiction: jur.clone(),
                        doc_type: doc_type.clone(),
                        title: title.clone(),
                        abstract_text: abs.clone(),
                        inventors,
                        assignee: assignee.clone(),
                        filing_date: filing.clone(),
                        publication_date: pub_date.clone(),
                        grant_date: grant.clone(),
                        fetched_at: fetched_at.clone(),
                        legal_status: legal_status.clone(),
                    };

                    results.push(Some(CacheResult {
                        canonical_id: id.to_string(),
                        cache_dir: PathBuf::from(cache_dir),
                        files,
                        metadata: Some(metadata),
                        is_complete,
                    }));
                }
            }
        }

        Ok(results)
    }

    /// Store patent artifacts and metadata in the local cache.
    pub fn store(
        &self,
        canonical_id: &str,
        artifacts: &ArtifactSet,
        metadata: &PatentMetadata,
        fetch_sources: Option<&[SourceAttempt]>,
    ) -> Result<()> {
        let dest_dir = self.patent_dir(canonical_id);
        std::fs::create_dir_all(&dest_dir)?;

        // Copy artifact files (skip if already in destination)
        let mut file_entries: Vec<(String, PathBuf)> = Vec::new();
        for (fmt, src) in [
            ("pdf", &artifacts.pdf),
            ("txt", &artifacts.txt),
            ("md", &artifacts.md),
        ] {
            if let Some(src_path) = src {
                let dst = dest_dir.join(src_path.file_name().unwrap_or_default());
                if src_path.canonicalize().ok() != dst.canonicalize().ok() {
                    std::fs::copy(src_path, &dst)?;
                }
                file_entries.push((fmt.to_string(), dst));
            }
        }

        // Write metadata.json
        let meta_json = dest_dir.join("metadata.json");
        let meta_content = serde_json::to_string_pretty(&metadata)?;
        std::fs::write(&meta_json, meta_content)?;

        // Write sources.json
        if let Some(sources) = fetch_sources {
            if !sources.is_empty() {
                let sources_json = dest_dir.join("sources.json");
                let sources_content = serde_json::to_string_pretty(
                    &sources
                        .iter()
                        .map(|s| {
                            serde_json::json!({
                                "source": s.source,
                                "success": s.success,
                                "elapsed_ms": s.elapsed_ms,
                                "error": s.error,
                            })
                        })
                        .collect::<Vec<_>>(),
                )?;
                std::fs::write(sources_json, sources_content)?;
            }
        }

        let conn = self.connect()?;
        conn.execute_batch("BEGIN TRANSACTION")?;

        let db_result: Result<()> = (|| {
            conn.execute(
                "INSERT OR REPLACE INTO patents
                 (canonical_id, jurisdiction, doc_type, title, abstract, inventors,
                  assignee, filing_date, publication_date, grant_date, fetched_at,
                  legal_status, status_fetched_at, cache_dir)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
                params![
                    metadata.canonical_id,
                    metadata.jurisdiction,
                    metadata.doc_type,
                    metadata.title,
                    metadata.abstract_text,
                    serde_json::to_string(&metadata.inventors).unwrap_or_else(|_| "[]".into()),
                    metadata.assignee,
                    metadata.filing_date,
                    metadata.publication_date,
                    metadata.grant_date,
                    metadata.fetched_at,
                    metadata.legal_status,
                    Option::<String>::None,
                    dest_dir.to_string_lossy().as_ref(),
                ],
            )?;

            conn.execute(
                "DELETE FROM patent_locations WHERE patent_id = ?1",
                params![canonical_id],
            )?;

            for (fmt, path) in &file_entries {
                conn.execute(
                    "INSERT INTO patent_locations(patent_id, format, path) VALUES (?1,?2,?3)",
                    params![canonical_id, fmt, path.to_string_lossy().as_ref()],
                )?;
            }

            if let Some(sources) = fetch_sources {
                let now = Utc::now().to_rfc3339();
                for s in sources {
                    conn.execute(
                        "INSERT INTO fetch_sources(patent_id, source, success, elapsed_ms, error, fetched_at)
                         VALUES (?1,?2,?3,?4,?5,?6)",
                        params![canonical_id, s.source, s.success as i32, s.elapsed_ms, s.error, now],
                    )?;
                }
            }

            Ok(())
        })();

        match db_result {
            Ok(()) => {
                conn.execute_batch("COMMIT")?;
                Ok(())
            }
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }

    /// List all patents in the local cache.
    pub fn list_all(&self) -> Result<Vec<CacheEntry>> {
        let conn = self.connect()?;
        let mut stmt =
            conn.prepare("SELECT canonical_id, cache_dir FROM patents ORDER BY canonical_id")?;
        let entries = stmt
            .query_map([], |row| {
                Ok(CacheEntry {
                    canonical_id: row.get(0)?,
                    cache_dir: PathBuf::from(row.get::<_, String>(1)?),
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(entries)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_config(tmp: &TempDir) -> PatentConfig {
        PatentConfig {
            cache_local_dir: tmp.path().join("local").join("patents"),
            cache_global_db: tmp.path().join("global").join("index.db"),
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

    fn make_meta(canonical_id: &str) -> PatentMetadata {
        PatentMetadata {
            canonical_id: canonical_id.to_string(),
            jurisdiction: "US".to_string(),
            doc_type: "patent".to_string(),
            title: Some("Test Patent".to_string()),
            abstract_text: None,
            inventors: vec!["Alice".to_string()],
            assignee: None,
            filing_date: None,
            publication_date: None,
            grant_date: None,
            fetched_at: "2026-01-01T00:00:00+00:00".to_string(),
            legal_status: None,
        }
    }

    #[test]
    fn test_cache_init() {
        let tmp = TempDir::new().unwrap();
        let cfg = make_config(&tmp);
        let _cache = PatentCache::new(&cfg).unwrap();
        assert!(cfg.cache_global_db.exists());
    }

    #[test]
    fn test_lookup_miss() {
        let tmp = TempDir::new().unwrap();
        let cfg = make_config(&tmp);
        let cache = PatentCache::new(&cfg).unwrap();
        let result = cache.lookup("US9999999").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_store_and_lookup() {
        let tmp = TempDir::new().unwrap();
        let cfg = make_config(&tmp);
        let cache = PatentCache::new(&cfg).unwrap();

        let pdf = tmp.path().join("test.pdf");
        std::fs::write(&pdf, b"%PDF-1.4").unwrap();

        let artifacts = ArtifactSet {
            pdf: Some(pdf),
            txt: None,
            md: None,
            images: vec![],
        };
        let meta = make_meta("US7654321");
        cache.store("US7654321", &artifacts, &meta, None).unwrap();

        let result = cache.lookup("US7654321").unwrap();
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.canonical_id, "US7654321");
        assert!(r.metadata.is_some());
        assert_eq!(r.metadata.unwrap().title, Some("Test Patent".to_string()));
    }

    #[test]
    fn test_list_all_empty() {
        let tmp = TempDir::new().unwrap();
        let cfg = make_config(&tmp);
        let cache = PatentCache::new(&cfg).unwrap();
        let entries = cache.list_all().unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_list_all_two_patents() {
        let tmp = TempDir::new().unwrap();
        let cfg = make_config(&tmp);
        let cache = PatentCache::new(&cfg).unwrap();

        let pdf1 = tmp.path().join("a.pdf");
        std::fs::write(&pdf1, b"%PDF-1.4").unwrap();
        let pdf2 = tmp.path().join("b.pdf");
        std::fs::write(&pdf2, b"%PDF-1.4").unwrap();

        cache
            .store(
                "US7654321",
                &ArtifactSet {
                    pdf: Some(pdf1),
                    txt: None,
                    md: None,
                    images: vec![],
                },
                &make_meta("US7654321"),
                None,
            )
            .unwrap();
        cache
            .store(
                "EP1234567",
                &ArtifactSet {
                    pdf: Some(pdf2),
                    txt: None,
                    md: None,
                    images: vec![],
                },
                &make_meta("EP1234567"),
                None,
            )
            .unwrap();

        let entries = cache.list_all().unwrap();
        assert_eq!(entries.len(), 2);
        let ids: Vec<&str> = entries.iter().map(|e| e.canonical_id.as_str()).collect();
        assert!(ids.contains(&"US7654321"));
        assert!(ids.contains(&"EP1234567"));
    }

    #[test]
    fn test_session_cache_basic() {
        let sc = SessionCache::new();
        assert!(sc.get("PPUBS").is_none());
        sc.set("PPUBS", "mytoken", 30);
        assert_eq!(sc.get("PPUBS"), Some("mytoken".to_string()));
        sc.invalidate("PPUBS");
        assert!(sc.get("PPUBS").is_none());
    }

    #[test]
    fn test_session_cache_expired() {
        let sc = SessionCache::new();
        let past = Utc::now() - chrono::Duration::seconds(1);
        sc.set_with_expiry("EPO", "oldtoken", past);
        assert!(sc.get("EPO").is_none());
    }

    #[test]
    fn test_store_same_file_no_error() {
        // Storing a file already in the destination dir should not crash
        let tmp = TempDir::new().unwrap();
        let cfg = make_config(&tmp);
        let cache = PatentCache::new(&cfg).unwrap();

        let dest_dir = cfg.cache_local_dir.join("US7654321");
        std::fs::create_dir_all(&dest_dir).unwrap();
        let pdf = dest_dir.join("US7654321.pdf");
        std::fs::write(&pdf, b"%PDF-1.4").unwrap();

        let artifacts = ArtifactSet {
            pdf: Some(pdf),
            txt: None,
            md: None,
            images: vec![],
        };
        cache
            .store("US7654321", &artifacts, &make_meta("US7654321"), None)
            .unwrap();
        // Should not panic or return error
    }

    #[test]
    fn patent_metadata_serde_roundtrip() {
        let meta = PatentMetadata {
            canonical_id: "US1234567".to_string(),
            jurisdiction: "US".to_string(),
            doc_type: "patent".to_string(),
            title: Some("Test Patent".to_string()),
            abstract_text: Some("An abstract".to_string()),
            inventors: vec!["Alice".to_string(), "Bob".to_string()],
            assignee: Some("Test Corp".to_string()),
            filing_date: Some("2023-06-15".to_string()),
            publication_date: Some("2024-01-15".to_string()),
            grant_date: Some("2024-06-01".to_string()),
            fetched_at: "2026-01-01T00:00:00+00:00".to_string(),
            legal_status: Some("Active".to_string()),
        };
        let json = serde_json::to_value(&meta).unwrap();
        let back: PatentMetadata = serde_json::from_value(json).unwrap();
        assert_eq!(back.canonical_id, meta.canonical_id);
        assert_eq!(back.jurisdiction, meta.jurisdiction);
        assert_eq!(back.doc_type, meta.doc_type);
        assert_eq!(back.title, meta.title);
        assert_eq!(back.abstract_text, meta.abstract_text);
        assert_eq!(back.inventors, meta.inventors);
        assert_eq!(back.assignee, meta.assignee);
        assert_eq!(back.filing_date, meta.filing_date);
        assert_eq!(back.publication_date, meta.publication_date);
        assert_eq!(back.grant_date, meta.grant_date);
        assert_eq!(back.fetched_at, meta.fetched_at);
        assert_eq!(back.legal_status, meta.legal_status);
    }

    #[test]
    fn patent_metadata_serde_accepts_abstract_alias() {
        let json = serde_json::json!({
            "canonical_id": "US9999999",
            "jurisdiction": "US",
            "doc_type": "patent",
            "title": "Alias Test",
            "abstract": "Deserialized via alias",
            "inventors": [],
            "fetched_at": "2026-01-01T00:00:00+00:00"
        });
        let meta: PatentMetadata = serde_json::from_value(json).unwrap();
        assert_eq!(
            meta.abstract_text,
            Some("Deserialized via alias".to_string())
        );
    }

    #[test]
    fn patent_metadata_serde_serializes_as_abstract_text() {
        let meta = PatentMetadata {
            canonical_id: "US1111111".to_string(),
            jurisdiction: "US".to_string(),
            doc_type: "patent".to_string(),
            title: None,
            abstract_text: Some("serialized field name".to_string()),
            inventors: vec![],
            assignee: None,
            filing_date: None,
            publication_date: None,
            grant_date: None,
            fetched_at: "2026-01-01T00:00:00+00:00".to_string(),
            legal_status: None,
        };
        let json = serde_json::to_value(&meta).unwrap();
        assert!(
            json.get("abstract_text").is_some(),
            "should serialize as abstract_text"
        );
        assert!(
            json.get("abstract").is_none(),
            "should not use alias for serialization"
        );
    }
}
