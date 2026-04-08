# INTERFACE — 04-format-conversion

## Exposes
```python
class ConverterPipeline:
    def __init__(self, config: PatentConfig): ...

    def pdf_to_markdown(self, pdf_path: Path, output_path: Path, 
                         metadata: PatentMetadata) -> ConversionResult: ...
    
    def pdf_to_text(self, pdf_path: Path, output_path: Path) -> ConversionResult: ...
    
    def download_images(self, image_urls: list[str], output_dir: Path) -> list[ImageResult]: ...
    
    def ocr_image(self, image_path: Path) -> str | None: ...
    
    def assemble_markdown(self, base_md: str, metadata: PatentMetadata,
                          images: list[ImageResult]) -> str: ...

@dataclass
class ConversionResult:
    success: bool
    output_path: Path | None
    converter_used: str | None   # "pymupdf4llm" | "pdfplumber" | "pdftotext" | "marker" | None
    error: str | None            # "no_converters_available" | "converter_failed: {name}: {msg}" | None

@dataclass
class ImageResult:
    url: str
    local_path: Path
    ocr_text: str | None
    figure_number: int
```

## Depends On
06-config

## Consumed By
03-source-fetchers (calls converters after download), 07-test-infra
