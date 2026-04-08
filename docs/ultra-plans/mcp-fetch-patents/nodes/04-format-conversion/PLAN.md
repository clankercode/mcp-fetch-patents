# PLAN — 04-format-conversion: Format Conversion Pipeline

*Depends on: 06-config, 07-test-infra (stub PDF fixture)*
*All subprocess calls (pdftotext, tesseract, marker) are mocked in unit tests*

---

## Python Implementation

### T01 — Tool availability check
- **RED**: `test_check_available_tools_all_present` — mock `importlib.util.find_spec` and `shutil.which` to return True; `check_available_tools(config)` returns all True
- **RED**: `test_check_available_tools_none_present` — all mocked as absent; check returns all False, no exception
- **GREEN**: implement `check_available_tools(config) -> dict[str, bool]`

### T02 — pymupdf4llm conversion (mocked import)
- **RED**: `test_pymupdf4llm_converts_stub_pdf` — mock `pymupdf4llm.to_markdown` to return `"# Test Patent\n..."` for the stub PDF path; `ConverterPipeline.pdf_to_markdown(stub_pdf, output_path, metadata)` → `ConversionResult(success=True, converter_used="pymupdf4llm")`
- **GREEN**: implement `_try_pymupdf4llm()` — import + call; catch ImportError
- **REFACTOR**: converter tries them in order from config

### T03 — pdfplumber extraction + table merging
- **RED**: `test_pdfplumber_extracts_tables` — mock `pdfplumber.open()` returning mocked page with table; `_try_pdfplumber(stub_pdf)` → markdown with `| col | col |` rows
- **RED**: `test_pdfplumber_borderless_table_detected` — page with 3 lines all starting with digits + periods (claim-like); verify extracted as structured table (not flat text)
- **RED**: `test_merge_pymupdf_and_pdfplumber` — mock pymupdf4llm returns prose markdown; mock pdfplumber returns 2 tables not in pymupdf output; `_merge_pymupdf4llm_with_pdfplumber(prose_md, tables)` → combined markdown with tables inserted at correct position
- **RED**: `test_merge_no_duplicate_content` — content found in both converters is not duplicated in merged output
- **GREEN**: implement `_try_pdfplumber()` — extract tables; format as markdown
- **GREEN**: implement `_merge_pymupdf4llm_with_pdfplumber(prose, tables)` — find table insertion points by section heading proximity; deduplicate via text similarity threshold (0.8)

### T04 — pdftotext subprocess
- **RED**: `test_pdftotext_called_correctly` — mock `subprocess.run`; verify called with `["pdftotext", "-layout", str(pdf_path), "-"]`; verify output captured
- **RED**: `test_pdftotext_converts_output_to_markdown` — mock subprocess returning "TITLE\nAbstract text"; verify headers added
- **GREEN**: implement `_try_pdftotext()` — subprocess call; post-process output with simple heuristics

### T05 — Marker subprocess (disabled in tests by default)
- **RED**: `test_marker_skipped_when_disabled` — config has `converters_disabled=["marker"]`; call `pdf_to_markdown()`; verify marker's import is never attempted
- **RED**: `test_marker_called_when_enabled` — config has marker enabled; mock marker import; verify called
- **GREEN**: check `config.converters_disabled` before trying marker

### T06 — Converter fallback chain
- **RED**: `test_fallback_to_pdftotext_when_pymupdf4llm_fails` — mock pymupdf4llm to raise `ImportError`; mock pdftotext subprocess; verify `ConversionResult.converter_used == "pdftotext"`
- **RED**: `test_all_converters_fail_returns_error` — mock all to fail; `ConversionResult(success=False)`
- **GREEN**: iterate through `config.converters_order`, try each; return first success

### T07 — Plain text extraction
- **RED**: `test_pdf_to_text` — mock pymupdf `extract_text()` to return text; `pdf_to_text(stub_pdf, output_path)` → output file written
- **GREEN**: implement `pdf_to_text()` using pymupdf `fitz.open().get_text()`

### T08 — Image download
- **RED**: `test_download_images` — mock httpx GET to return PNG bytes; `download_images(["http://mock/fig1.png"], images_dir)` → file written to `images_dir/fig001.png`
- **RED**: `test_download_images_numbered_correctly` — 3 URLs → fig001.png, fig002.png, fig003.png
- **GREEN**: implement `download_images()` with async httpx; save with zero-padded filename

### T09 — Tesseract OCR (subprocess mocked)
- **RED**: `test_ocr_image` — mock `subprocess.run(["tesseract", ...])` returning "Figure 1: Widget"; `ocr_image(image_path)` → "Figure 1: Widget"
- **RED**: `test_ocr_missing_tesseract_returns_none` — mock `shutil.which("tesseract")` → None; `ocr_image()` → None, no exception
- **GREEN**: implement `ocr_image()` — subprocess call; return stdout; None if unavailable

### T10 — Markdown assembly
- **RED**: `test_assemble_markdown_with_metadata` — call `assemble_markdown(base_md="...", metadata=..., images=[ImageResult(...)])` → result contains `# {title}`, `**Inventors:**`, `![Figure 1](images/fig001.png)`, OCR caption
- **RED**: `test_assemble_markdown_no_images` — no images; no Figures section
- **GREEN**: implement `assemble_markdown()` template

### T11 — Graceful degradation (all tools missing)
- **RED**: `test_pipeline_all_tools_missing_returns_error_not_exception` — mock all tools absent; `pdf_to_markdown()` → `ConversionResult(success=False, error="No converters available")`
- **GREEN**: existing fallback chain handles this

### T12 — Full pipeline integration test (mocked)
- **RED**: `test_full_pipeline_produces_patent_md` — given stub PDF + image URLs + metadata; mock all subprocess calls; verify `patent.md` file written with expected structure
- **GREEN**: wire `ConverterPipeline.run(pdf, images, metadata, output_dir)`

---

## Rust Implementation

### T13 — Rust: subprocess calls for pdftotext + tesseract
- Mirror T04 + T09 in Rust using `std::process::Command`
- **Test**: mock by setting `PATENT_PDFTOTEXT_CMD` env var to a test script
- **GREEN**: implement subprocess wrapper in Rust

### T14 — Rust: pymupdf4llm via Python subprocess bridge
- **RED**: `test_pymupdf_subprocess_bridge` — Rust calls `python -m patent_mcp.converters.pymupdf_runner {pdf} {output}` and reads result
- **GREEN**: implement bridge in both Python runner script and Rust caller

### T15 — Parity: Python markdown == Rust markdown for stub PDF
- **RED**: `test_markdown_parity` in `cross_impl/` — same stub PDF, same metadata; Python output == Rust output (normalized whitespace)

---

## Acceptance Criteria
- Unit tests run in <200ms (all subprocess calls mocked)
- No real PDF/image processing in unit tests
- `patent.md` output has consistent structure regardless of which converter was used
- Graceful degradation: missing tools logged as warnings, not errors

## Dependencies
- `06-config`
- `07-test-infra` (T15 — stub PDF fixture)
