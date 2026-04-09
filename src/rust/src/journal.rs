//! Per-repo activity journal — appends JSONL records of tool invocations.

use std::path::PathBuf;

/// Activity journal that appends JSONL records to a file.
/// Failures are logged at warn level and never propagated.
pub struct ActivityJournal {
    path: Option<PathBuf>,
}

impl ActivityJournal {
    pub fn new(path: Option<PathBuf>) -> Self {
        ActivityJournal { path }
    }

    fn append_record(&self, record: serde_json::Value) {
        let Some(path) = &self.path else { return };
        let line = match serde_json::to_string(&record) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("Failed to serialize journal record: {}", e);
                return;
            }
        };
        if let Err(e) = self.append_line(&line, path) {
            tracing::warn!("Failed to write activity journal {}: {}", path.display(), e);
        }
    }

    fn append_line(&self, line: &str, path: &std::path::Path) -> std::io::Result<()> {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        writeln!(file, "{}", line)?;
        Ok(())
    }

    pub fn log_fetch(&self, patent_ids: &[String], summary: &serde_json::Value) {
        self.append_record(serde_json::json!({
            "ts": chrono::Utc::now().to_rfc3339(),
            "action": "fetch",
            "patent_ids": patent_ids,
            "results": summary,
        }));
    }

    pub fn log_list(&self, count: usize) {
        self.append_record(serde_json::json!({
            "ts": chrono::Utc::now().to_rfc3339(),
            "action": "list",
            "count": count,
        }));
    }

    pub fn log_metadata(&self, patent_ids: &[String], found: usize, missing: usize) {
        self.append_record(serde_json::json!({
            "ts": chrono::Utc::now().to_rfc3339(),
            "action": "metadata",
            "patent_ids": patent_ids,
            "found": found,
            "missing": missing,
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_journal_append() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("activity.jsonl");
        let j = ActivityJournal::new(Some(path.clone()));
        j.log_fetch(&["US7654321".into()], &serde_json::json!({"total": 1}));
        j.log_list(5);
        j.log_metadata(&["US7654321".into()], 1, 0);

        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.trim().lines().collect();
        assert_eq!(lines.len(), 3);

        let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(first["action"], "fetch");
        assert!(first["ts"].is_string());
        assert_eq!(first["patent_ids"][0], "US7654321");

        let second: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(second["action"], "list");
        assert_eq!(second["count"], 5);

        let third: serde_json::Value = serde_json::from_str(lines[2]).unwrap();
        assert_eq!(third["action"], "metadata");
        assert_eq!(third["found"], 1);
        assert_eq!(third["missing"], 0);
    }

    #[test]
    fn test_none_path_disables_journal() {
        let j = ActivityJournal::new(None);
        // Should not panic or create any file
        j.log_fetch(&["US7654321".into()], &serde_json::json!({"total": 1}));
        j.log_list(0);
        j.log_metadata(&[], 0, 0);
    }

    #[test]
    fn test_non_writable_path_does_not_crash() {
        let j = ActivityJournal::new(Some(PathBuf::from("/nonexistent/dir/activity.jsonl")));
        // Should log a warning but not panic
        j.log_fetch(&["US7654321".into()], &serde_json::json!({"total": 1}));
    }
}
