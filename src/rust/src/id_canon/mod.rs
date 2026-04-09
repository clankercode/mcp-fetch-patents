//! Patent ID canonicalization — mirrors Python patent_mcp.id_canon module.
//!
//! Parses any patent ID format (US, EP, WO, JP, CN, KR, AU, CA, NZ, BR, IN,
//! generic ISO, and URL forms) into a canonical struct.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// Canonicalized patent identifier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalPatentId {
    pub raw: String,
    pub canonical: String,
    pub jurisdiction: String,
    pub number: String,
    pub kind_code: Option<String>,
    pub doc_type: String, // "patent" | "application" | "unknown"
    pub filing_year: Option<u32>,
    pub errors: Vec<String>,
}

// ---------------------------------------------------------------------------
// Compiled regex patterns
// ---------------------------------------------------------------------------

static US_GRANTED_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^US\s*([0-9]{6,8})([A-Z][0-9]?)?$").unwrap());

static US_APP_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^US\s*(20[0-9]{2})[/\-]?([0-9]{6,7})([A-Z][0-9]?)?$").unwrap()
});

static US_BARE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([0-9]{6,8})$").unwrap());

static EP_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^EP\s*([0-9]{5,8})([A-Z][0-9]?)?$").unwrap());

static WO_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:WO|PCT)[/\s]*((?:19|20)[0-9]{2})[/\s]*([0-9]{5,8})([A-Z][0-9]?)?$")
        .unwrap()
});

static JP_MODERN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^JP\s*((?:19|20)[0-9]{2})[/\-]?([0-9]{6,7})([A-Z][0-9]?)?$").unwrap()
});

static JP_BARE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^JP\s*([0-9]{7,8})([A-Z][0-9]?)?$").unwrap());

static CN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^CN\s*((?:19|20)?[0-9]{6,10})([A-Z][0-9]?)?$").unwrap());

static KR_APP_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^KR\s*10-([0-9]{4})-([0-9]{7})([A-Z][0-9]?)?$").unwrap());

static KR_GRANTED_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^KR\s*10-?([0-9]{7,8})([A-Z][0-9]?)?$").unwrap());

static AU_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^AU\s*((?:19|20)[0-9]{2}[0-9]{4,7})([A-Z][0-9]?)?$").unwrap()
});

static CA_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^CA\s*([0-9]{5,8})([A-Z][0-9]?)?$").unwrap());

static NZ_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^NZ\s*([0-9]{5,8})([A-Z][0-9]?)?$").unwrap());

static BR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^BR\s*((?:10|11|20|PI|MU|DI)[0-9]{6,8})([A-Z][0-9]?)?$").unwrap()
});

static IN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^IN\s*([0-9]+(?:/[A-Z]+/[0-9]{4})?)([A-Z][0-9]?)?$").unwrap()
});

static ISO_GENERIC_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^([A-Z]{2})\s*([0-9][0-9A-Z]{4,})([A-Z][0-9]?)?$").unwrap());

// URL patterns
static GOOGLE_PATENTS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)patents\.google\.com/patent/([A-Z]{2}[0-9][0-9A-Z]*)").unwrap()
});

static ESPACENET_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)espacenet\.com.*?/([A-Z]{2}[0-9][0-9A-Z]*)(?:/|\?|$)").unwrap()
});

// ---------------------------------------------------------------------------
// Main parse function
// ---------------------------------------------------------------------------

/// Canonicalize a patent ID string into structured form.
///
/// Never panics — invalid IDs return a result with `jurisdiction == "UNKNOWN"`
/// and a non-empty `errors` list.
pub fn canonicalize(raw: &str) -> CanonicalPatentId {
    let trimmed = raw.trim().replace('\u{00a0}', " ");

    // Try URL extraction first
    if let Some(id) = extract_from_url(&trimmed) {
        let mut result = canonicalize(&id);
        result.raw = raw.to_string();
        return result;
    }

    // Normalize separators/whitespace
    let s = trimmed.to_uppercase();

    // US granted
    if let Some(c) = try_us_granted(&s, raw) {
        return c;
    }
    // US application
    if let Some(c) = try_us_application(&s, raw) {
        return c;
    }
    // US bare number
    if let Some(c) = try_us_bare(&s, raw) {
        return c;
    }
    // EP
    if let Some(c) = try_ep(&s, raw) {
        return c;
    }
    // WO / PCT
    if let Some(c) = try_wo(&s, raw) {
        return c;
    }
    // JP modern
    if let Some(c) = try_jp_modern(&s, raw) {
        return c;
    }
    // JP bare
    if let Some(c) = try_jp_bare(&s, raw) {
        return c;
    }
    // CN
    if let Some(c) = try_cn(&s, raw) {
        return c;
    }
    // KR application
    if let Some(c) = try_kr_app(&s, raw) {
        return c;
    }
    // KR granted
    if let Some(c) = try_kr_granted(&s, raw) {
        return c;
    }
    // AU
    if let Some(c) = try_au(&s, raw) {
        return c;
    }
    // CA
    if let Some(c) = try_ca(&s, raw) {
        return c;
    }
    // NZ
    if let Some(c) = try_nz(&s, raw) {
        return c;
    }
    // BR
    if let Some(c) = try_br(&s, raw) {
        return c;
    }
    // IN
    if let Some(c) = try_in(&s, raw) {
        return c;
    }
    // Generic ISO
    if let Some(c) = try_iso_generic(&s, raw) {
        return c;
    }

    // Fallback: unparseable
    // Python canonical: raw_id.upper().replace(" ", ""), or "UNKNOWN" if empty
    let canonical_fallback = {
        let up = raw.to_uppercase().replace(' ', "");
        if up.is_empty() {
            "UNKNOWN".to_string()
        } else {
            up
        }
    };
    CanonicalPatentId {
        raw: raw.to_string(),
        canonical: canonical_fallback,
        jurisdiction: "UNKNOWN".to_string(),
        number: raw.to_string(),
        kind_code: None,
        doc_type: "unknown".to_string(),
        filing_year: None,
        errors: vec![format!("Could not parse patent ID: {:?}", raw)],
    }
}

fn extract_from_url(s: &str) -> Option<String> {
    if let Some(cap) = GOOGLE_PATENTS_RE.captures(s) {
        return Some(cap[1].to_string());
    }
    if let Some(cap) = ESPACENET_RE.captures(s) {
        return Some(cap[1].to_string());
    }
    None
}

fn make_ok(
    raw: &str,
    canonical: String,
    jurisdiction: &str,
    number: String,
    kind_code: Option<String>,
    doc_type: &str,
    filing_year: Option<u32>,
) -> CanonicalPatentId {
    CanonicalPatentId {
        raw: raw.to_string(),
        canonical,
        jurisdiction: jurisdiction.to_string(),
        number,
        kind_code,
        doc_type: doc_type.to_string(),
        filing_year,
        errors: vec![],
    }
}

fn try_us_granted(s: &str, raw: &str) -> Option<CanonicalPatentId> {
    let cap = US_GRANTED_RE.captures(s)?;
    let number = cap[1].to_string();
    let kind = cap.get(2).map(|m| m.as_str().to_string());
    let canonical = format!("US{}", number);
    Some(make_ok(raw, canonical, "US", number, kind, "patent", None))
}

fn try_us_application(s: &str, raw: &str) -> Option<CanonicalPatentId> {
    let cap = US_APP_RE.captures(s)?;
    let year: u32 = cap[1].parse().ok()?;
    let serial = &cap[2];
    let kind = cap.get(3).map(|m| m.as_str().to_string());
    let number = format!("{}{}", year, serial);
    let canonical = format!("US{}", number);
    Some(make_ok(
        raw,
        canonical,
        "US",
        number,
        kind,
        "application",
        Some(year),
    ))
}

fn try_us_bare(s: &str, raw: &str) -> Option<CanonicalPatentId> {
    // Only match bare numbers if they don't look like years
    let cap = US_BARE_RE.captures(s)?;
    let number = cap[1].to_string();
    let canonical = format!("US{}", number);
    Some(make_ok(raw, canonical, "US", number, None, "patent", None))
}

fn try_ep(s: &str, raw: &str) -> Option<CanonicalPatentId> {
    let cap = EP_RE.captures(s)?;
    let number = cap[1].to_string();
    let kind = cap.get(2).map(|m| m.as_str().to_string());
    let canonical = format!("EP{}", number);
    Some(make_ok(raw, canonical, "EP", number, kind, "patent", None))
}

fn try_wo(s: &str, raw: &str) -> Option<CanonicalPatentId> {
    let cap = WO_RE.captures(s)?;
    let year: u32 = cap[1].parse().ok()?;
    let serial = &cap[2];
    let kind = cap.get(3).map(|m| m.as_str().to_string());
    // Python canonical: WO<year><serial> (no slash separator)
    let number = format!("{}{}", year, serial);
    let canonical = format!("WO{}{}", year, serial);
    Some(make_ok(
        raw,
        canonical,
        "WO",
        number,
        kind,
        "application",
        Some(year),
    ))
}

fn try_jp_modern(s: &str, raw: &str) -> Option<CanonicalPatentId> {
    let cap = JP_MODERN_RE.captures(s)?;
    let year: u32 = cap[1].parse().ok()?;
    let serial = &cap[2];
    let kind = cap.get(3).map(|m| m.as_str().to_string());
    let number = format!("{}{}", year, serial);
    let canonical = format!("JP{}", number);
    Some(make_ok(
        raw,
        canonical,
        "JP",
        number,
        kind,
        "application",
        Some(year),
    ))
}

fn try_jp_bare(s: &str, raw: &str) -> Option<CanonicalPatentId> {
    let cap = JP_BARE_RE.captures(s)?;
    let number = cap[1].to_string();
    let kind = cap.get(2).map(|m| m.as_str().to_string());
    let canonical = format!("JP{}", number);
    Some(make_ok(raw, canonical, "JP", number, kind, "patent", None))
}

fn try_cn(s: &str, raw: &str) -> Option<CanonicalPatentId> {
    let cap = CN_RE.captures(s)?;
    let number = cap[1].to_string();
    let kind = cap.get(2).map(|m| m.as_str().to_string());
    // Python canonical includes the kind code suffix: CN<number><kind> (e.g. CN202310001234A)
    let canonical = match &kind {
        Some(k) => format!("CN{}{}", number, k),
        None => format!("CN{}", number),
    };
    Some(make_ok(raw, canonical, "CN", number, kind, "patent", None))
}

fn try_kr_app(s: &str, raw: &str) -> Option<CanonicalPatentId> {
    let cap = KR_APP_RE.captures(s)?;
    let year: u32 = cap[1].parse().ok()?;
    let serial = &cap[2];
    let kind = cap.get(3).map(|m| m.as_str().to_string());
    let number = format!("10-{}-{}", year, serial);
    let canonical = format!("KR{}", number);
    Some(make_ok(
        raw,
        canonical,
        "KR",
        number,
        kind,
        "application",
        Some(year),
    ))
}

fn try_kr_granted(s: &str, raw: &str) -> Option<CanonicalPatentId> {
    let cap = KR_GRANTED_RE.captures(s)?;
    // Python canonical: KR10<number> (no dash separator)
    let number = format!("10{}", &cap[1]);
    let kind = cap.get(2).map(|m| m.as_str().to_string());
    let canonical = format!("KR{}", number);
    Some(make_ok(raw, canonical, "KR", number, kind, "patent", None))
}

fn try_au(s: &str, raw: &str) -> Option<CanonicalPatentId> {
    let cap = AU_RE.captures(s)?;
    let number = cap[1].to_string();
    let kind = cap.get(2).map(|m| m.as_str().to_string());
    let canonical = format!("AU{}", number);
    let year = number.get(..4).and_then(|y| y.parse().ok());
    Some(make_ok(raw, canonical, "AU", number, kind, "patent", year))
}

fn try_ca(s: &str, raw: &str) -> Option<CanonicalPatentId> {
    let cap = CA_RE.captures(s)?;
    let number = cap[1].to_string();
    let kind = cap.get(2).map(|m| m.as_str().to_string());
    let canonical = format!("CA{}", number);
    Some(make_ok(raw, canonical, "CA", number, kind, "patent", None))
}

fn try_nz(s: &str, raw: &str) -> Option<CanonicalPatentId> {
    let cap = NZ_RE.captures(s)?;
    let number = cap[1].to_string();
    let kind = cap.get(2).map(|m| m.as_str().to_string());
    let canonical = format!("NZ{}", number);
    Some(make_ok(raw, canonical, "NZ", number, kind, "patent", None))
}

fn try_br(s: &str, raw: &str) -> Option<CanonicalPatentId> {
    let cap = BR_RE.captures(s)?;
    let number = cap[1].to_string();
    let kind = cap.get(2).map(|m| m.as_str().to_string());
    let canonical = format!("BR{}", number);
    Some(make_ok(raw, canonical, "BR", number, kind, "patent", None))
}

fn try_in(s: &str, raw: &str) -> Option<CanonicalPatentId> {
    let cap = IN_RE.captures(s)?;
    let number = cap[1].to_string();
    let kind = cap.get(2).map(|m| m.as_str().to_string());
    let canonical = format!("IN{}", number);
    Some(make_ok(raw, canonical, "IN", number, kind, "patent", None))
}

fn try_iso_generic(s: &str, raw: &str) -> Option<CanonicalPatentId> {
    // Generic 2-letter ISO code + digits — catch-all for less common jurisdictions
    let cap = ISO_GENERIC_RE.captures(s)?;
    let jur = cap[1].to_string();
    // Exclude already-matched jurisdictions
    if matches!(
        jur.as_str(),
        "US" | "EP" | "WO" | "JP" | "CN" | "KR" | "AU" | "CA" | "NZ" | "BR" | "IN"
    ) {
        return None;
    }
    let number = cap[2].to_string();
    let kind = cap.get(3).map(|m| m.as_str().to_string());
    let canonical = format!("{}{}", jur, number);
    Some(make_ok(raw, canonical, &jur, number, kind, "patent", None))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_us_granted() {
        let r = canonicalize("US7654321");
        assert_eq!(r.canonical, "US7654321");
        assert_eq!(r.jurisdiction, "US");
        assert_eq!(r.doc_type, "patent");
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_us_granted_with_kind() {
        let r = canonicalize("US7654321B2");
        assert_eq!(r.canonical, "US7654321");
        assert_eq!(r.kind_code, Some("B2".to_string()));
    }

    #[test]
    fn test_us_application() {
        let r = canonicalize("US20240123456");
        assert_eq!(r.jurisdiction, "US");
        assert_eq!(r.doc_type, "application");
        assert_eq!(r.filing_year, Some(2024));
    }

    #[test]
    fn test_ep() {
        let r = canonicalize("EP1234567");
        assert_eq!(r.canonical, "EP1234567");
        assert_eq!(r.jurisdiction, "EP");
    }

    #[test]
    fn test_ep_with_kind() {
        let r = canonicalize("EP1234567B1");
        assert_eq!(r.canonical, "EP1234567");
        assert_eq!(r.kind_code, Some("B1".to_string()));
    }

    #[test]
    fn test_wo() {
        let r = canonicalize("WO2024123456");
        assert_eq!(r.jurisdiction, "WO");
        assert_eq!(r.filing_year, Some(2024));
        // Python canonical: WO<year><serial> (no slash separator)
        assert!(r.canonical.starts_with("WO2024"));
        assert!(!r.canonical.contains('/'));
    }

    #[test]
    fn test_wo_slash_format() {
        let r = canonicalize("WO2024/123456");
        assert_eq!(r.jurisdiction, "WO");
    }

    #[test]
    fn test_jp_modern() {
        let r = canonicalize("JP2023123456");
        assert_eq!(r.jurisdiction, "JP");
        assert_eq!(r.filing_year, Some(2023));
    }

    #[test]
    fn test_cn() {
        let r = canonicalize("CN112345678");
        assert_eq!(r.jurisdiction, "CN");
    }

    #[test]
    fn test_kr_granted() {
        let r = canonicalize("KR101234567");
        assert_eq!(r.jurisdiction, "KR");
    }

    #[test]
    fn test_au() {
        let r = canonicalize("AU2023123456");
        assert_eq!(r.jurisdiction, "AU");
    }

    #[test]
    fn test_ca() {
        let r = canonicalize("CA3012345");
        assert_eq!(r.jurisdiction, "CA");
    }

    #[test]
    fn test_nz() {
        let r = canonicalize("NZ123456");
        assert_eq!(r.jurisdiction, "NZ");
    }

    #[test]
    fn test_google_patents_url() {
        let r = canonicalize("https://patents.google.com/patent/US7654321");
        assert_eq!(r.canonical, "US7654321");
        assert_eq!(r.jurisdiction, "US");
    }

    #[test]
    fn test_invalid_id_returns_unknown() {
        let r = canonicalize("NOTAPATENTID");
        assert_eq!(r.jurisdiction, "UNKNOWN");
        assert!(!r.errors.is_empty());
    }

    #[test]
    fn test_empty_string_returns_unknown() {
        let r = canonicalize("");
        assert_eq!(r.jurisdiction, "UNKNOWN");
        assert!(!r.errors.is_empty());
    }

    #[test]
    fn test_case_insensitive_us() {
        let r = canonicalize("us7654321");
        assert_eq!(r.canonical, "US7654321");
        assert_eq!(r.jurisdiction, "US");
    }

    #[test]
    fn test_iso_generic_passthrough() {
        // MX (Mexico) — should use ISO generic fallback
        let r = canonicalize("MX123456");
        assert_eq!(r.jurisdiction, "MX");
        assert!(r.errors.is_empty());
    }

    #[test]
    fn test_parity_with_python_fixtures() {
        // These canonical forms must match the Python implementation exactly.
        // Cross-validated by tests/cross_impl/test_id_canon_parity.py
        let cases = [
            ("US7654321", "US7654321", "US"),
            ("EP1234567B1", "EP1234567", "EP"),
            // Python canonical for WO: no slash separator (WO<year><serial>)
            ("WO2024123456", "WO2024123456", "WO"),
        ];
        for (input, expected_canonical, expected_jur) in cases {
            let r = canonicalize(input);
            assert_eq!(
                r.canonical, expected_canonical,
                "canonical mismatch for {}",
                input
            );
            assert_eq!(
                r.jurisdiction, expected_jur,
                "jurisdiction mismatch for {}",
                input
            );
        }
    }
}
