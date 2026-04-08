# SPEC — 04-format-conversion: Format Conversion Pipeline

## Responsibility
Convert downloaded patent artifacts (PDF, raw HTML/XML, images) into structured formats: markdown (primary LLM-friendly format), plain text, and captioned images (via OCR).

## Pipeline Overview
```
Input: PDF file + images dir
  │
  ├─► PDF → Markdown
  │     1. pymupdf4llm (default)
  │     2. pdftotext + cleanup (fallback)
  │     3. marker (optional, disabled in default test config)
  │
  ├─► PDF → Full Text (plain)
  │     pdftotext (poppler) or pymupdf extract_text()
  │
  ├─► Image Download
  │     Source-specific: download each figure URL
  │     Save as images/fig{NNN}.{ext}
  │
  └─► Image → OCR Caption
        tesseract per image → images/fig{NNN}_ocr.txt
        Result embedded in markdown: ![fig](images/fig001.png)
        Caption: figure description from OCR or HTML alt text
```

## Converter Chain for PDF→Markdown

### 1. pymupdf4llm (default, fastest)
- Python: `import pymupdf4llm; md = pymupdf4llm.to_markdown(pdf_path)`
- Rust: call Python as subprocess OR use `pymupdf` Rust binding if available
- Output: well-structured markdown with headers, tables preserved
- Speed: <1s for typical patent PDF on modern hardware
- License: MIT

### 2. pdfplumber (claim tables + structured text)
- Python: `import pdfplumber; with pdfplumber.open(pdf_path) as pdf: ...`
- Rust: subprocess call to Python pdfplumber runner
- Excellent for claim tables (coordinate-based extraction, preserves table structure)
- Use as secondary pass: extract tables that pymupdf4llm misses (borderless tables rendered as plain text by pymupdf4llm)
- Output: combine pdfplumber tables + pymupdf4llm prose for best results

### 3. pdftotext + cleanup (simple text fallback)
- Python: `subprocess.run(["pdftotext", "-layout", pdf_path, "-"])`
- Rust: `std::process::Command::new("pdftotext")`
- Output: plain text with layout hints; post-process to add markdown headers
- Speed: very fast
- Requires: poppler-utils installed

### 4. marker (optional / premium)
- Python: `from marker.convert import convert_single_pdf`
- ML-based: accurate for complex patent layouts, figures, equations
- Heavy: requires model download (~1GB), slow first run
- Disabled by default in tests (`PATENT_DISABLE_MARKER=1` or config)
- Rust: subprocess call to Python marker runner

## Converter Configuration
```toml
[converters]
pdf_to_markdown_order = ["pymupdf4llm", "pdfplumber", "pdftotext", "marker"]
disable = ["marker"]   # disabled converters (skipped entirely)
```
Note: Nougat was evaluated and is NOT suitable for patent PDFs — it overfits to arXiv academic papers and hallucinates on patent content. Do not include Nougat in the converter chain.

## Markdown Assembly
After conversion, assemble final `patent.md`:
```markdown
# {Title}

**Patent ID:** {canonical_id}
**Jurisdiction:** {jurisdiction}
**Inventors:** {inventors}
**Assignee:** {assignee}
**Filed:** {filing_date}
**Published:** {publication_date}

## Abstract

{abstract}

## Description

{description text from converter}

## Claims

{claims from converter}

## Figures

![Figure 1](images/fig001.png)
*{OCR caption for fig001}*

![Figure 2](images/fig002.png)
*{OCR caption for fig002}*
```

## OCR Strategy (tesseract)
- Run `tesseract {image_path} stdout -l eng` for each figure
- Extract text → save as `images/fig{NNN}_ocr.txt`
- Embed caption under figure reference in markdown
- If tesseract not installed: skip OCR, embed image without caption (log warning)
- Tesseract is optional; tests mock its subprocess call

## Subprocess Interface (Python→tools, Rust→tools)
All external tools (pdftotext, tesseract, marker) called via subprocess:
- Python: `subprocess.run([...], capture_output=True)`
- Rust: `std::process::Command::new(...)`
- Identical command construction in both implementations

## Tool Availability Check
On startup, check which tools are available:
```
available_tools = {
  "pymupdf4llm": check_import("pymupdf4llm"),
  "pdftotext": check_binary("pdftotext"),
  "marker": check_import("marker") and not config.disable_marker,
  "tesseract": check_binary("tesseract")
}
```
Log missing tools as warnings; skip gracefully.

## Dependencies
- `pymupdf4llm` Python package
- `poppler-utils` (system package, for `pdftotext`)
- `tesseract-ocr` (system package)
- `marker` (optional Python package)
- `06-config` (for converter order, disabled list)

## Test Surface
- Unit: pymupdf4llm converter tested with tiny fixture PDF (mocked subprocess or real tiny PDF)
- Unit: markdown assembly with all metadata fields
- Unit: OCR caption embedding
- Unit: graceful degradation when tools missing
- Cross-impl: Python markdown output == Rust markdown output for same inputs
- Speed: full conversion pipeline on 1-page test PDF must complete in <100ms
