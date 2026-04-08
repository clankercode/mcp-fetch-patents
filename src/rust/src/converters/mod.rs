//! Format conversion pipeline — mirrors Python patent_mcp.converters.pipeline module.
//!
//! Provides PDF-to-Markdown/Text conversion, image downloading with OCR,
//! and final markdown assembly with patent metadata.

use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::cache::PatentMetadata;
use crate::config::PatentConfig;

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

/// Result of a conversion operation.
#[derive(Debug, Clone)]
pub struct ConversionResult {
    pub success: bool,
    pub output_path: Option<PathBuf>,
    pub converter_used: Option<String>,
    pub error: Option<String>,
}

/// Result of downloading (and optionally OCR-ing) a patent figure image.
#[derive(Debug, Clone)]
pub struct ImageResult {
    pub url: String,
    pub local_path: PathBuf,
    pub ocr_text: Option<String>,
    pub figure_number: u32,
}

// ---------------------------------------------------------------------------
// Tool availability
// ---------------------------------------------------------------------------

/// Check whether a single converter tool is available on this system.
///
/// - For Python-based tools (`pymupdf4llm`, `pdfplumber`, `marker`): runs
///   `python3 -c "import <name>"` and checks the exit code.
/// - For binary tools (`pdftotext`, `tesseract`): runs `which <name>`.
/// - For unknown names: returns `false`.
pub fn check_tool_available(name: &str) -> bool {
    match name {
        "pymupdf4llm" | "pdfplumber" | "marker" => {
            std::process::Command::new("python3")
                .args(["-c", &format!("import {name}")])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        }
        "pdftotext" | "tesseract" => {
            std::process::Command::new("which")
                .arg(name)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        }
        _ => false,
    }
}

/// Check availability of all configured converter tools plus tesseract.
///
/// Iterates `config.converters_order`, skipping disabled tools (marking them
/// `false`), and always adds a `"tesseract"` entry.  Matches the Python
/// `check_available_tools()` behavior exactly.
pub fn check_available_tools(config: &PatentConfig) -> HashMap<String, bool> {
    let mut results = HashMap::new();
    for name in &config.converters_order {
        if config.converters_disabled.contains(name) {
            results.insert(name.clone(), false);
            continue;
        }
        results.insert(name.clone(), check_tool_available(name));
    }
    // Always check tesseract additionally
    results.insert("tesseract".to_string(), check_tool_available("tesseract"));
    results
}

// ---------------------------------------------------------------------------
// ConverterPipeline
// ---------------------------------------------------------------------------

/// PDF to Markdown conversion pipeline.
pub struct ConverterPipeline {
    order: Vec<String>,
    disabled: Vec<String>,
}

impl ConverterPipeline {
    pub fn new(order: Vec<String>, disabled: Vec<String>) -> Self {
        ConverterPipeline { order, disabled }
    }

    // -----------------------------------------------------------------------
    // pdf_to_markdown
    // -----------------------------------------------------------------------

    /// Convert PDF to Markdown using the configured pipeline.
    /// Tries converters in priority order, stops at first success.
    pub fn pdf_to_markdown(&self, pdf_path: &Path, output_path: &Path) -> Result<ConversionResult> {
        for converter in &self.order {
            if self.disabled.contains(converter) {
                continue;
            }
            match converter.as_str() {
                "pymupdf4llm" => {
                    if let Ok(r) = self.try_pymupdf4llm(pdf_path, output_path) {
                        if r.success {
                            return Ok(r);
                        }
                    }
                }
                "pdfplumber" => {
                    if let Ok(r) = self.try_pdfplumber(pdf_path, output_path) {
                        if r.success {
                            return Ok(r);
                        }
                    }
                }
                "pdftotext" => {
                    if let Ok(r) = self.try_pdftotext(pdf_path, output_path) {
                        if r.success {
                            return Ok(r);
                        }
                    }
                }
                "marker" => {
                    if let Ok(r) = self.try_marker(pdf_path, output_path) {
                        if r.success {
                            return Ok(r);
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(ConversionResult {
            success: false,
            output_path: None,
            converter_used: None,
            error: Some("All converters failed or disabled".to_string()),
        })
    }

    // -----------------------------------------------------------------------
    // pdf_to_text
    // -----------------------------------------------------------------------

    /// Extract plain text from a PDF using PyMuPDF (fitz) via a Python subprocess.
    ///
    /// Paths are passed as command-line arguments (sys.argv[1], sys.argv[2]) rather
    /// than interpolated into the script body, preventing path injection vulnerabilities.
    pub fn pdf_to_text(&self, pdf_path: &Path, output_path: &Path) -> Result<ConversionResult> {
        let script = r#"
import sys
import fitz  # PyMuPDF

pdf_path = sys.argv[1]
output_path = sys.argv[2]

try:
    text_parts = []
    with fitz.open(pdf_path) as doc:
        for page in doc:
            text_parts.append(page.get_text())
    text = "\n".join(text_parts)
    with open(output_path, "w", encoding="utf-8") as f:
        f.write(text)
    sys.exit(0)
except Exception as e:
    print(f"Error: {e}", file=sys.stderr)
    sys.exit(1)
"#;

        let status = std::process::Command::new("python3")
            .arg("-c")
            .arg(script)
            .arg(pdf_path)    // sys.argv[1] — path passed safely as argument, not interpolated
            .arg(output_path) // sys.argv[2] — same
            .status();

        match status {
            Ok(s) if s.success() => Ok(ConversionResult {
                success: true,
                output_path: Some(output_path.to_path_buf()),
                converter_used: Some("pymupdf".into()),
                error: None,
            }),
            Ok(s) => Ok(ConversionResult {
                success: false,
                output_path: None,
                converter_used: Some("pymupdf".into()),
                error: Some(format!("pdf_to_text failed with exit code: {}", s.code().unwrap_or(-1))),
            }),
            Err(e) => Ok(ConversionResult {
                success: false,
                output_path: None,
                converter_used: Some("pymupdf".into()),
                error: Some(format!("pdf_to_text subprocess error: {}", e)),
            }),
        }
    }

    // -----------------------------------------------------------------------
    // download_images
    // -----------------------------------------------------------------------

    /// Download images from URLs to `output_dir`, running OCR on each.
    ///
    /// Uses `reqwest::blocking::Client`.  Filename pattern: `fig{i:03}.{ext}`
    /// where `ext` comes from the URL suffix or defaults to `"png"`.
    /// On download failure the `ImageResult` is still emitted with `ocr_text = None`.
    pub fn download_images(&self, image_urls: &[String], output_dir: &Path) -> Vec<ImageResult> {
        if image_urls.is_empty() {
            return Vec::new();
        }

        let _ = std::fs::create_dir_all(output_dir);
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::blocking::Client::new());

        let mut results = Vec::new();

        for (i, url) in image_urls.iter().enumerate() {
            let figure_number = (i + 1) as u32;
            let ext = Path::new(url)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("png");
            let filename = format!("fig{:03}.{}", figure_number, ext);
            let dest = output_dir.join(&filename);

            match client.get(url).send().and_then(|r| r.error_for_status()) {
                Ok(resp) => {
                    let bytes = resp.bytes().unwrap_or_default();
                    let write_ok = std::fs::write(&dest, &bytes).is_ok();
                    let ocr_text = if write_ok {
                        self.ocr_image(&dest)
                    } else {
                        None
                    };
                    results.push(ImageResult {
                        url: url.clone(),
                        local_path: dest,
                        ocr_text,
                        figure_number,
                    });
                }
                Err(_) => {
                    results.push(ImageResult {
                        url: url.clone(),
                        local_path: dest,
                        ocr_text: None,
                        figure_number,
                    });
                }
            }
        }

        results
    }

    // -----------------------------------------------------------------------
    // ocr_image
    // -----------------------------------------------------------------------

    /// Run tesseract OCR on an image file. Returns `None` if tesseract is not
    /// installed, the file does not exist, or OCR produces no text.
    pub fn ocr_image(&self, image_path: &Path) -> Option<String> {
        // Check tesseract availability
        let which_ok = std::process::Command::new("which")
            .arg("tesseract")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !which_ok {
            return None;
        }

        let output = std::process::Command::new("tesseract")
            .arg(image_path)
            .arg("stdout")
            .args(["-l", "eng"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output();

        match output {
            Ok(o) if o.status.success() => {
                let text = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if text.is_empty() {
                    None
                } else {
                    Some(text)
                }
            }
            _ => None,
        }
    }

    // -----------------------------------------------------------------------
    // assemble_markdown
    // -----------------------------------------------------------------------

    /// Assemble final patent markdown from base content, metadata, and images.
    ///
    /// Output format matches the Python `ConverterPipeline.assemble_markdown()`
    /// exactly:
    ///
    /// ```text
    /// # {title or canonical_id}
    ///
    /// **Patent ID:** {canonical_id}
    /// **Inventors:** {inventors joined with ", "}
    /// ...
    ///
    /// ## Abstract
    ///
    /// {abstract_text}
    ///
    /// {base_md}
    ///
    /// ## Figures
    ///
    /// ![Figure {n}]({filename})
    /// *{ocr_text}*
    /// ```
    pub fn assemble_markdown(
        &self,
        base_md: &str,
        metadata: &PatentMetadata,
        images: &[ImageResult],
    ) -> String {
        let mut parts: Vec<String> = Vec::new();

        // Header
        let title = metadata
            .title
            .as_deref()
            .unwrap_or(&metadata.canonical_id);
        parts.push(format!("# {title}"));
        parts.push(String::new());

        // Metadata block
        let mut meta_lines: Vec<String> = Vec::new();
        if !metadata.canonical_id.is_empty() {
            meta_lines.push(format!("**Patent ID:** {}", metadata.canonical_id));
        }
        if !metadata.inventors.is_empty() {
            meta_lines.push(format!("**Inventors:** {}", metadata.inventors.join(", ")));
        }
        if let Some(ref assignee) = metadata.assignee {
            meta_lines.push(format!("**Assignee:** {assignee}"));
        }
        if let Some(ref filing_date) = metadata.filing_date {
            meta_lines.push(format!("**Filing Date:** {filing_date}"));
        }
        if let Some(ref pub_date) = metadata.publication_date {
            meta_lines.push(format!("**Publication Date:** {pub_date}"));
        }
        if let Some(ref grant_date) = metadata.grant_date {
            meta_lines.push(format!("**Grant Date:** {grant_date}"));
        }
        if !meta_lines.is_empty() {
            parts.extend(meta_lines);
            parts.push(String::new());
        }

        // Abstract
        if let Some(ref abstract_text) = metadata.abstract_text {
            if !abstract_text.is_empty() {
                parts.push("## Abstract".to_string());
                parts.push(String::new());
                parts.push(abstract_text.clone());
                parts.push(String::new());
            }
        }

        // Body
        if !base_md.is_empty() {
            parts.push(base_md.to_string());
            parts.push(String::new());
        }

        // Figures
        if !images.is_empty() {
            parts.push("## Figures".to_string());
            parts.push(String::new());
            for img in images {
                let rel_path = img
                    .local_path
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_default();
                parts.push(format!("![Figure {}]({rel_path})", img.figure_number));
                if let Some(ref ocr) = img.ocr_text {
                    parts.push(format!("*{ocr}*"));
                }
                parts.push(String::new());
            }
        }

        parts.join("\n")
    }

    // -----------------------------------------------------------------------
    // Private converter helpers
    // -----------------------------------------------------------------------

    fn try_pymupdf4llm(&self, pdf: &Path, out: &Path) -> Result<ConversionResult> {
        let status = std::process::Command::new("python3")
            .args(["-m", "patent_mcp.converters", "pymupdf4llm"])
            .arg(pdf)
            .arg(out)
            .status();
        match status {
            Ok(s) if s.success() => Ok(ConversionResult {
                success: true,
                output_path: Some(out.to_path_buf()),
                converter_used: Some("pymupdf4llm".into()),
                error: None,
            }),
            _ => Ok(ConversionResult {
                success: false,
                output_path: None,
                converter_used: Some("pymupdf4llm".into()),
                error: Some("pymupdf4llm subprocess failed".into()),
            }),
        }
    }

    fn try_pdfplumber(&self, pdf: &Path, out: &Path) -> Result<ConversionResult> {
        let status = std::process::Command::new("python3")
            .args(["-m", "patent_mcp.converters", "pdfplumber"])
            .arg(pdf)
            .arg(out)
            .status();
        match status {
            Ok(s) if s.success() => Ok(ConversionResult {
                success: true,
                output_path: Some(out.to_path_buf()),
                converter_used: Some("pdfplumber".into()),
                error: None,
            }),
            _ => Ok(ConversionResult {
                success: false,
                output_path: None,
                converter_used: Some("pdfplumber".into()),
                error: Some("pdfplumber subprocess failed".into()),
            }),
        }
    }

    fn try_pdftotext(&self, pdf: &Path, out: &Path) -> Result<ConversionResult> {
        let txt_path = out.with_extension("txt");
        let status = std::process::Command::new("pdftotext")
            .arg("-layout")
            .arg(pdf)
            .arg(&txt_path)
            .status();
        match status {
            Ok(s) if s.success() => Ok(ConversionResult {
                success: true,
                output_path: Some(out.to_path_buf()),
                converter_used: Some("pdftotext".into()),
                error: None,
            }),
            _ => Ok(ConversionResult {
                success: false,
                output_path: None,
                converter_used: Some("pdftotext".into()),
                error: Some("pdftotext subprocess failed".into()),
            }),
        }
    }

    fn try_marker(&self, pdf: &Path, out: &Path) -> Result<ConversionResult> {
        let status = std::process::Command::new("python3")
            .args(["-m", "patent_mcp.converters", "marker"])
            .arg(pdf)
            .arg(out)
            .status();
        match status {
            Ok(s) if s.success() => Ok(ConversionResult {
                success: true,
                output_path: Some(out.to_path_buf()),
                converter_used: Some("marker".into()),
                error: None,
            }),
            _ => Ok(ConversionResult {
                success: false,
                output_path: None,
                converter_used: Some("marker".into()),
                error: Some("marker subprocess failed".into()),
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_all_disabled_returns_failure() {
        let pipeline = ConverterPipeline::new(
            vec!["pymupdf4llm".into(), "pdfplumber".into(), "pdftotext".into()],
            vec!["pymupdf4llm".into(), "pdfplumber".into(), "pdftotext".into()],
        );
        let result = pipeline
            .pdf_to_markdown(
                std::path::Path::new("/nonexistent.pdf"),
                std::path::Path::new("/nonexistent.md"),
            )
            .unwrap();
        assert!(!result.success);
        assert!(result.error.is_some());
    }

    #[test]
    fn test_check_tool_available_unknown() {
        assert!(!check_tool_available("nonexistent_tool_xyz"));
    }

    #[test]
    fn test_check_available_tools_respects_disabled() {
        let config = PatentConfig {
            cache_local_dir: PathBuf::from(".patents"),
            cache_global_db: PathBuf::from("/tmp/test_global.db"),
            source_priority: vec![],
            concurrency: 5,
            fetch_all_sources: false,
            timeout_secs: 30.0,
            converters_order: vec![
                "pymupdf4llm".into(),
                "pdfplumber".into(),
                "pdftotext".into(),
                "marker".into(),
            ],
            converters_disabled: vec![
                "pymupdf4llm".into(),
                "pdfplumber".into(),
                "pdftotext".into(),
                "marker".into(),
            ],
            source_base_urls: HashMap::new(),
            epo_client_id: None,
            epo_client_secret: None,
            lens_api_key: None,
            serpapi_key: None,
            bing_key: None,
            bigquery_project: None,
        };
        let tools = check_available_tools(&config);
        for name in &config.converters_order {
            assert_eq!(
                tools.get(name.as_str()),
                Some(&false),
                "disabled tool {name} should be false"
            );
        }
        // tesseract should always be present (true or false depending on system)
        assert!(tools.contains_key("tesseract"));
    }

    #[test]
    fn test_ocr_image_missing_tesseract_graceful() {
        // ocr_image should return None gracefully if tesseract not found
        // or file doesn't exist — no panic.
        let pipeline = ConverterPipeline::new(vec![], vec![]);
        let result = pipeline.ocr_image(Path::new("/nonexistent/image.png"));
        assert!(result.is_none());
    }

    #[test]
    fn test_download_images_empty_list() {
        let pipeline = ConverterPipeline::new(vec![], vec![]);
        let results = pipeline.download_images(&[], Path::new("/tmp/test_images"));
        assert!(results.is_empty());
    }

    #[test]
    fn test_assemble_markdown_structure() {
        let metadata = PatentMetadata {
            canonical_id: "US1234567".to_string(),
            jurisdiction: "US".to_string(),
            doc_type: "grant".to_string(),
            title: Some("Test Patent Title".to_string()),
            abstract_text: Some("This is the abstract.".to_string()),
            inventors: vec!["Alice".to_string(), "Bob".to_string()],
            assignee: Some("Test Corp".to_string()),
            filing_date: Some("2024-01-01".to_string()),
            publication_date: Some("2024-06-01".to_string()),
            grant_date: None,
            fetched_at: "2024-07-01T00:00:00Z".to_string(),
            legal_status: None,
        };
        let images = vec![ImageResult {
            url: "https://example.com/fig1.png".to_string(),
            local_path: PathBuf::from("/tmp/fig001.png"),
            ocr_text: Some("Figure 1 caption".to_string()),
            figure_number: 1,
        }];
        let pipeline = ConverterPipeline::new(vec![], vec![]);
        let md = pipeline.assemble_markdown("Body content here.", &metadata, &images);

        // Verify structure
        assert!(md.starts_with("# Test Patent Title"));
        assert!(md.contains("**Patent ID:** US1234567"));
        assert!(md.contains("**Inventors:** Alice, Bob"));
        assert!(md.contains("**Assignee:** Test Corp"));
        assert!(md.contains("**Filing Date:** 2024-01-01"));
        assert!(md.contains("**Publication Date:** 2024-06-01"));
        assert!(!md.contains("**Grant Date:**")); // None should be omitted
        assert!(md.contains("## Abstract"));
        assert!(md.contains("This is the abstract."));
        assert!(md.contains("Body content here."));
        assert!(md.contains("## Figures"));
        assert!(md.contains("![Figure 1](fig001.png)"));
        assert!(md.contains("*Figure 1 caption*"));
    }

    #[test]
    fn test_assemble_markdown_no_title_uses_id() {
        let metadata = PatentMetadata {
            canonical_id: "US9999999".to_string(),
            jurisdiction: "US".to_string(),
            doc_type: "grant".to_string(),
            title: None,
            abstract_text: None,
            inventors: vec![],
            assignee: None,
            filing_date: None,
            publication_date: None,
            grant_date: None,
            fetched_at: "2024-07-01T00:00:00Z".to_string(),
            legal_status: None,
        };
        let pipeline = ConverterPipeline::new(vec![], vec![]);
        let md = pipeline.assemble_markdown("", &metadata, &[]);
        assert!(md.starts_with("# US9999999"));
        assert!(!md.contains("## Abstract"));
        assert!(!md.contains("## Figures"));
    }

    #[test]
    fn test_marker_in_pipeline_when_disabled() {
        let pipeline = ConverterPipeline::new(
            vec!["marker".into()],
            vec!["marker".into()],
        );
        let result = pipeline
            .pdf_to_markdown(
                Path::new("/nonexistent.pdf"),
                Path::new("/nonexistent.md"),
            )
            .unwrap();
        assert!(!result.success);
    }
}
