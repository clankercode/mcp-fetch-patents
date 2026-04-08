//! Patent artifact cache — mirrors Python patent_mcp.cache module.
//!
//! Two-layer cache:
//!   - Local per-repo SQLite at `.patents/index.db`
//!   - Global XDG index at `$XDG_DATA_HOME/patent-cache/index.db`

use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
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
        let mut tokens = self.tokens.lock().unwrap();
        match tokens.entry(source.to_string()) {
            Entry::Occupied(e) if e.get().expires_at > Utc::now() => {
                Some(e.get().token.clone())
            }
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
        let mut tokens = self.tokens.lock().unwrap();
        tokens.insert(
            source.to_string(),
            SessionToken {
                token: token.to_string(),
                expires_at,
            },
        );
    }

    pub fn invalidate(&self, source: &str) {
        self.tokens.lock().unwrap().remove(source);
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
    local_db: PathBuf,
    #[allow(dead_code)]
    global_db: PathBuf,
}

impl PatentCache {
    pub fn new(config: &PatentConfig) -> Result<Self> {
        let local_dir = config.cache_local_dir.clone();
        let local_db = local_dir.join("index.db");
        let global_db = config.cache_global_db.clone();

        let cache = PatentCache {
            local_dir,
            local_db: local_db.clone(),
            global_db: global_db.clone(),
        };

        cache.init_db(&local_db)?;
        cache.init_db(&global_db)?;

        // Register local cache dir in global index
        let conn = cache.connect(Some(&global_db))?;
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

    fn connect(&self, db_path: Option<&Path>) -> Result<Connection> {
        let path = db_path.unwrap_or(&self.local_db);
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        Ok(conn)
    }

    fn patent_dir(&self, canonical_id: &str) -> PathBuf {
        self.local_dir.join(canonical_id)
    }

    /// Look up a patent in the local cache. Returns None on miss or stale.
    pub fn lookup(&self, canonical_id: &str) -> Result<Option<CacheResult>> {
        let conn = self.connect(None)?;

        let row = {
            let mut stmt = conn.prepare(
                "SELECT canonical_id, jurisdiction, doc_type, title, abstract, inventors,
                        assignee, filing_date, publication_date, grant_date, fetched_at,
                        legal_status, status_fetched_at, cache_dir
                 FROM patents WHERE canonical_id = ?1"
            )?;
            stmt.query_row(params![canonical_id], |row| {
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
            }).optional()?
        };

        let (cid, jur, doc_type, title, abs, inventors_json, assignee, filing, pub_date,
             grant, fetched_at, legal_status, _status_fetched_at, cache_dir) = match row {
            None => return Ok(None),
            Some(r) => r,
        };

        let inventors: Vec<String> = inventors_json
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();

        // Get file locations
        let mut stmt = conn.prepare(
            "SELECT format, path FROM patent_locations WHERE patent_id = ?1"
        )?;
        let loc_rows: Vec<(String, String)> = stmt
            .query_map(params![canonical_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<_>>()?;

        let mut files: HashMap<String, PathBuf> = HashMap::new();
        for (fmt, path) in &loc_rows {
            let p = PathBuf::from(path);
            if p.exists() {
                files.insert(fmt.clone(), p);
            }
        }

        // Stale check: no files at all
        if !loc_rows.is_empty() && files.is_empty() {
            return Ok(None);
        }

        let is_complete = files.len() == loc_rows.len();
        let metadata = PatentMetadata {
            canonical_id: cid,
            jurisdiction: jur,
            doc_type,
            title,
            abstract_text: abs,
            inventors,
            assignee,
            filing_date: filing,
            publication_date: pub_date,
            grant_date: grant,
            fetched_at,
            legal_status,
        };

        Ok(Some(CacheResult {
            canonical_id: canonical_id.to_string(),
            cache_dir: PathBuf::from(cache_dir),
            files,
            metadata: Some(metadata),
            is_complete,
        }))
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
                    &sources.iter().map(|s| serde_json::json!({
                        "source": s.source,
                        "success": s.success,
                        "elapsed_ms": s.elapsed_ms,
                        "error": s.error,
                    })).collect::<Vec<_>>()
                )?;
                std::fs::write(sources_json, sources_content)?;
            }
        }

        // Persist to DB in one transaction
        let conn = self.connect(None)?;
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
                Option::<String>::None, // status_fetched_at
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
    }

    /// List all patents in the local cache.
    pub fn list_all(&self) -> Result<Vec<CacheEntry>> {
        let conn = self.connect(None)?;
        let mut stmt = conn.prepare("SELECT canonical_id, cache_dir FROM patents ORDER BY canonical_id")?;
        let entries = stmt.query_map([], |row| {
            Ok(CacheEntry {
                canonical_id: row.get(0)?,
                cache_dir: PathBuf::from(row.get::<_, String>(1)?),
            })
        })?.collect::<rusqlite::Result<_>>()?;
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
            cache_local_dir: tmp.path().join("local").join(".patents"),
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
        let cache = PatentCache::new(&cfg).unwrap();
        assert!(cfg.cache_local_dir.join("index.db").exists());
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

        let artifacts = ArtifactSet { pdf: Some(pdf), txt: None, md: None, images: vec![] };
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

        cache.store("US7654321", &ArtifactSet { pdf: Some(pdf1), txt: None, md: None, images: vec![] }, &make_meta("US7654321"), None).unwrap();
        cache.store("EP1234567", &ArtifactSet { pdf: Some(pdf2), txt: None, md: None, images: vec![] }, &make_meta("EP1234567"), None).unwrap();

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

        let artifacts = ArtifactSet { pdf: Some(pdf), txt: None, md: None, images: vec![] };
        cache.store("US7654321", &artifacts, &make_meta("US7654321"), None).unwrap();
        // Should not panic or return error
    }
}
