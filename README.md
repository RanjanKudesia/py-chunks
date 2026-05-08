# py-chunks

[![Python](https://img.shields.io/badge/python-3.9+-blue)](https://www.python.org/downloads/) [![License](https://img.shields.io/badge/license-MIT-green)](LICENSE)

Fast, framework-agnostic document chunking library backed by Rust. Extract meaningful content segments from DOCX, PDF, PPTX, TXT, MD, and HTML files—optimized for production use.

## Features

- **6 Document Formats**: DOCX, PDF, PPTX, TXT, Markdown, HTML
- **Zero Dependencies**: Pure Python stdlib; all parsing is Rust-compiled
- **Framework Agnostic**: Local files, bytes, file-like objects, FastAPI uploads, S3 presigned URLs, or raw streams
- **Consistent Output Schema**: Structured chunks with content type, section headings, and format-specific metadata
- **High Performance**: Rust parsing engine with millisecond latency per document
- **Production Ready**: Full type hints, comprehensive error handling, extensive test coverage, Pylint 10.00/10

## Installation

```bash
pip install py-chunks
```

**Requirements**: Python 3.9+

## Quick Start

```python
from py_chunks import get_chunks

# Auto-detects source type and format
chunks = get_chunks("document.pdf")

for chunk in chunks:
    print(chunk["content"])
    print(chunk["content_type"])  # "heading", "plain_paragraph", etc.
```

## Chunking Modes (DOCX)

By default, DOCX files use structural chunking. Pass `mode` to switch strategies:

```python
chunks = get_chunks("file.docx", mode="structural")   # default
chunks = get_chunks("file.docx", mode="section")
chunks = get_chunks("file.docx", mode="semantic")
chunks = get_chunks("file.docx", mode="sliding_window")
chunks = get_chunks("file.docx", mode="sentence")
chunks = get_chunks("file.docx", mode="page_aware")
```

### structural (default)
Splits by document structure — headings, paragraphs, tables, lists.
Each element becomes its own chunk with a typed content_type.
content_type values: heading, plain_paragraph, table, bullet_list,
code_block

### section
Groups everything under a heading into one chunk including all
sub-paragraphs, lists, and tables until the next heading of equal
or higher level. Sections exceeding 2000 chars are split automatically.
```python
chunks = get_chunks("file.docx", mode="section")
# chunk["metadata"] contains: section_heading, section_level, heading_path
```

### semantic
Groups paragraphs by topic continuity using pure heuristics — no
ML models or external dependencies.
Signals used (in priority order):
- Reference continuity: paragraph starts with This/It/They/These
- Transition keywords: However/Therefore/In contrast -> new chunk
- Keyword overlap: shared significant words -> merge
- Short paragraph merging: paragraphs under 80 chars merged with next
- Max chunk size: 1500 chars hard limit
```python
chunks = get_chunks("file.docx", mode="semantic")
# chunk["metadata"] contains: paragraph_count, merge_reason
```

### sliding_window
Fixed-size overlapping windows of paragraphs. Useful for RAG with
overlap context.
```python
chunks = get_chunks("file.docx", mode="sliding_window",
                    window_size=3, overlap=1)
# defaults: window_size=3, overlap=1
# chunk["metadata"] contains: window_size, overlap, window_index,
#                              paragraph_indices
```

### sentence
Splits content into sentence-level chunks grouped by
sentences_per_chunk. Sentence boundaries detected without NLP
libraries using punctuation rules. Handles common abbreviations
(Mr. Dr. vs. etc.) without false splits.
```python
chunks = get_chunks("file.docx", mode="sentence",
                    sentences_per_chunk=3)
# defaults: sentences_per_chunk=3
# chunk["metadata"] contains: sentences_per_chunk,
#                              actual_sentence_count, chunk_index,
#                              source_paragraph_index
```

### page_aware
Approximates page boundaries using 3 signals in priority order:
1. Explicit page breaks (w:pageBreak) — most reliable
2. Section breaks (w:sectPr) — reliable
3. Paragraph count approximation — fallback (~15 per page)
```python
chunks = get_chunks("file.docx", mode="page_aware",
                    paragraphs_per_page=15)
# defaults: paragraphs_per_page=15
# chunk["metadata"] contains: page_number, page_break_type,
#                              paragraph_count
# page_break_type values: "explicit", "section", "estimated"
```

Note: Chunking modes currently apply to DOCX only. PDF, PPTX, HTML,
MD, and TXT use structural chunking by default.

## Supported Sources

The library accepts documents from multiple sources with automatic type detection:

| Source | Method | Example |
|--------|--------|---------|
| Local File Path | `get_chunks()` or `get_chunks_from_path()` | `get_chunks("report.pdf")` |
| Raw Bytes | `get_chunks_from_bytes()` | `get_chunks_from_bytes(data, filename="report.pdf")` |
| File-like Object | `get_chunks_from_fileobj()` | `get_chunks_from_fileobj(BytesIO(...), filename="doc.md")` |
| FastAPI Upload | `get_chunks_from_upload()` | `get_chunks_from_upload(file)` |
| S3 Presigned URL | `get_chunks_from_s3_presigned_url()` | `get_chunks_from_s3_presigned_url(url)` |

## Supported Formats

| Format      | Extensions       |
|-------------|------------------|
| PDF         | .pdf             |
| DOCX        | .docx            |
| PPTX        | .pptx            |
| Markdown    | .md              |
| HTML        | .html, .htm      |
| Plain Text  | .txt             |

## Usage Examples

**From local file:**
```python
from py_chunks import get_chunks

chunks = get_chunks("report.pdf")
```

**From bytes (API upload):**
```python
from py_chunks import get_chunks_from_bytes

file_bytes = request.files['document'].read()
chunks = get_chunks_from_bytes(file_bytes, filename="report.pdf")
```

**From FastAPI upload:**
```python
from fastapi import FastAPI, File, UploadFile
from py_chunks import get_chunks_from_upload

app = FastAPI()

@app.post("/upload/")
async def chunk_document(file: UploadFile = File(...)):
    chunks = get_chunks_from_upload(file)
    return {"chunks": chunks}
```

**From file-like object:**
```python
from py_chunks import get_chunks_from_fileobj
from io import BytesIO

bio = BytesIO(file_data)
chunks = get_chunks_from_fileobj(bio, filename="document.md")
```

**From S3 presigned URL:**
```python
from py_chunks import get_chunks_from_s3_presigned_url

url = "https://bucket.s3.amazonaws.com/file.docx?AWSAccessKeyId=..."
chunks = get_chunks_from_s3_presigned_url(url)
```

## API Reference

### Universal Entry Point

#### `get_chunks(source, *, filename=None, mode="structural", window_size=3, overlap=1, sentences_per_chunk=3, paragraphs_per_page=15) -> list[dict]`

Accepts any source type and automatically detects format and dispatch target.

**Parameters:**
- `source`: `str`, `PathLike`, `bytes`, file-like object, upload object, or URL
- `filename`: Optional filename to disambiguate format (required for bytes/file objects without `.filename` attribute)

**Returns:** `list[dict]` – Each chunk is a dictionary with:
- `content`: `str` – Extracted text segment
- `content_type`: `str` – Content classification
- `metadata`: `dict` – Section heading, page number, source type, format-specific fields

**Raises:** `FileNotFoundError`, `ValueError`, `TypeError` with descriptive messages

### Format-Specific Entry Points

- `get_chunks_from_path(file_path: str) -> list[dict]` – Local filesystem
- `get_chunks_from_bytes(data: bytes, filename: str) -> list[dict]` – Raw bytes (temp file auto-cleanup)
- `get_chunks_from_fileobj(file_obj, filename: str | None = None) -> list[dict]` – File-like objects (`open()`, `BytesIO`, etc.)
- `get_chunks_from_upload(upload_file) -> list[dict]` – FastAPI/Starlette uploads (handles async)
- `get_chunks_from_s3_presigned_url(url: str, filename: str | None = None, timeout: int = 60) -> list[dict]` – HTTP/HTTPS URLs

### Advanced: Format-Specific Chunkers

Direct access to format parsers (returns timing information):

```python
from py_chunks.chunkers import chunk_pdf, chunk_docx, chunk_html, chunk_md, chunk_pptx, chunk_txt

chunks, timing = chunk_pdf("file.pdf")
print(f"Rust: {timing['rust_ms']}ms, Python: {timing['python_ms']}ms")
```

Each format-specific chunker returns a tuple: `(list[dict], dict)` where the second element contains `rust_ms` and `python_ms` keys.

## Output Schema

### Chunk Structure

```python
{
    "content": "The extracted text segment.",
    "content_type": "plain_paragraph",  # See content types below
    "metadata": {
        "section_heading": "Section Title" | None,
        "footnotes_captions": [...],
        "page_number": 1 | None,
        "document_metadata": {
            "source_type": "pdf",
            # format-specific fields
        }
    }
}
```

### Content Types

- `heading` – Section heading (H1-H6, bold text, etc.)
- `plain_paragraph` – Prose paragraph
- `bullet_list` – Unordered list
- `table` – Tabular data
- `code_block` – Code or preformatted text
- `long_single_paragraph` – Paragraph > 900 characters
- `short_disconnected_paragraph` – Paragraph < 90 characters
- `section` – Heading-scoped grouped content
- `semantic` – Heuristic topic-continuity grouped content
- `sliding_window` – Fixed-size overlapping paragraph windows
- `sentence` – Sentence-count grouped content
- `page_aware` – Page boundary grouped content

## Architecture

### System Design

```
┌──────────────────────────────────┐
│     Python Public API            │
│   (py_chunks/__init__.py)        │
│  ├─ get_chunks()                 │
│  ├─ get_chunks_from_path()       │
│  ├─ get_chunks_from_bytes()      │
│  ├─ get_chunks_from_fileobj()    │
│  ├─ get_chunks_from_upload()     │
│  └─ get_chunks_from_s3_presigned_url() │
└────────────┬─────────────────────┘
             │
             ↓
┌──────────────────────────────────┐
│    Format Dispatcher             │
│  (py_chunks/chunkers/*.py)       │
│  ├─ chunk_pdf()                  │
│  ├─ chunk_docx()                 │
│  ├─ chunk_pptx()                 │
│  ├─ chunk_html()                 │
│  ├─ chunk_md()                   │
│  └─ chunk_txt()                  │
└────────────┬─────────────────────┘
             │
             ↓
┌──────────────────────────────────┐
│     Rust Extension Module        │
│    (src/extensions/*.rs)         │
│  • Format-specific parsing       │
│  • Returns PyDict chunks         │
│  • Timing instrumentation        │
└──────────────────────────────────┘
```

### Design Principles

- **Single Responsibility**: Each format parser handles only its format
- **Framework Agnostic**: Python layer abstracts source complexity; Rust layer is format-specific
- **Temp File Strategy**: Bytes → temp file → Rust → auto-cleanup provides safety without complex FFI
- **Zero Dependencies**: Pure Python stdlib; all parsing is Rust-compiled

## Development & Testing

### Running Tests

```bash
cd py_chunks
python -m pytest -v
```

Expected: 15 integration tests passing

### Code Quality

```bash
python -m pylint py_chunks tests/test_source_apis.py
```

Expected: 10.00/10 score

### Local Development Build

```bash
pip install -e .
```

Rebuilds the Rust extension with local changes.

## Error Handling

All functions raise descriptive exceptions:

```python
from py_chunks import get_chunks

try:
    chunks = get_chunks("missing.pdf")
except FileNotFoundError as e:
    print(e)  # File not found: missing.pdf

try:
    chunks = get_chunks("image.png")
except ValueError as e:
    print(e)  # Unsupported file type '.png'. Supported: .docx, .htm, .html, .md, .pdf, .pptx, .txt
```

## Framework Integration

### Flask

```python
from flask import Flask, request
from py_chunks import get_chunks_from_bytes

app = Flask(__name__)

@app.post("/chunk")
def chunk_document():
    file = request.files['document']
    chunks = get_chunks_from_bytes(file.read(), file.filename)
    return {"chunks": chunks}
```

### FastAPI

```python
from fastapi import FastAPI, File, UploadFile
from py_chunks import get_chunks_from_upload

app = FastAPI()

@app.post("/chunk/")
async def chunk_document(file: UploadFile = File(...)):
    chunks = get_chunks_from_upload(file)
    return {"chunks": chunks}
```

### Django

```python
from django.http import JsonResponse
from py_chunks import get_chunks_from_upload

def chunk_view(request):
    if request.FILES:
        file = request.FILES['document']
        chunks = get_chunks_from_upload(file)
        return JsonResponse({"chunks": chunks})
    return JsonResponse({"error": "No file"}, status=400)
```

### Celery Background Job

```python
import celery
from py_chunks import get_chunks

@celery.task
def process_document(file_path: str):
    chunks = get_chunks(file_path)
    # Persist to database
    return len(chunks)
```

## License

MIT

---

Built with Rust (performance) + Python (simplicity)
