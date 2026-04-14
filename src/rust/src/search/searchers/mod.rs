mod epo_ops;
mod serpapi;
mod uspto;

pub use epo_ops::EpoOpsSearchBackend;
pub use serpapi::SerpApiGooglePatentsBackend;
pub use uspto::UsptoTextSearchBackend;

pub(crate) const SOURCE_SERPAPI: &str = "SerpAPI_Google_Patents";
pub(crate) const SOURCE_USPTO: &str = "USPTO_PPUBS";
pub(crate) const SOURCE_EPO_OPS: &str = "EPO_OPS";

use anyhow::{anyhow, Result};

fn validate_path_segment(input: &str, label: &str) -> Result<()> {
    if input.contains('/')
        || input.contains('\\')
        || input.contains("..")
        || input.contains('?')
        || input.contains('#')
        || input.chars().any(|c| c.is_whitespace())
    {
        return Err(anyhow!(
            "Invalid {} '{}' contains forbidden characters",
            label,
            input
        ));
    }
    Ok(())
}

fn validate_classification_code(input: &str) -> Result<()> {
    if input.contains('\\')
        || input.contains("..")
        || input.contains('?')
        || input.contains('#')
        || input.chars().any(|c| c.is_whitespace())
    {
        return Err(anyhow!(
            "Invalid classification code '{}' contains forbidden characters",
            input
        ));
    }
    Ok(())
}

fn local_name(raw: &[u8]) -> String {
    let name = String::from_utf8_lossy(raw);
    match name.rfind(':') {
        Some(pos) => name[pos + 1..].to_string(),
        None => name.to_string(),
    }
}

fn string_or_array_to_vec(v: &serde_json::Value) -> Vec<String> {
    match v {
        v if v.is_string() => vec![v.as_str().unwrap().to_string()],
        v if v.is_array() => v
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|i| i.as_str().map(String::from))
            .collect(),
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_local_name_strips_namespace() {
        assert_eq!(local_name(b"ex:country"), "country");
        assert_eq!(local_name(b"country"), "country");
        assert_eq!(local_name(b"ops:world-patent-data"), "world-patent-data");
    }

    #[test]
    fn test_classification_code_allows_slash_subgroups() {
        validate_classification_code("G06Q50/18").unwrap();
        validate_classification_code("H02J50/10").unwrap();
    }

    #[test]
    fn test_classification_code_rejects_unsafe_characters() {
        assert!(validate_classification_code("G06Q50/18?x=1").is_err());
        assert!(validate_classification_code("G06Q50\\18").is_err());
        assert!(validate_classification_code("G06Q50/../18").is_err());
        assert!(validate_classification_code("G06Q 50/18").is_err());
    }
}
