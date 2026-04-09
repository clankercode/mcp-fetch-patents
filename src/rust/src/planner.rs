//! Natural language search planner — turns plain English into patent search query variants.
//!
//! No LLM dependency. Uses keyword extraction, a static synonym table, and template-based
//! query generation to expand a single description into multiple search formulations.
//!
//! This module mirrors the Python implementation at
//! `src/python/patent_mcp/search/planner.py` exactly.

use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::{HashMap, HashSet};

// ---------------------------------------------------------------------------
// Stop words — filtered out during concept extraction
// ---------------------------------------------------------------------------

static STOP_WORDS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    let mut s = HashSet::new();
    for w in &[
        "a",
        "an",
        "the",
        "and",
        "or",
        "but",
        "in",
        "on",
        "at",
        "to",
        "for",
        "of",
        "with",
        "by",
        "from",
        "as",
        "is",
        "was",
        "are",
        "were",
        "be",
        "been",
        "being",
        "have",
        "has",
        "had",
        "do",
        "does",
        "did",
        "will",
        "would",
        "shall",
        "should",
        "may",
        "might",
        "can",
        "could",
        "must",
        "about",
        "above",
        "after",
        "before",
        "between",
        "into",
        "through",
        "during",
        "against",
        "without",
        "within",
        "along",
        "across",
        "behind",
        "below",
        "beneath",
        "beside",
        "beyond",
        "under",
        "until",
        "upon",
        "that",
        "this",
        "these",
        "those",
        "which",
        "who",
        "whom",
        "whose",
        "what",
        "where",
        "when",
        "why",
        "how",
        "each",
        "every",
        "all",
        "any",
        "both",
        "few",
        "more",
        "most",
        "other",
        "some",
        "such",
        "no",
        "nor",
        "not",
        "only",
        "own",
        "same",
        "so",
        "than",
        "too",
        "very",
        "just",
        "also",
        "then",
        "there",
        "here",
        "now",
        "it",
        "its",
        "they",
        "them",
        "their",
        "we",
        "us",
        "our",
        "you",
        "your",
        "he",
        "him",
        "his",
        "she",
        "her",
        "my",
        "me",
        "i",
        "using",
        "use",
        "used",
        "based",
        "related",
        "new",
        "novel",
        "improved",
        "existing",
        "like",
        "similar",
        "etc",
        "specifically",
        "particularly",
        "especially",
        "generally",
        "typically",
        "works",
        "working",
        "work",
        "make",
        "makes",
        "made",
        "find",
        "look",
        "looking",
        "search",
        "patent",
        "patents",
        "invention",
        "prior",
        "art",
    ] {
        s.insert(*w);
    }
    s
});

// ---------------------------------------------------------------------------
// Synonym table — phrase/term → list of alternatives for patent language
// ---------------------------------------------------------------------------

static SYNONYMS: Lazy<HashMap<&'static str, Vec<&'static str>>> = Lazy::new(|| {
    let mut m = HashMap::new();

    // Power / Energy
    m.insert(
        "wireless charging",
        vec![
            "inductive coupling",
            "contactless power transfer",
            "wireless power transfer",
            "inductive power transfer",
        ],
    );
    m.insert(
        "wireless power",
        vec![
            "contactless power",
            "inductive power",
            "wireless energy transfer",
        ],
    );
    m.insert(
        "battery",
        vec![
            "energy storage device",
            "electrochemical cell",
            "rechargeable cell",
            "accumulator",
        ],
    );
    m.insert(
        "solar cell",
        vec![
            "photovoltaic cell",
            "solar panel",
            "photovoltaic device",
            "PV cell",
        ],
    );
    m.insert(
        "solar",
        vec!["photovoltaic", "solar energy", "solar radiation"],
    );
    m.insert(
        "fuel cell",
        vec!["electrochemical energy converter", "hydrogen fuel cell"],
    );
    m.insert(
        "capacitor",
        vec!["energy storage element", "charge storage device"],
    );
    m.insert(
        "supercapacitor",
        vec![
            "ultracapacitor",
            "electrochemical capacitor",
            "double-layer capacitor",
        ],
    );
    m.insert(
        "transformer",
        vec!["magnetic core", "inductive device", "voltage converter"],
    );
    m.insert("inverter", vec!["power converter", "DC-AC converter"]);

    // Computing / AI
    m.insert(
        "machine learning",
        vec![
            "artificial intelligence",
            "neural network",
            "deep learning",
            "pattern recognition",
        ],
    );
    m.insert(
        "neural network",
        vec!["deep learning model", "artificial neural network", "ANN"],
    );
    m.insert(
        "computer vision",
        vec![
            "image recognition",
            "visual processing",
            "image analysis",
            "object detection",
        ],
    );
    m.insert(
        "natural language processing",
        vec![
            "NLP",
            "text analysis",
            "language understanding",
            "computational linguistics",
        ],
    );
    m.insert(
        "blockchain",
        vec![
            "distributed ledger",
            "decentralized ledger",
            "cryptographic chain",
        ],
    );
    m.insert(
        "cloud computing",
        vec![
            "distributed computing",
            "remote computing",
            "network computing",
        ],
    );
    m.insert(
        "processor",
        vec![
            "CPU",
            "computing unit",
            "microprocessor",
            "processing element",
        ],
    );
    m.insert(
        "memory",
        vec!["storage device", "data storage", "RAM", "cache memory"],
    );
    m.insert(
        "algorithm",
        vec!["computational method", "data processing method"],
    );
    m.insert(
        "encryption",
        vec!["cryptography", "cipher", "encoding", "data security"],
    );
    m.insert(
        "database",
        vec!["data store", "data repository", "data management system"],
    );

    // Manufacturing
    m.insert(
        "3d printing",
        vec![
            "additive manufacturing",
            "rapid prototyping",
            "three-dimensional printing",
            "fused deposition modeling",
        ],
    );
    m.insert(
        "robot",
        vec!["robotic system", "automated manipulator", "robotic device"],
    );
    m.insert("robotic", vec!["automated", "autonomous", "mechanized"]);
    m.insert(
        "sensor",
        vec![
            "detector",
            "transducer",
            "sensing element",
            "measuring device",
        ],
    );
    m.insert(
        "actuator",
        vec!["drive mechanism", "motor", "activating element"],
    );
    m.insert(
        "laser",
        vec!["coherent light source", "optical amplifier", "laser beam"],
    );
    m.insert("welding", vec!["joining", "bonding", "fusion bonding"]);
    m.insert("mold", vec!["mould", "die", "casting form"]);
    m.insert(
        "cnc",
        vec!["computer numerical control", "numerically controlled"],
    );

    // Medical / Bio
    m.insert(
        "drug delivery",
        vec![
            "pharmaceutical delivery",
            "therapeutic delivery",
            "controlled release",
            "drug administration",
        ],
    );
    m.insert(
        "medical device",
        vec![
            "biomedical device",
            "clinical device",
            "therapeutic apparatus",
        ],
    );
    m.insert(
        "implant",
        vec!["prosthesis", "prosthetic device", "biocompatible implant"],
    );
    m.insert(
        "stent",
        vec![
            "vascular scaffold",
            "endovascular implant",
            "tubular implant",
        ],
    );
    m.insert(
        "catheter",
        vec!["intravascular device", "tubular medical device"],
    );
    m.insert("antibody", vec!["immunoglobulin", "monoclonal antibody"]);
    m.insert("protein", vec!["polypeptide", "amino acid sequence"]);
    m.insert(
        "dna",
        vec!["nucleic acid", "polynucleotide", "genetic material"],
    );
    m.insert(
        "gene therapy",
        vec!["genetic therapy", "gene transfer", "gene editing"],
    );
    m.insert("diagnostic", vec!["detection method", "assay", "screening"]);

    // Materials
    m.insert(
        "composite",
        vec![
            "composite material",
            "fiber-reinforced material",
            "laminate",
        ],
    );
    m.insert(
        "polymer",
        vec!["plastic", "resin", "thermoplastic", "synthetic resin"],
    );
    m.insert(
        "semiconductor",
        vec!["integrated circuit", "chip", "transistor", "silicon device"],
    );
    m.insert(
        "nanoparticle",
        vec!["nanomaterial", "nano-sized particle", "nanostructure"],
    );
    m.insert(
        "coating",
        vec!["surface treatment", "film", "layer", "surface coating"],
    );
    m.insert("alloy", vec!["metal composition", "metallic mixture"]);
    m.insert("ceramic", vec!["sintered material", "oxide material"]);
    m.insert(
        "graphene",
        vec!["carbon nanostructure", "two-dimensional carbon"],
    );

    // Transport
    m.insert(
        "autonomous vehicle",
        vec![
            "self-driving vehicle",
            "driverless vehicle",
            "automated driving system",
        ],
    );
    m.insert(
        "electric vehicle",
        vec![
            "EV",
            "electric car",
            "battery electric vehicle",
            "electric motor vehicle",
        ],
    );
    m.insert(
        "lidar",
        vec![
            "light detection and ranging",
            "laser scanner",
            "optical radar",
        ],
    );
    m.insert(
        "radar",
        vec!["radio detection and ranging", "microwave sensor"],
    );

    // Communication
    m.insert(
        "antenna",
        vec!["aerial", "radiator", "electromagnetic radiator"],
    );
    m.insert(
        "wireless",
        vec!["radio frequency", "RF", "electromagnetic", "over-the-air"],
    );
    m.insert(
        "optical fiber",
        vec!["fibre optic", "optical waveguide", "light guide"],
    );
    m.insert("5g", vec!["fifth generation", "new radio", "NR", "mmWave"]);
    m.insert(
        "bluetooth",
        vec!["short-range wireless", "personal area network"],
    );

    // Mechanical / structural
    m.insert(
        "valve",
        vec!["flow control device", "gate valve", "control element"],
    );
    m.insert(
        "bearing",
        vec!["rotational support", "journal bearing", "bushing"],
    );
    m.insert(
        "spring",
        vec!["elastic element", "resilient member", "biasing element"],
    );
    m.insert(
        "gear",
        vec!["toothed wheel", "transmission element", "cogwheel"],
    );
    m.insert("seal", vec!["gasket", "sealing element", "O-ring"]);
    m.insert("hinge", vec!["pivot", "articulation", "rotary joint"]);
    m.insert(
        "filter",
        vec!["filtration device", "separation element", "strainer"],
    );
    m.insert(
        "pump",
        vec!["fluid mover", "compressor", "fluid displacement device"],
    );
    m.insert(
        "heat exchanger",
        vec!["thermal exchanger", "heat transfer device", "radiator"],
    );
    m.insert("turbine", vec!["rotary engine", "turbo machine"]);

    // Optics / Display
    m.insert(
        "display",
        vec!["screen", "monitor", "visual display", "panel"],
    );
    m.insert(
        "led",
        vec![
            "light emitting diode",
            "solid-state light",
            "electroluminescent device",
        ],
    );
    m.insert(
        "oled",
        vec!["organic light emitting diode", "organic electroluminescent"],
    );
    m.insert("lens", vec!["optical element", "refractive element"]);
    m.insert(
        "camera",
        vec!["image sensor", "imaging device", "image capture device"],
    );

    // General patent language
    m.insert("method", vec!["process", "technique", "procedure"]);
    m.insert(
        "device",
        vec!["apparatus", "system", "equipment", "mechanism"],
    );
    m.insert(
        "coupled",
        vec!["connected", "attached", "linked", "joined", "fastened"],
    );
    m.insert(
        "disposed",
        vec!["positioned", "arranged", "located", "situated"],
    );
    m.insert(
        "adjacent",
        vec!["proximate", "near", "neighboring", "abutting"],
    );
    m.insert("layer", vec!["film", "coating", "stratum"]);
    m.insert("surface", vec!["face", "exterior", "outer surface"]);
    m.insert("housing", vec!["enclosure", "casing", "chassis", "body"]);
    m.insert("opening", vec!["aperture", "orifice", "hole", "port"]);
    m.insert("channel", vec!["conduit", "passage", "duct", "groove"]);
    m.insert("substrate", vec!["base", "foundation", "support layer"]);
    m.insert(
        "controller",
        vec!["control unit", "control module", "processor"],
    );
    m.insert(
        "signal",
        vec!["data signal", "electrical signal", "communication signal"],
    );
    m.insert(
        "circuit",
        vec!["electronic circuit", "circuitry", "electrical circuit"],
    );
    m.insert("module", vec!["unit", "component", "assembly"]);
    m.insert("interface", vec!["connection", "port", "coupling"]);

    m
});

/// Pre-sorted synonym keys (longest first) for deterministic multi-word matching.
static SYNONYMS_SORTED: Lazy<Vec<&'static str>> = Lazy::new(|| {
    let mut keys: Vec<&'static str> = SYNONYMS.keys().copied().collect();
    keys.sort_by_key(|k| std::cmp::Reverse(k.len()));
    keys
});

/// Regex for extracting single-word tokens from lowercased text.
/// Allows leading digits to catch terms like "5g".
static WORD_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b[a-z0-9][a-z0-9-]*[a-z0-9]\b").unwrap());

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

/// A single search query generated by the planner.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct QueryVariant {
    pub query: String,
    /// "broad", "synonym_expanded", "title_focused", "quoted_phrase", "concepts_and"
    pub variant_type: String,
    /// "google_patents" | "serpapi" | "any"
    pub backend: String,
    pub rationale: String,
}

/// Structured output from the planner — everything needed to drive a multi-query search.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SearchIntent {
    pub raw_description: String,
    pub concepts: Vec<String>,
    pub synonyms: HashMap<String, Vec<String>>,
    pub exclusions: Vec<String>,
    pub date_cutoff: Option<String>,
    pub jurisdictions: Vec<String>,
    pub query_variants: Vec<QueryVariant>,
    pub rationale: String,
}

// ---------------------------------------------------------------------------
// Planner
// ---------------------------------------------------------------------------

/// Turn a plain-English invention description into multiple patent search queries.
pub struct NaturalLanguagePlanner;

impl NaturalLanguagePlanner {
    pub fn plan(
        &self,
        description: &str,
        date_cutoff: Option<&str>,
        jurisdictions: Option<&[String]>,
    ) -> SearchIntent {
        let concepts = Self::extract_concepts(description);
        let synonyms = Self::expand_synonyms(&concepts);
        let variants = Self::generate_variants(
            description,
            &concepts,
            &synonyms,
            date_cutoff,
            jurisdictions,
        );
        let rationale = Self::build_rationale(&concepts, &synonyms);

        SearchIntent {
            raw_description: description.to_string(),
            concepts,
            synonyms,
            exclusions: vec![],
            date_cutoff: date_cutoff.map(|s| s.to_string()),
            jurisdictions: jurisdictions.map(|j| j.to_vec()).unwrap_or_default(),
            query_variants: variants,
            rationale,
        }
    }

    // ------------------------------------------------------------------
    // Concept extraction
    // ------------------------------------------------------------------

    /// Extract key concepts: multi-word phrases first, then important single words.
    fn extract_concepts(description: &str) -> Vec<String> {
        let mut text = description.to_lowercase();
        text = text.trim().to_string();
        let lower_desc = text.clone();
        let mut found_phrases: Vec<String> = Vec::new();

        // Match multi-word phrases from synonym table (longest first)
        // Use \b word boundaries to avoid false positives like
        // "non-electric vehicle" matching "electric vehicle"
        for phrase in SYNONYMS_SORTED.iter() {
            if !phrase.contains(' ') {
                continue;
            }
            let pattern = format!(r"\b{}\b", regex::escape(phrase));
            if let Ok(re) = Regex::new(&pattern) {
                if re.is_match(&text) {
                    found_phrases.push(phrase.to_string());
                    text = re.replace(&text, " _ ").to_string();
                }
            }
        }

        // Tokenise remaining text
        let words: Vec<String> = WORD_RE
            .find_iter(&text)
            .map(|m| m.as_str().to_string())
            .collect();

        let mut single_words: Vec<String> = words
            .into_iter()
            .filter(|w| w != "_" && w.len() > 1 && !STOP_WORDS.contains(w.as_str()))
            .collect();

        // Also check single-word synonym keys against the original text
        // (catches terms like "5g" that the tokenizer might miss)
        for key in SYNONYMS.keys() {
            if key.contains(' ') {
                continue;
            }
            let pattern = format!(r"\b{}\b", regex::escape(key));
            if let Ok(re) = Regex::new(&pattern) {
                if re.is_match(&lower_desc) && !found_phrases.contains(&key.to_string()) {
                    single_words.push(key.to_string());
                }
            }
        }

        // Promote single words that appear in synonym table
        let mut promoted: Vec<String> = Vec::new();
        let mut remainder: Vec<String> = Vec::new();
        for w in &single_words {
            if SYNONYMS.contains_key(w.as_str()) {
                promoted.push(w.clone());
            } else {
                remainder.push(w.clone());
            }
        }

        // Deduplicate while preserving order
        let mut seen = HashSet::new();
        let mut result: Vec<String> = Vec::new();
        for c in found_phrases.into_iter().chain(promoted).chain(remainder) {
            if seen.insert(c.clone()) {
                result.push(c);
            }
        }
        result
    }

    // ------------------------------------------------------------------
    // Synonym expansion
    // ------------------------------------------------------------------

    /// Look up each concept in the synonym table.
    fn expand_synonyms(concepts: &[String]) -> HashMap<String, Vec<String>> {
        let mut out = HashMap::new();
        for concept in concepts {
            if let Some(alts) = SYNONYMS.get(concept.as_str()) {
                if !alts.is_empty() {
                    out.insert(
                        concept.clone(),
                        alts.iter().map(|s| s.to_string()).collect(),
                    );
                }
            }
        }
        out
    }

    // ------------------------------------------------------------------
    // Query variant generation
    // ------------------------------------------------------------------

    fn generate_variants(
        description: &str,
        concepts: &[String],
        synonyms: &HashMap<String, Vec<String>>,
        _date_cutoff: Option<&str>,
        _jurisdictions: Option<&[String]>,
    ) -> Vec<QueryVariant> {
        let mut variants: Vec<QueryVariant> = Vec::new();

        if description.trim().is_empty() {
            return variants;
        }

        // 1. Broad — raw description as-is
        variants.push(QueryVariant {
            query: description.trim().to_string(),
            variant_type: "broad".to_string(),
            backend: "any".to_string(),
            rationale: "Raw description for maximum recall".to_string(),
        });

        // 2. Synonym-expanded — OR groups for concepts with known synonyms
        if !synonyms.is_empty() {
            let mut parts: Vec<String> = Vec::new();
            for concept in concepts {
                if let Some(alts) = synonyms.get(concept) {
                    // Build OR group: ("wireless charging" OR "inductive coupling" OR ...)
                    let mut options = vec![format!("\"{}\"", concept)];
                    for a in alts.iter().take(3) {
                        options.push(format!("\"{}\"", a));
                    }
                    parts.push(format!("({})", options.join(" OR ")));
                } else {
                    parts.push(concept.clone());
                }
            }
            if !parts.is_empty() {
                // cap at 6 groups to keep query manageable
                let q = parts.into_iter().take(6).collect::<Vec<_>>().join(" AND ");
                variants.push(QueryVariant {
                    query: q,
                    variant_type: "synonym_expanded".to_string(),
                    backend: "any".to_string(),
                    rationale: "Synonym expansion for broader coverage".to_string(),
                });
            }
        }

        // 3. Title-focused — core concepts in title search (Google Patents syntax)
        if !concepts.is_empty() {
            // Pick the 2-3 most important concepts for title search
            let core: Vec<&String> = concepts.iter().take(3).collect();
            let title_parts: String = core
                .iter()
                .map(|c| {
                    if c.contains(' ') {
                        format!("\"{}\"", c)
                    } else {
                        c.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");
            variants.push(QueryVariant {
                query: title_parts,
                variant_type: "title_focused".to_string(),
                backend: "any".to_string(),
                rationale: "Core concepts only — tighter precision".to_string(),
            });
        }

        // 4. Quoted multi-word phrases — exact match for key phrases
        let multi_word: Vec<&String> = concepts.iter().filter(|c| c.contains(' ')).collect();
        if !multi_word.is_empty() {
            let quoted: String = multi_word
                .iter()
                .take(3)
                .map(|p| format!("\"{}\"", p))
                .collect::<Vec<_>>()
                .join(" AND ");
            let remaining_single: Vec<&String> = concepts
                .iter()
                .filter(|c| !c.contains(' '))
                .take(3)
                .collect();
            let full_query = if remaining_single.is_empty() {
                quoted
            } else {
                format!(
                    "{} {}",
                    quoted,
                    remaining_single
                        .iter()
                        .map(|c| c.as_str())
                        .collect::<Vec<_>>()
                        .join(" ")
                )
            };
            variants.push(QueryVariant {
                query: full_query,
                variant_type: "quoted_phrase".to_string(),
                backend: "any".to_string(),
                rationale: "Exact multi-word phrase matching".to_string(),
            });
        }

        // 5. Concepts AND-linked — all single concepts joined
        if concepts.len() >= 2 {
            let and_query: String = concepts
                .iter()
                .take(6)
                .cloned()
                .collect::<Vec<_>>()
                .join(" AND ");
            variants.push(QueryVariant {
                query: and_query,
                variant_type: "concepts_and".to_string(),
                backend: "any".to_string(),
                rationale: "All key concepts required".to_string(),
            });
        }

        variants
    }

    // ------------------------------------------------------------------
    // Rationale
    // ------------------------------------------------------------------

    fn build_rationale(concepts: &[String], synonyms: &HashMap<String, Vec<String>>) -> String {
        let concepts_display: Vec<&str> = concepts.iter().take(8).map(|s| s.as_str()).collect();
        let mut parts = vec![format!(
            "Extracted {} concepts: {}",
            concepts.len(),
            concepts_display.join(", ")
        )];
        if !synonyms.is_empty() {
            // Iterate concepts in order (matching Python's dict insertion order)
            let expanded: Vec<&str> = concepts
                .iter()
                .filter(|c| synonyms.contains_key(c.as_str()))
                .take(5)
                .map(|s| s.as_str())
                .collect();
            parts.push(format!(
                "Synonym expansion available for: {}",
                expanded.join(", ")
            ));
        }
        format!("{}.", parts.join(". "))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concept_extraction_wireless_charging() {
        let concepts =
            NaturalLanguagePlanner::extract_concepts("wireless charging through metal barriers");
        assert!(
            concepts.contains(&"wireless charging".to_string()),
            "Expected 'wireless charging' in concepts: {:?}",
            concepts
        );
    }

    #[test]
    fn synonym_expansion_for_known_terms() {
        let concepts = vec!["wireless charging".to_string(), "battery".to_string()];
        let syns = NaturalLanguagePlanner::expand_synonyms(&concepts);
        assert!(
            syns.contains_key("wireless charging"),
            "Expected synonym entry for 'wireless charging'"
        );
        assert!(
            syns.get("wireless charging")
                .unwrap()
                .contains(&"inductive coupling".to_string()),
            "Expected 'inductive coupling' as synonym for 'wireless charging'"
        );
        assert!(
            syns.contains_key("battery"),
            "Expected synonym entry for 'battery'"
        );
    }

    #[test]
    fn query_variant_types() {
        let planner = NaturalLanguagePlanner;
        let intent = planner.plan("wireless charging through metal barriers", None, None);
        let types: Vec<&str> = intent
            .query_variants
            .iter()
            .map(|v| v.variant_type.as_str())
            .collect();
        assert!(
            types.contains(&"broad"),
            "Expected 'broad' variant, got: {:?}",
            types
        );
        assert!(
            types.contains(&"synonym_expanded"),
            "Expected 'synonym_expanded' variant, got: {:?}",
            types
        );
        assert!(
            types.contains(&"title_focused"),
            "Expected 'title_focused' variant, got: {:?}",
            types
        );
        // "wireless charging" is multi-word, so quoted_phrase should be present
        assert!(
            types.contains(&"quoted_phrase"),
            "Expected 'quoted_phrase' variant, got: {:?}",
            types
        );
        // Multiple concepts, so concepts_and should be present
        assert!(
            types.contains(&"concepts_and"),
            "Expected 'concepts_and' variant, got: {:?}",
            types
        );
    }

    #[test]
    fn empty_description_returns_empty_variants() {
        let planner = NaturalLanguagePlanner;
        let intent = planner.plan("", None, None);
        assert!(
            intent.query_variants.is_empty(),
            "Expected empty variants for empty description"
        );

        let intent2 = planner.plan("   ", None, None);
        assert!(
            intent2.query_variants.is_empty(),
            "Expected empty variants for whitespace-only description"
        );
    }

    #[test]
    fn stop_words_table_matches_python() {
        // Spot-check several stop words that are present in the Python set
        for w in &[
            "a",
            "the",
            "using",
            "patent",
            "invention",
            "prior",
            "art",
            "specifically",
            "particularly",
            "looking",
            "search",
        ] {
            assert!(STOP_WORDS.contains(w), "Expected '{}' in stop words", w);
        }
    }

    #[test]
    fn synonym_table_entry_count_matches_python() {
        // Python has exactly 88 entries in _SYNONYMS
        assert_eq!(
            SYNONYMS.len(),
            88,
            "Expected exactly 88 synonym entries (matching Python), got {}",
            SYNONYMS.len()
        );

        // Check specific entries match Python exactly
        assert_eq!(
            SYNONYMS.get("wireless charging").unwrap(),
            &vec![
                "inductive coupling",
                "contactless power transfer",
                "wireless power transfer",
                "inductive power transfer"
            ]
        );
        assert_eq!(
            SYNONYMS.get("machine learning").unwrap(),
            &vec![
                "artificial intelligence",
                "neural network",
                "deep learning",
                "pattern recognition"
            ]
        );
        assert_eq!(
            SYNONYMS.get("interface").unwrap(),
            &vec!["connection", "port", "coupling"]
        );
    }

    #[test]
    fn concept_extraction_deduplicates() {
        // "wireless" appears both as multi-word phrase part and single word synonym key
        let concepts =
            NaturalLanguagePlanner::extract_concepts("wireless charging wireless device");
        let count = concepts.iter().filter(|c| *c == "wireless").count();
        assert!(
            count <= 1,
            "Expected 'wireless' at most once, found {} times in {:?}",
            count,
            concepts
        );
    }

    #[test]
    fn concepts_and_variant_requires_two_concepts() {
        let planner = NaturalLanguagePlanner;
        // Single short word that is a concept
        let intent = planner.plan("battery", None, None);
        let types: Vec<&str> = intent
            .query_variants
            .iter()
            .map(|v| v.variant_type.as_str())
            .collect();
        // Only one concept, so concepts_and should NOT be present
        assert!(
            !types.contains(&"concepts_and"),
            "Expected no 'concepts_and' with single concept, got: {:?}",
            types
        );
    }
}
