//! Format conversion pipeline — stub for Phase B.
//! Mirrors Python patent_mcp.converters.pipeline module.

use anyhow::Result;
use std::path::{Path, PathBuf};

/// Result of a conversion operation.
#[derive(Debug, Clone)]
pub struct ConversionResult {
    pub success: bool,
    pub output_path: Option<PathBuf>,
    pub converter_used: Option<String>,
    pub error: Option<String>,
}

/// PDF → Markdown conversion pipeline.
pub struct ConverterPipeline {
    order: Vec<String>,
    disabled: Vec<String>,
}

impl ConverterPipeline {
    pub fn new(order: Vec<String>, disabled: Vec<String>) -> Self {
        ConverterPipeline { order, disabled }
    }

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

    fn try_pymupdf4llm(&self, pdf: &Path, out: &Path) -> Result<ConversionResult> {
        // Delegate to Python subprocess: python -m patent_mcp.converters pymupdf4llm <pdf> <out>
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_all_disabled_returns_failure() {
        let pipeline = ConverterPipeline::new(
            vec!["pymupdf4llm".into(), "pdfplumber".into(), "pdftotext".into()],
            vec!["pymupdf4llm".into(), "pdfplumber".into(), "pdftotext".into()],
        );
        let result = pipeline.pdf_to_markdown(
            std::path::Path::new("/nonexistent.pdf"),
            std::path::Path::new("/nonexistent.md"),
        ).unwrap();
        assert!(!result.success);
        assert!(result.error.is_some());
    }
}
