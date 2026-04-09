use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use crate::ranking::PatentHit;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryRecord {
    pub query_id: String,
    pub timestamp: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub query_text: String,
    #[serde(default)]
    pub result_count: i64,
    #[serde(default)]
    pub results: Vec<PatentHit>,
    #[serde(default)]
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub session_id: String,
    #[serde(default)]
    pub topic: String,
    pub created_at: String,
    pub modified_at: String,
    #[serde(default)]
    pub prior_art_cutoff: Option<String>,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub queries: Vec<QueryRecord>,
    #[serde(default)]
    pub classifications_explored: Vec<String>,
    #[serde(default)]
    pub citation_chains: Value,
    #[serde(default)]
    pub patent_families: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub session_id: String,
    pub topic: String,
    pub created_at: String,
    pub modified_at: String,
    pub query_count: usize,
    pub patent_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct IndexFile {
    sessions: Vec<SessionSummary>,
}

fn now_iso() -> String {
    Utc::now().to_rfc3339()
}

fn validate_session_id(id: &str) -> Result<()> {
    if id.is_empty() {
        anyhow::bail!("Session ID cannot be empty");
    }
    if id.contains('/') || id.contains('\\') || id.contains("..") || id.contains('\0') {
        anyhow::bail!("Invalid session ID: {:?}", id);
    }
    Ok(())
}

fn make_slug(topic: &str) -> String {
    let slug = topic.to_lowercase().replace(' ', "-");
    let slug: String = slug
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .take(30)
        .collect();
    slug
}

fn count_unique_patents(session: &Session) -> usize {
    let mut seen = HashSet::new();
    for q in &session.queries {
        for r in &q.results {
            seen.insert(r.patent_id.clone());
        }
    }
    seen.len()
}

fn update_index(dir: &Path, session: &Session) -> Result<()> {
    let index_path = dir.join(".index.json");

    let mut existing: Vec<SessionSummary> = Vec::new();
    if index_path.exists() {
        if let Ok(data) = fs::read_to_string(&index_path) {
            if let Ok(idx) = serde_json::from_str::<IndexFile>(&data) {
                existing = idx
                    .sessions
                    .into_iter()
                    .filter(|s| s.session_id != session.session_id)
                    .collect();
            }
        }
    }

    existing.push(SessionSummary {
        session_id: session.session_id.clone(),
        topic: session.topic.clone(),
        created_at: session.created_at.clone(),
        modified_at: session.modified_at.clone(),
        query_count: session.queries.len(),
        patent_count: count_unique_patents(session),
    });

    let index = IndexFile { sessions: existing };
    let content = serde_json::to_string_pretty(&index)?;
    let tmp_path = dir.join(".index.json.tmp");
    fs::write(&tmp_path, &content)?;
    fs::rename(&tmp_path, &index_path)?;
    Ok(())
}

pub struct SessionManager {
    dir: PathBuf,
}

impl SessionManager {
    pub fn new(sessions_dir: Option<PathBuf>) -> Self {
        let dir = match sessions_dir {
            Some(d) => d,
            None => std::env::var("PATENT_SESSIONS_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from(".patent-sessions")),
        };
        fs::create_dir_all(&dir).ok();
        Self { dir }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub async fn create_session(
        &self,
        topic: &str,
        prior_art_cutoff: Option<&str>,
        notes: &str,
    ) -> Result<Session> {
        let now = now_iso();
        let slug = make_slug(topic);
        let ts = Utc::now().format("%Y%m%d-%H%M%S").to_string();
        let session_id = format!("{}-{}", ts, slug);
        let mut session = Session {
            session_id,
            topic: topic.to_string(),
            created_at: now.clone(),
            modified_at: now,
            prior_art_cutoff: prior_art_cutoff.map(|s| s.to_string()),
            notes: notes.to_string(),
            queries: vec![],
            classifications_explored: vec![],
            citation_chains: Value::Object(serde_json::Map::new()),
            patent_families: BTreeMap::new(),
        };
        let modified = self.save_session(&mut session).await?;
        session.modified_at = modified;
        Ok(session)
    }

    pub async fn load_session(&self, session_id: &str) -> Result<Session> {
        let dir = self.dir.clone();
        let id = session_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Session> {
            validate_session_id(&id)?;
            let path = dir.join(format!("{}.json", id));
            let canonical_dir = dir.canonicalize().unwrap_or_else(|_| dir.clone());
            let canonical_path = path.canonicalize().unwrap_or_else(|_| path.clone());
            if !canonical_path.starts_with(&canonical_dir) {
                anyhow::bail!("Session ID escapes sessions directory: {}", id);
            }
            if !path.exists() {
                anyhow::bail!("Session not found: {}", id);
            }
            let data = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read session file: {}", path.display()))?;
            let session: Session = serde_json::from_str(&data)
                .with_context(|| format!("Failed to parse session file: {}", path.display()))?;
            Ok(session)
        })
        .await?
    }

    pub async fn save_session(&self, session: &mut Session) -> Result<String> {
        let modified = now_iso();
        session.modified_at = modified.clone();
        let dir = self.dir.clone();
        let sid = session.session_id.clone();
        let content = serde_json::to_string_pretty(&*session)?;
        let session_snapshot = session.clone();

        tokio::task::spawn_blocking(move || -> Result<()> {
            let path = dir.join(format!("{}.json", sid));
            let tmp_path = dir.join(format!("{}.json.tmp", sid));
            fs::write(&tmp_path, &content)
                .with_context(|| format!("Failed to write tmp file: {}", tmp_path.display()))?;
            fs::rename(&tmp_path, &path)
                .with_context(|| format!("Failed to rename tmp to {}", path.display()))?;
            update_index(&dir, &session_snapshot)?;
            Ok(())
        })
        .await??;

        Ok(modified)
    }

    pub async fn list_sessions(&self, limit: Option<usize>) -> Result<Vec<SessionSummary>> {
        let dir = self.dir.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<SessionSummary>> {
            let index_path = dir.join(".index.json");
            if index_path.exists() {
                if let Ok(data) = fs::read_to_string(&index_path) {
                    if let Ok(idx) = serde_json::from_str::<IndexFile>(&data) {
                        let mut summaries = idx.sessions;
                        summaries.sort_by(|a, b| b.modified_at.cmp(&a.modified_at));
                        if let Some(lim) = limit {
                            summaries.truncate(lim);
                        }
                        return Ok(summaries);
                    }
                }
            }

            let mut summaries: Vec<SessionSummary> = Vec::new();
            for entry in fs::read_dir(&dir)? {
                let entry = entry?;
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with('.') || !name.ends_with(".json") {
                    continue;
                }
                if let Ok(data) = fs::read_to_string(entry.path()) {
                    if let Ok(session) = serde_json::from_str::<Session>(&data) {
                        let pc = count_unique_patents(&session);
                        summaries.push(SessionSummary {
                            session_id: session.session_id,
                            topic: session.topic,
                            created_at: session.created_at,
                            modified_at: session.modified_at,
                            query_count: session.queries.len(),
                            patent_count: pc,
                        });
                    }
                }
            }

            summaries.sort_by(|a, b| b.modified_at.cmp(&a.modified_at));
            if let Some(lim) = limit {
                summaries.truncate(lim);
            }
            Ok(summaries)
        })
        .await?
    }

    pub async fn append_query_result(&self, session_id: &str, query: QueryRecord) -> Result<()> {
        let mut session = self.load_session(session_id).await?;
        session.queries.push(query);
        self.save_session(&mut session).await?;
        Ok(())
    }

    pub async fn add_note(&self, session_id: &str, note: &str) -> Result<()> {
        let mut session = self.load_session(session_id).await?;
        if session.notes.is_empty() {
            session.notes = note.to_string();
        } else {
            session.notes = format!("{}\n\n{}", session.notes, note);
        }
        self.save_session(&mut session).await?;
        Ok(())
    }

    pub async fn delete_session(&self, session_id: &str) -> Result<bool> {
        let dir = self.dir.clone();
        let id = session_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<bool> {
            validate_session_id(&id)?;
            let path = dir.join(format!("{}.json", id));
            if !path.exists() {
                return Ok(false);
            }
            let canonical_dir = dir.canonicalize().unwrap_or_else(|_| dir.clone());
            let canonical_path = path.canonicalize().unwrap_or_else(|_| path.clone());
            if !canonical_path.starts_with(&canonical_dir) {
                anyhow::bail!("Session ID escapes sessions directory: {}", id);
            }
            fs::remove_file(&path)
                .with_context(|| format!("Failed to delete session file: {}", path.display()))?;
            let index_path = dir.join(".index.json");
            if index_path.exists() {
                if let Ok(data) = fs::read_to_string(&index_path) {
                    if let Ok(mut idx) = serde_json::from_str::<IndexFile>(&data) {
                        let before = idx.sessions.len();
                        idx.sessions.retain(|s| s.session_id != id);
                        if idx.sessions.len() < before {
                            let content = serde_json::to_string_pretty(&idx)?;
                            let tmp_path = dir.join(".index.json.tmp");
                            let _ = fs::write(&tmp_path, &content);
                            let _ = fs::rename(&tmp_path, &index_path);
                        }
                    }
                }
            }
            Ok(true)
        })
        .await?
    }

    pub async fn annotate_patent(
        &self,
        session_id: &str,
        patent_id: &str,
        annotation: &str,
        relevance: &str,
    ) -> Result<bool> {
        let mut session = self.load_session(session_id).await?;
        let mut updated = false;
        for query in &mut session.queries {
            for hit in &mut query.results {
                if hit.patent_id == patent_id {
                    hit.note = annotation.to_string();
                    hit.relevance = relevance.to_string();
                    updated = true;
                }
            }
        }
        if updated {
            self.save_session(&mut session).await?;
        }
        Ok(updated)
    }

    pub async fn export_markdown(
        &self,
        session_id: &str,
        output_path: Option<&Path>,
    ) -> Result<PathBuf> {
        let session = self.load_session(session_id).await?;

        let output_path = match output_path {
            Some(p) => p.to_path_buf(),
            None => self.dir.join(format!("{}-report.md", session.session_id)),
        };

        if output_path.is_relative() {
            let mut depth: i32 = 0;
            for comp in output_path.components() {
                match comp {
                    std::path::Component::ParentDir => {
                        depth -= 1;
                        if depth < 0 {
                            anyhow::bail!(
                                "output_path escapes directory: {}",
                                output_path.display()
                            );
                        }
                    }
                    std::path::Component::Normal(_) => {
                        depth += 1;
                    }
                    std::path::Component::CurDir => {}
                    std::path::Component::Prefix(_) | std::path::Component::RootDir => {}
                }
            }
        }

        let mut lines: Vec<String> = Vec::new();

        lines.push(format!("# Patent Search Report: {}", session.topic));
        lines.push(String::new());
        lines.push(format!("**Session ID:** {}  ", session.session_id));
        lines.push(format!("**Created:** {}  ", session.created_at));
        lines.push(format!("**Last Modified:** {}  ", session.modified_at));
        if session.prior_art_cutoff.is_some() {
            lines.push(format!(
                "**Prior Art Cutoff:** {}  ",
                session.prior_art_cutoff.as_deref().unwrap()
            ));
        }
        lines.push(String::new());

        let mut unique_patents: HashMap<String, &PatentHit> = HashMap::new();
        for query in &session.queries {
            for hit in &query.results {
                if !unique_patents.contains_key(&hit.patent_id) {
                    unique_patents.insert(hit.patent_id.clone(), hit);
                }
            }
        }
        let total_queries = session.queries.len();
        let total_patents = unique_patents.len();

        lines.push("## Summary".to_string());
        lines.push(String::new());
        lines.push(format!("- **Queries run:** {}", total_queries));
        lines.push(format!("- **Unique patents found:** {}", total_patents));
        if !session.classifications_explored.is_empty() {
            lines.push(format!(
                "- **Classifications explored:** {}",
                session.classifications_explored.join(", ")
            ));
        }
        lines.push(String::new());

        if !session.notes.is_empty() {
            lines.push("## Researcher Notes".to_string());
            lines.push(String::new());
            lines.push(session.notes.clone());
            lines.push(String::new());
        }

        if !unique_patents.is_empty() {
            let relevance_order: HashMap<&str, i32> =
                [("high", 0), ("medium", 1), ("low", 2), ("unknown", 3)]
                    .iter()
                    .cloned()
                    .collect();

            let mut sorted_hits: Vec<&PatentHit> = unique_patents.values().copied().collect();
            sorted_hits.sort_by(|a, b| {
                let ra = relevance_order.get(a.relevance.as_str()).unwrap_or(&3);
                let rb = relevance_order.get(b.relevance.as_str()).unwrap_or(&3);
                ra.cmp(rb).then_with(|| {
                    a.date
                        .as_deref()
                        .unwrap_or("")
                        .cmp(b.date.as_deref().unwrap_or(""))
                })
            });

            lines.push("## Patents Found".to_string());
            lines.push(String::new());
            lines.push("| Patent ID | Title | Date | Relevance | Assignee | Note |".to_string());
            lines.push("|-----------|-------|------|-----------|----------|------|".to_string());
            for hit in &sorted_hits {
                let title = hit.title.as_deref().unwrap_or("").replace('|', "\\|");
                let note = hit.note.replace('|', "\\|");
                let assignee = hit.assignee.as_deref().unwrap_or("").replace('|', "\\|");
                let date = hit.date.as_deref().unwrap_or("");
                lines.push(format!(
                    "| {} | {} | {} | {} | {} | {} |",
                    hit.patent_id, title, date, hit.relevance, assignee, note
                ));
            }
            lines.push(String::new());
        }

        if !session.queries.is_empty() {
            lines.push("## Query History".to_string());
            lines.push(String::new());
            for query in &session.queries {
                lines.push(format!("### {} — {}", query.query_id, query.source));
                lines.push(String::new());
                lines.push(format!("**Timestamp:** {}  ", query.timestamp));
                lines.push(format!("**Query:** `{}`  ", query.query_text));
                lines.push(format!("**Results:** {}  ", query.result_count));
                lines.push(String::new());
                if !query.results.is_empty() {
                    lines.push("| Patent ID | Title | Date | Relevance |".to_string());
                    lines.push("|-----------|-------|------|-----------|".to_string());
                    for hit in &query.results {
                        let title = hit.title.as_deref().unwrap_or("").replace('|', "\\|");
                        let date = hit.date.as_deref().unwrap_or("");
                        lines.push(format!(
                            "| {} | {} | {} | {} |",
                            hit.patent_id, title, date, hit.relevance
                        ));
                    }
                    lines.push(String::new());
                }
            }
        }

        let report = lines.join("\n");

        let dir = self.dir.clone();
        let out_path = output_path.clone();
        tokio::task::spawn_blocking(move || -> Result<PathBuf> {
            if out_path.is_absolute() {
                if let Ok(canonical_path) = out_path.canonicalize() {
                    let canonical_dir = dir.canonicalize().unwrap_or_else(|_| dir.clone());
                    let canonical_cwd =
                        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                    if !canonical_path.starts_with(&canonical_dir)
                        && !canonical_path.starts_with(&canonical_cwd)
                    {
                        anyhow::bail!(
                            "output_path escapes allowed directories: {}",
                            out_path.display()
                        );
                    }
                }
            }
            fs::write(&out_path, &report)
                .with_context(|| format!("Failed to write report: {}", out_path.display()))?;
            Ok(out_path)
        })
        .await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_manager() -> (TempDir, SessionManager) {
        let tmp = TempDir::new().unwrap();
        let mgr = SessionManager::new(Some(tmp.path().to_path_buf()));
        (tmp, mgr)
    }

    fn make_hit(id: &str) -> PatentHit {
        PatentHit {
            patent_id: id.to_string(),
            title: None,
            date: None,
            assignee: None,
            inventors: vec![],
            abstract_text: None,
            source: String::new(),
            relevance: "unknown".to_string(),
            note: String::new(),
            prior_art: None,
            url: None,
        }
    }

    fn make_query(id: &str, results: Vec<PatentHit>) -> QueryRecord {
        QueryRecord {
            query_id: id.to_string(),
            timestamp: now_iso(),
            source: "USPTO".to_string(),
            query_text: "test query".to_string(),
            result_count: results.len() as i64,
            results,
            metadata: None,
        }
    }

    #[test]
    fn make_slug_basic() {
        assert_eq!(make_slug("Wireless Charging"), "wireless-charging");
    }

    #[test]
    fn make_slug_strips_special_chars() {
        assert_eq!(make_slug("RF/IoT @ 5G!"), "rfiot--5g");
    }

    #[test]
    fn make_slug_truncates_to_30() {
        let long = "a".repeat(50);
        assert_eq!(make_slug(&long).len(), 30);
    }

    #[test]
    fn make_slug_preserves_hyphens() {
        assert_eq!(make_slug("foo-bar baz"), "foo-bar-baz");
    }

    #[tokio::test]
    async fn create_session_persists() {
        let (_tmp, mgr) = make_manager();
        let session = mgr
            .create_session("Wireless Charging", Some("2020-01-01"), "initial note")
            .await
            .unwrap();
        assert!(session.session_id.contains("wireless-charging"));
        assert_eq!(session.topic, "Wireless Charging");
        assert_eq!(session.prior_art_cutoff.as_deref(), Some("2020-01-01"));
        assert_eq!(session.notes, "initial note");
        assert!(session.queries.is_empty());

        let loaded = mgr.load_session(&session.session_id).await.unwrap();
        assert_eq!(loaded.session_id, session.session_id);
        assert_eq!(loaded.topic, session.topic);
    }

    #[tokio::test]
    async fn load_session_not_found() {
        let (_tmp, mgr) = make_manager();
        let result = mgr.load_session("nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn save_session_updates_modified_at() {
        let (_tmp, mgr) = make_manager();
        let session = mgr.create_session("Test", None, "").await.unwrap();
        let first_modified = session.modified_at.clone();

        std::thread::sleep(std::time::Duration::from_millis(10));

        let mut session2 = mgr.load_session(&session.session_id).await.unwrap();
        session2.notes = "updated".to_string();
        mgr.save_session(&mut session2).await.unwrap();

        let reloaded = mgr.load_session(&session.session_id).await.unwrap();
        assert_ne!(reloaded.modified_at, first_modified);
        assert_eq!(reloaded.notes, "updated");
    }

    #[tokio::test]
    async fn atomic_write_no_leftover_tmp() {
        let (_tmp, mgr) = make_manager();
        let session = mgr.create_session("Atomic", None, "").await.unwrap();

        let tmp_path = mgr.dir.join(format!("{}.json.tmp", session.session_id));
        assert!(!tmp_path.exists(), "tmp file should not remain after save");
    }

    #[tokio::test]
    async fn list_sessions_returns_created() {
        let (_tmp, mgr) = make_manager();
        mgr.create_session("Alpha", None, "").await.unwrap();
        mgr.create_session("Beta", None, "").await.unwrap();

        let summaries = mgr.list_sessions(None).await.unwrap();
        assert_eq!(summaries.len(), 2);

        let ids: Vec<&str> = summaries.iter().map(|s| s.session_id.as_str()).collect();
        assert!(ids.iter().any(|id| id.contains("alpha")));
        assert!(ids.iter().any(|id| id.contains("beta")));
    }

    #[tokio::test]
    async fn list_sessions_limit() {
        let (_tmp, mgr) = make_manager();
        mgr.create_session("A", None, "").await.unwrap();
        mgr.create_session("B", None, "").await.unwrap();
        mgr.create_session("C", None, "").await.unwrap();

        let summaries = mgr.list_sessions(Some(2)).await.unwrap();
        assert_eq!(summaries.len(), 2);
    }

    #[tokio::test]
    async fn list_sessions_sorted_by_modified_desc() {
        let (_tmp, mgr) = make_manager();
        let s1 = mgr.create_session("First", None, "").await.unwrap();

        std::thread::sleep(std::time::Duration::from_millis(10));

        mgr.add_note(&s1.session_id, "update first")
            .await
            .unwrap();

        let _s2 = mgr.create_session("Second", None, "").await.unwrap();

        let summaries = mgr.list_sessions(None).await.unwrap();
        assert_eq!(summaries.len(), 2);
        assert!(
            summaries[0].modified_at >= summaries[1].modified_at,
            "sessions should be sorted by modified_at descending"
        );
    }

    #[tokio::test]
    async fn append_query_result() {
        let (_tmp, mgr) = make_manager();
        let session = mgr.create_session("QueryTest", None, "").await.unwrap();

        let hit = make_hit("US1234567");
        let query = make_query("q001", vec![hit]);
        mgr.append_query_result(&session.session_id, query)
            .await
            .unwrap();

        let loaded = mgr.load_session(&session.session_id).await.unwrap();
        assert_eq!(loaded.queries.len(), 1);
        assert_eq!(loaded.queries[0].query_id, "q001");
        assert_eq!(loaded.queries[0].results.len(), 1);
        assert_eq!(loaded.queries[0].results[0].patent_id, "US1234567");
    }

    #[tokio::test]
    async fn add_note_appends() {
        let (_tmp, mgr) = make_manager();
        let session = mgr.create_session("Notes", None, "").await.unwrap();

        mgr.add_note(&session.session_id, "first note")
            .await
            .unwrap();
        mgr.add_note(&session.session_id, "second note")
            .await
            .unwrap();

        let loaded = mgr.load_session(&session.session_id).await.unwrap();
        assert_eq!(loaded.notes, "first note\n\nsecond note");
    }

    #[tokio::test]
    async fn add_note_to_empty_notes() {
        let (_tmp, mgr) = make_manager();
        let session = mgr.create_session("EmptyNotes", None, "").await.unwrap();
        assert_eq!(session.notes, "");

        mgr.add_note(&session.session_id, "hello").await.unwrap();

        let loaded = mgr.load_session(&session.session_id).await.unwrap();
        assert_eq!(loaded.notes, "hello");
    }

    #[tokio::test]
    async fn annotate_patent_updates_hit() {
        let (_tmp, mgr) = make_manager();
        let session = mgr.create_session("Annotate", None, "").await.unwrap();

        let mut hit = make_hit("US9999999");
        hit.title = Some("Test Patent".to_string());
        let query = make_query("q001", vec![hit]);
        mgr.append_query_result(&session.session_id, query)
            .await
            .unwrap();

        mgr.annotate_patent(&session.session_id, "US9999999", "key reference", "high")
            .await
            .unwrap();

        let loaded = mgr.load_session(&session.session_id).await.unwrap();
        let found = loaded.queries[0]
            .results
            .iter()
            .find(|h| h.patent_id == "US9999999")
            .unwrap();
        assert_eq!(found.note, "key reference");
        assert_eq!(found.relevance, "high");
    }

    #[tokio::test]
    async fn annotate_patent_no_match_no_save() {
        let (_tmp, mgr) = make_manager();
        let session = mgr.create_session("NoMatch", None, "").await.unwrap();
        let before = session.modified_at.clone();

        std::thread::sleep(std::time::Duration::from_millis(10));

        mgr.annotate_patent(&session.session_id, "US0000000", "note", "low")
            .await
            .unwrap();

        let after = mgr.load_session(&session.session_id).await.unwrap();
        assert_eq!(
            after.modified_at, before,
            "should not save if no patent matched"
        );
    }

    #[tokio::test]
    async fn index_file_updated() {
        let (_tmp, mgr) = make_manager();
        let session = mgr.create_session("Indexed", None, "").await.unwrap();

        let index_path = mgr.dir.join(".index.json");
        assert!(index_path.exists());

        let data = fs::read_to_string(&index_path).unwrap();
        let idx: IndexFile = serde_json::from_str(&data).unwrap();
        assert_eq!(idx.sessions.len(), 1);
        assert_eq!(idx.sessions[0].session_id, session.session_id);
    }

    #[tokio::test]
    async fn index_file_deduplicates_on_save() {
        let (_tmp, mgr) = make_manager();
        let session = mgr.create_session("Dedup", None, "").await.unwrap();

        let mut loaded = mgr.load_session(&session.session_id).await.unwrap();
        loaded.notes = "updated".to_string();
        mgr.save_session(&mut loaded).await.unwrap();

        let index_path = mgr.dir.join(".index.json");
        let data = fs::read_to_string(&index_path).unwrap();
        let idx: IndexFile = serde_json::from_str(&data).unwrap();
        let count = idx
            .sessions
            .iter()
            .filter(|s| s.session_id == session.session_id)
            .count();
        assert_eq!(count, 1, "session should appear exactly once in index");
    }

    #[tokio::test]
    async fn session_summary_counts() {
        let (_tmp, mgr) = make_manager();
        let session = mgr.create_session("Counts", None, "").await.unwrap();

        let h1 = make_hit("US111");
        let h2 = make_hit("US222");
        let h3 = make_hit("US111");
        let q1 = make_query("q001", vec![h1, h2]);
        let q2 = make_query("q002", vec![h3]);
        mgr.append_query_result(&session.session_id, q1)
            .await
            .unwrap();
        mgr.append_query_result(&session.session_id, q2)
            .await
            .unwrap();

        let summaries = mgr.list_sessions(None).await.unwrap();
        let s = summaries
            .iter()
            .find(|s| s.session_id == session.session_id)
            .unwrap();
        assert_eq!(s.query_count, 2);
        assert_eq!(
            s.patent_count, 2,
            "should deduplicate patents across queries"
        );
    }

    #[tokio::test]
    async fn export_markdown_default_path() {
        let (_tmp, mgr) = make_manager();
        let session = mgr
            .create_session("Export", Some("2019-06-01"), "research notes")
            .await
            .unwrap();

        let mut hit = make_hit("US111");
        hit.title = Some("Widget Patent".to_string());
        hit.date = Some("2018-03-15".to_string());
        hit.relevance = "high".to_string();
        let query = make_query("q001", vec![hit]);
        mgr.append_query_result(&session.session_id, query)
            .await
            .unwrap();

        let output = mgr
            .export_markdown(&session.session_id, None)
            .await
            .unwrap();
        assert!(output.exists());
        assert_eq!(
            output.file_name().unwrap().to_string_lossy(),
            format!("{}-report.md", session.session_id)
        );

        let content = fs::read_to_string(&output).unwrap();
        assert!(content.contains("# Patent Search Report: Export"));
        assert!(content.contains("**Prior Art Cutoff:** 2019-06-01"));
        assert!(content.contains("## Researcher Notes"));
        assert!(content.contains("research notes"));
        assert!(content.contains("## Patents Found"));
        assert!(content.contains("US111"));
        assert!(content.contains("Widget Patent"));
        assert!(content.contains("## Query History"));
        assert!(content.contains("q001"));
    }

    #[tokio::test]
    async fn export_markdown_custom_path() {
        let (_tmp, mgr) = make_manager();
        let session = mgr.create_session("Custom", None, "").await.unwrap();
        let custom = _tmp.path().join("custom-report.md");

        let output = mgr
            .export_markdown(&session.session_id, Some(&custom))
            .await
            .unwrap();
        assert_eq!(output, custom);
        assert!(custom.exists());
    }

    #[tokio::test]
    async fn export_markdown_relevance_sorting() {
        let (_tmp, mgr) = make_manager();
        let session = mgr.create_session("Sort", None, "").await.unwrap();

        let mut h1 = make_hit("US_LOW");
        h1.relevance = "low".to_string();
        h1.title = Some("Low Patent".to_string());

        let mut h2 = make_hit("US_HIGH");
        h2.relevance = "high".to_string();
        h2.title = Some("High Patent".to_string());

        let query = make_query("q001", vec![h1, h2]);
        mgr.append_query_result(&session.session_id, query)
            .await
            .unwrap();

        let output = mgr
            .export_markdown(&session.session_id, None)
            .await
            .unwrap();
        let content = fs::read_to_string(&output).unwrap();

        let high_pos = content.find("US_HIGH").unwrap();
        let low_pos = content.find("US_LOW").unwrap();
        assert!(
            high_pos < low_pos,
            "high relevance should appear before low"
        );
    }

    #[tokio::test]
    async fn export_markdown_pipe_escaping() {
        let (_tmp, mgr) = make_manager();
        let session = mgr.create_session("Pipe", None, "").await.unwrap();

        let mut hit = make_hit("US111");
        hit.title = Some("A|B|C".to_string());
        let query = make_query("q001", vec![hit]);
        mgr.append_query_result(&session.session_id, query)
            .await
            .unwrap();

        let output = mgr
            .export_markdown(&session.session_id, None)
            .await
            .unwrap();
        let content = fs::read_to_string(&output).unwrap();
        assert!(content.contains("A\\|B\\|C"));
    }

    #[tokio::test]
    async fn serde_roundtrip_session() {
        let (_tmp, mgr) = make_manager();
        let session = mgr
            .create_session("Roundtrip", Some("2021-12-31"), "test notes")
            .await
            .unwrap();

        let mut hit = make_hit("US9999");
        hit.title = Some("Roundtrip Patent".to_string());
        hit.relevance = "medium".to_string();
        hit.prior_art = Some(true);
        hit.inventors = vec!["Alice".to_string(), "Bob".to_string()];

        let query = QueryRecord {
            query_id: "q001".to_string(),
            timestamp: "2024-01-01T00:00:00Z".to_string(),
            source: "EPO_OPS".to_string(),
            query_text: "test".to_string(),
            result_count: 1,
            results: vec![hit],
            metadata: Some(serde_json::json!({"key": "value"})),
        };
        mgr.append_query_result(&session.session_id, query)
            .await
            .unwrap();

        let loaded = mgr.load_session(&session.session_id).await.unwrap();
        assert_eq!(loaded.topic, "Roundtrip");
        assert_eq!(loaded.prior_art_cutoff.as_deref(), Some("2021-12-31"));
        assert_eq!(loaded.notes, "test notes");
        assert_eq!(loaded.queries.len(), 1);
        assert_eq!(loaded.queries[0].source, "EPO_OPS");
        assert_eq!(
            loaded.queries[0].results[0].inventors,
            vec!["Alice", "Bob"]
        );
        assert_eq!(loaded.queries[0].results[0].prior_art, Some(true));
        assert_eq!(
            loaded.queries[0].metadata,
            Some(serde_json::json!({"key": "value"}))
        );
    }

    #[test]
    fn patent_hit_serde_roundtrip() {
        let hit = PatentHit {
            patent_id: "US1234".to_string(),
            title: Some("Test".to_string()),
            date: Some("2023-01-01".to_string()),
            assignee: Some("Acme".to_string()),
            inventors: vec!["Alice".to_string()],
            abstract_text: Some("Abstract".to_string()),
            source: "USPTO".to_string(),
            relevance: "high".to_string(),
            note: "important".to_string(),
            prior_art: Some(true),
            url: Some("https://example.com".to_string()),
        };

        let json = serde_json::to_string(&hit).unwrap();
        let roundtrip: PatentHit = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtrip.patent_id, hit.patent_id);
        assert_eq!(roundtrip.title, hit.title);
        assert_eq!(roundtrip.relevance, hit.relevance);
        assert_eq!(roundtrip.prior_art, hit.prior_art);
        assert_eq!(roundtrip.inventors, hit.inventors);
    }

    #[tokio::test]
    async fn empty_session_export() {
        let (_tmp, mgr) = make_manager();
        let session = mgr.create_session("Empty", None, "").await.unwrap();

        let output = mgr
            .export_markdown(&session.session_id, None)
            .await
            .unwrap();
        let content = fs::read_to_string(&output).unwrap();
        assert!(content.contains("# Patent Search Report: Empty"));
        assert!(content.contains("## Summary"));
        assert!(content.contains("**Queries run:** 0"));
        assert!(content.contains("**Unique patents found:** 0"));
        assert!(!content.contains("## Patents Found"));
        assert!(!content.contains("## Researcher Notes"));
    }

    #[tokio::test]
    async fn list_sessions_fallback_without_index() {
        let (_tmp, mgr) = make_manager();
        let session = mgr.create_session("Fallback", None, "").await.unwrap();

        let index_path = mgr.dir.join(".index.json");
        fs::remove_file(&index_path).unwrap();

        let summaries = mgr.list_sessions(None).await.unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].session_id, session.session_id);
    }

    #[tokio::test]
    async fn classifications_in_export() {
        let (_tmp, mgr) = make_manager();
        let mut session = mgr.create_session("Class", None, "").await.unwrap();
        session.classifications_explored = vec!["H02J50".to_string(), "H01F38".to_string()];
        mgr.save_session(&mut session).await.unwrap();

        let loaded = mgr
            .load_session(
                &mgr.list_sessions(None).await.unwrap()[0].session_id,
            )
            .await
            .unwrap();
        let output = mgr
            .export_markdown(&loaded.session_id, None)
            .await
            .unwrap();
        let content = fs::read_to_string(&output).unwrap();
        assert!(content.contains("H02J50"));
        assert!(content.contains("H01F38"));
    }

    #[test]
    fn env_var_override() {
        let tmp = TempDir::new().unwrap();
        std::env::set_var("PATENT_SESSIONS_DIR", tmp.path().to_str().unwrap());
        let mgr = SessionManager::new(None);
        assert_eq!(mgr.dir(), tmp.path());
        std::env::remove_var("PATENT_SESSIONS_DIR");
    }

    #[tokio::test]
    async fn session_id_rejects_path_traversal() {
        let (_tmp, mgr) = make_manager();
        assert!(mgr.load_session("../../etc/passwd").await.is_err());
        assert!(mgr.load_session("foo/bar").await.is_err());
        assert!(mgr.load_session("foo\\bar").await.is_err());
        assert!(mgr.load_session("..").await.is_err());
        assert!(mgr.load_session("").await.is_err());
    }
}
