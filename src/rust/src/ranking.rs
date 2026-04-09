//! Heuristic reranking for multi-query patent search results.
//!
//! Scores hits by: query-term coverage in title/snippet, date satisfaction,
//! multi-query appearance bonus, and metadata completeness.
//!
//! Mirrors `patent_mcp.search.ranking` (Python) exactly.

use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

/// A single patent hit returned by a search backend.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PatentHit {
    pub patent_id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub assignee: Option<String>,
    #[serde(default)]
    pub inventors: Vec<String>,
    #[serde(default)]
    pub abstract_text: Option<String>,
    #[serde(default)]
    pub source: String,
    #[serde(default = "default_relevance")]
    pub relevance: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub prior_art: Option<bool>,
    #[serde(default)]
    pub url: Option<String>,
}

fn default_relevance() -> String {
    "unknown".to_string()
}

/// A [`PatentHit`] decorated with a computed relevance score.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ScoredHit {
    pub hit: PatentHit,
    pub score: f64,
    pub score_breakdown: HashMap<String, f64>,
    pub query_matches: usize,
}

/// Re-export the planner's SearchIntent for convenience.
pub use crate::planner::SearchIntent;

// ---------------------------------------------------------------------------
// Ranker
// ---------------------------------------------------------------------------

/// Rank patent search results using heuristic scoring.
pub struct SearchRanker;

impl SearchRanker {
    /// Merge hits from multiple queries, deduplicate, score, and sort.
    ///
    /// `hits_by_query` maps query text to the list of hits found by that query.
    /// The same patent may appear in multiple query results -- that is a signal.
    pub fn rank(
        &self,
        hits_by_query: &HashMap<String, Vec<PatentHit>>,
        search_intent: &SearchIntent,
    ) -> Vec<ScoredHit> {
        // Count how many queries found each patent
        let mut query_counts: HashMap<String, usize> = HashMap::new();
        let mut merged: HashMap<String, PatentHit> = HashMap::new();

        for hits in hits_by_query.values() {
            for hit in hits {
                let pid = &hit.patent_id;
                *query_counts.entry(pid.clone()).or_insert(0) += 1;
                // Keep the hit with the most complete metadata
                let dominated = match merged.get(pid) {
                    Some(existing) => metadata_richness(hit) > metadata_richness(existing),
                    None => true,
                };
                if dominated {
                    merged.insert(pid.clone(), hit.clone());
                }
            }
        }

        // Score each unique hit
        let concepts = &search_intent.concepts;
        let date_cutoff = search_intent.date_cutoff.as_deref();

        let mut scored: Vec<ScoredHit> = Vec::with_capacity(merged.len());

        for (pid, hit) in &merged {
            let mut breakdown: HashMap<String, f64> = HashMap::new();

            // Title coverage: fraction of concepts in title * 3.0
            breakdown.insert(
                "title_coverage".to_string(),
                text_coverage(hit.title.as_deref(), concepts) * 3.0,
            );

            // Snippet / abstract coverage
            breakdown.insert(
                "snippet_coverage".to_string(),
                text_coverage(hit.abstract_text.as_deref(), concepts) * 2.0,
            );

            // Multi-query bonus: found by N variants -> high signal
            let n_queries = *query_counts.get(pid).unwrap_or(&1);
            let bonus = ((n_queries as isize - 1).clamp(0, 4)) as f64 * 1.5;
            breakdown.insert("multi_query_bonus".to_string(), bonus);

            // Date satisfaction
            breakdown.insert(
                "date_satisfaction".to_string(),
                date_score(hit.date.as_deref(), date_cutoff),
            );

            // Metadata completeness
            breakdown.insert("completeness".to_string(), metadata_richness(hit) * 0.3);

            let total: f64 = breakdown.values().sum();

            scored.push(ScoredHit {
                hit: hit.clone(),
                score: total,
                score_breakdown: breakdown,
                query_matches: n_queries,
            });
        }

        scored.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scored
    }
}

// ---------------------------------------------------------------------------
// Scoring helpers
// ---------------------------------------------------------------------------

/// Fraction of `concepts` that appear as case-insensitive substrings in `text`.
///
/// Returns 0.0 when `text` is `None` or `concepts` is empty.
fn text_coverage(text: Option<&str>, concepts: &[String]) -> f64 {
    let text = match text {
        Some(t) if !t.is_empty() => t,
        _ => return 0.0,
    };
    if concepts.is_empty() {
        return 0.0;
    }
    let lower = text.to_lowercase();
    let found = concepts
        .iter()
        .filter(|c| {
            let cl = c.to_lowercase();
            if cl.contains(' ') {
                // Multi-word: substring match is specific enough
                lower.contains(&cl)
            } else {
                // Single-word: word-boundary match to avoid "led" matching "assembled"
                let pattern = format!(r"\b{}\b", regex::escape(&cl));
                regex::Regex::new(&pattern)
                    .map(|re| re.is_match(&lower))
                    .unwrap_or_else(|_| lower.contains(&cl))
            }
        })
        .count();
    found as f64 / concepts.len() as f64
}

/// Score based on date relative to cutoff.
///
/// - No cutoff -> 0.5 (neutral)
/// - Before or equal to cutoff -> 1.0
/// - After cutoff -> 0.0
/// - No date on hit -> 0.3 (slight penalty for unknown)
fn date_score(date_str: Option<&str>, cutoff: Option<&str>) -> f64 {
    let cutoff = match cutoff {
        Some(c) if !c.is_empty() => c,
        _ => return 0.5,
    };
    let date_str = match date_str {
        Some(d) if !d.is_empty() => d,
        _ => return 0.3,
    };

    // Normalise to comparable strings (YYYYMMDD): strip non-digits, take first 8
    let hit_date: String = date_str
        .chars()
        .filter(|c| c.is_ascii_digit())
        .take(8)
        .collect();
    let cut_date: String = cutoff
        .chars()
        .filter(|c| c.is_ascii_digit())
        .take(8)
        .collect();

    if hit_date.is_empty() || hit_date.len() < 4 {
        return 0.3;
    }

    if hit_date <= cut_date {
        1.0
    } else {
        0.0
    }
}

/// Score 0.0-5.0 based on how many metadata fields are populated.
fn metadata_richness(hit: &PatentHit) -> f64 {
    // Match Python's falsy semantics: None and "" both count as absent
    let mut score = 0.0_f64;
    if hit.title.as_deref().is_some_and(|s| !s.is_empty()) {
        score += 1.0;
    }
    if hit.abstract_text.as_deref().is_some_and(|s| !s.is_empty()) {
        score += 1.0;
    }
    if hit.assignee.as_deref().is_some_and(|s| !s.is_empty()) {
        score += 1.0;
    }
    if !hit.inventors.is_empty() {
        score += 1.0;
    }
    if hit.date.as_deref().is_some_and(|s| !s.is_empty()) {
        score += 1.0;
    }
    score
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to build a minimal SearchIntent for testing.
    fn make_intent(concepts: Vec<String>, date_cutoff: Option<String>) -> SearchIntent {
        SearchIntent {
            raw_description: String::new(),
            concepts,
            synonyms: HashMap::new(),
            exclusions: vec![],
            date_cutoff,
            jurisdictions: vec![],
            query_variants: vec![],
            rationale: String::new(),
        }
    }

    /// Helper to build a minimal PatentHit for testing.
    fn make_hit(id: &str) -> PatentHit {
        PatentHit {
            patent_id: id.to_string(),
            title: None,
            date: None,
            assignee: None,
            inventors: vec![],
            abstract_text: None,
            source: String::new(),
            relevance: String::new(),
            note: String::new(),
            prior_art: None,
            url: None,
        }
    }

    // -----------------------------------------------------------------------
    // text_coverage
    // -----------------------------------------------------------------------

    #[test]
    fn text_coverage_full_match() {
        let concepts = vec!["neural".to_string(), "network".to_string()];
        let score = text_coverage(Some("Neural Network Architecture"), &concepts);
        assert!(
            (score - 1.0).abs() < f64::EPSILON,
            "expected 1.0, got {score}"
        );
    }

    #[test]
    fn text_coverage_partial_match() {
        let concepts = vec!["neural".to_string(), "quantum".to_string()];
        let score = text_coverage(Some("Neural Network Architecture"), &concepts);
        assert!(
            (score - 0.5).abs() < f64::EPSILON,
            "expected 0.5, got {score}"
        );
    }

    #[test]
    fn text_coverage_no_match() {
        let concepts = vec!["quantum".to_string(), "entanglement".to_string()];
        let score = text_coverage(Some("Neural Network Architecture"), &concepts);
        assert!(
            (score - 0.0).abs() < f64::EPSILON,
            "expected 0.0, got {score}"
        );
    }

    #[test]
    fn text_coverage_none_text() {
        let concepts = vec!["neural".to_string()];
        let score = text_coverage(None, &concepts);
        assert!((score - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn text_coverage_empty_concepts() {
        let score = text_coverage(Some("anything"), &[]);
        assert!((score - 0.0).abs() < f64::EPSILON);
    }

    // -----------------------------------------------------------------------
    // date_score
    // -----------------------------------------------------------------------

    #[test]
    fn date_score_before_cutoff() {
        let s = date_score(Some("2020-01-15"), Some("2021-06-01"));
        assert!((s - 1.0).abs() < f64::EPSILON, "expected 1.0, got {s}");
    }

    #[test]
    fn date_score_after_cutoff() {
        let s = date_score(Some("2023-03-01"), Some("2021-06-01"));
        assert!((s - 0.0).abs() < f64::EPSILON, "expected 0.0, got {s}");
    }

    #[test]
    fn date_score_no_cutoff() {
        let s = date_score(Some("2020-01-15"), None);
        assert!((s - 0.5).abs() < f64::EPSILON, "expected 0.5, got {s}");
    }

    #[test]
    fn date_score_no_date() {
        let s = date_score(None, Some("2021-06-01"));
        assert!((s - 0.3).abs() < f64::EPSILON, "expected 0.3, got {s}");
    }

    #[test]
    fn date_score_equal_to_cutoff() {
        let s = date_score(Some("2021-06-01"), Some("2021-06-01"));
        assert!((s - 1.0).abs() < f64::EPSILON, "expected 1.0, got {s}");
    }

    #[test]
    fn date_score_short_date() {
        // Less than 4 digits -> treated as unparseable
        let s = date_score(Some("99"), Some("2021-06-01"));
        assert!((s - 0.3).abs() < f64::EPSILON, "expected 0.3, got {s}");
    }

    // -----------------------------------------------------------------------
    // multi-query bonus
    // -----------------------------------------------------------------------

    #[test]
    fn multi_query_bonus_single_query() {
        let ranker = SearchRanker;
        let mut hits_by_query = HashMap::new();
        let mut hit = make_hit("US1234");
        hit.title = Some("Test Patent".to_string());
        hits_by_query.insert("query1".to_string(), vec![hit]);

        let intent = make_intent(vec!["test".to_string()], None);

        let scored = ranker.rank(&hits_by_query, &intent);
        assert_eq!(scored.len(), 1);
        let bonus = scored[0].score_breakdown.get("multi_query_bonus").unwrap();
        assert!(
            (*bonus - 0.0).abs() < f64::EPSILON,
            "single query should give 0.0 bonus, got {bonus}"
        );
        assert_eq!(scored[0].query_matches, 1);
    }

    #[test]
    fn multi_query_bonus_three_queries() {
        let ranker = SearchRanker;
        let mut hits_by_query = HashMap::new();

        for i in 1..=3 {
            let mut hit = make_hit("US1234");
            hit.title = Some("Test Patent".to_string());
            hits_by_query.insert(format!("query{i}"), vec![hit]);
        }

        let intent = make_intent(vec!["test".to_string()], None);

        let scored = ranker.rank(&hits_by_query, &intent);
        assert_eq!(scored.len(), 1);
        let bonus = scored[0].score_breakdown.get("multi_query_bonus").unwrap();
        // min(3-1, 4) * 1.5 = 2 * 1.5 = 3.0
        assert!(
            (*bonus - 3.0).abs() < f64::EPSILON,
            "expected 3.0, got {bonus}"
        );
        assert_eq!(scored[0].query_matches, 3);
    }

    #[test]
    fn multi_query_bonus_capped_at_four() {
        let ranker = SearchRanker;
        let mut hits_by_query = HashMap::new();

        for i in 1..=7 {
            let mut hit = make_hit("US1234");
            hit.title = Some("Test Patent".to_string());
            hits_by_query.insert(format!("query{i}"), vec![hit]);
        }

        let intent = make_intent(vec!["test".to_string()], None);

        let scored = ranker.rank(&hits_by_query, &intent);
        let bonus = scored[0].score_breakdown.get("multi_query_bonus").unwrap();
        // min(7-1, 4) * 1.5 = 4 * 1.5 = 6.0
        assert!(
            (*bonus - 6.0).abs() < f64::EPSILON,
            "expected 6.0 (capped), got {bonus}"
        );
    }

    // -----------------------------------------------------------------------
    // deduplication across queries
    // -----------------------------------------------------------------------

    #[test]
    fn dedup_keeps_richer_hit() {
        let ranker = SearchRanker;
        let mut hits_by_query = HashMap::new();

        // First query: sparse hit
        let sparse = make_hit("US5678");

        // Second query: rich hit with title + abstract
        let mut rich = make_hit("US5678");
        rich.title = Some("Machine Learning Method".to_string());
        rich.abstract_text = Some("A method for ML.".to_string());
        rich.assignee = Some("Acme Corp".to_string());

        hits_by_query.insert("q1".to_string(), vec![sparse]);
        hits_by_query.insert("q2".to_string(), vec![rich]);

        let intent = make_intent(vec!["machine".to_string(), "learning".to_string()], None);

        let scored = ranker.rank(&hits_by_query, &intent);
        assert_eq!(scored.len(), 1, "should deduplicate to 1 hit");
        assert_eq!(scored[0].query_matches, 2);
        // The richer hit should have been kept
        assert!(scored[0].hit.title.is_some(), "should keep the richer hit");
        assert!(scored[0].hit.assignee.is_some());
    }

    #[test]
    fn dedup_multiple_patents_sorted_by_score() {
        let ranker = SearchRanker;
        let mut hits_by_query = HashMap::new();

        // Patent A: appears in 2 queries, has title matching concepts
        let mut hit_a1 = make_hit("US-A");
        hit_a1.title = Some("Quantum Computing Processor".to_string());
        hit_a1.abstract_text = Some("Quantum computing with entanglement.".to_string());

        let mut hit_a2 = make_hit("US-A");
        hit_a2.title = Some("Quantum Computing Processor".to_string());

        // Patent B: appears in 1 query, fewer matches
        let mut hit_b = make_hit("US-B");
        hit_b.title = Some("Classical Processor".to_string());

        hits_by_query.insert("q1".to_string(), vec![hit_a1, hit_b]);
        hits_by_query.insert("q2".to_string(), vec![hit_a2]);

        let intent = make_intent(vec!["quantum".to_string(), "computing".to_string()], None);

        let scored = ranker.rank(&hits_by_query, &intent);
        assert_eq!(scored.len(), 2);
        // Patent A should rank higher (more query matches + better concept coverage)
        assert_eq!(scored[0].hit.patent_id, "US-A");
        assert!(scored[0].score > scored[1].score);
    }

    #[test]
    fn patent_hit_serde_roundtrip() {
        let hit = PatentHit {
            patent_id: "US1234567".to_string(),
            title: Some("Test Patent".to_string()),
            date: Some("2024-01-15".to_string()),
            assignee: Some("Test Corp".to_string()),
            inventors: vec!["Alice".to_string(), "Bob".to_string()],
            abstract_text: Some("An abstract".to_string()),
            source: "USPTO".to_string(),
            relevance: "high".to_string(),
            note: "important".to_string(),
            prior_art: Some(true),
            url: Some("https://example.com".to_string()),
        };
        let json = serde_json::to_value(&hit).unwrap();
        let back: PatentHit = serde_json::from_value(json).unwrap();
        assert_eq!(back.patent_id, hit.patent_id);
        assert_eq!(back.title, hit.title);
        assert_eq!(back.date, hit.date);
        assert_eq!(back.assignee, hit.assignee);
        assert_eq!(back.inventors, hit.inventors);
        assert_eq!(back.abstract_text, hit.abstract_text);
        assert_eq!(back.source, hit.source);
        assert_eq!(back.relevance, hit.relevance);
        assert_eq!(back.note, hit.note);
        assert_eq!(back.prior_art, hit.prior_art);
        assert_eq!(back.url, hit.url);
    }

    #[test]
    fn patent_hit_serde_defaults() {
        let json = serde_json::json!({
            "patent_id": "US0000000"
        });
        let hit: PatentHit = serde_json::from_value(json).unwrap();
        assert_eq!(hit.patent_id, "US0000000");
        assert!(hit.title.is_none());
        assert!(hit.date.is_none());
        assert!(hit.assignee.is_none());
        assert!(hit.inventors.is_empty());
        assert!(hit.abstract_text.is_none());
        assert_eq!(hit.source, "");
        assert_eq!(hit.relevance, "unknown");
        assert_eq!(hit.note, "");
        assert!(hit.prior_art.is_none());
        assert!(hit.url.is_none());
    }
}
