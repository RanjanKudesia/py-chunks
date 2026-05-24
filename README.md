# py-chunks

[![Python](https://img.shields.io/badge/python-3.9+-blue)](https://www.python.org/downloads/) [![License](https://img.shields.io/badge/license-MIT-green)](LICENSE)

Fast, framework-agnostic document chunking library backed by Rust. Extract meaningful content segments from DOCX, PDF, PPTX, TXT, Markdown, and HTML files — optimised for production use.

## Features

- **6 Document Formats**: PDF, DOCX, PPTX, Markdown, HTML, TXT
- **7 Chunking Modes across every format**: `default`, `structural`, `section`, `semantic`, `sliding_window`, `sentence`, `page_aware`
- **Streaming for every format** via a single `stream_chunks()` entry point
  - PDF: background Rust thread + `mpsc` channel (all 7 modes, true one-chunk-at-a-time)
  - Markdown / HTML / TXT: block-by-block state machine for `structural` + `semantic`; batch-drain for the rest
  - DOCX: all 7 modes — `DocxStructuralIterator` for `default`/`structural`; dedicated per-mode iterators for the remaining 5 modes (lazy chunk emission after a single upfront parse)
  - PPTX: batch-drain (ZIP must be read upfront, then chunks are yielded one at a time)
- **Multiple Input Sources**: local file paths, raw `bytes` / `bytearray` / `memoryview`, file-like objects (`BytesIO`, open files), FastAPI / Starlette `UploadFile`, HTTP(S) / S3 pre-signed URLs
- **Consistent Output Schema**: every chunk is a `dict` with `content`, `content_type`, and `metadata` keys
- **Zero Python runtime dependencies**: all parsing happens in the Rust extension; the PDFium native binary is bundled inside the wheel

## Installation

```bash
pip install py-chunks
```

**Requirements**: Python 3.9+

### PDF native library

PDF chunking uses the PDFium native library, which is bundled inside the wheel — no separate installation needed.

If you need to point to a custom PDFium binary (e.g. a system install or a specific build), set the environment variable before importing:

```bash
export PDFIUM_LIBRARY_PATH=/path/to/libpdfium.dylib   # macOS
export PDFIUM_LIBRARY_PATH=/path/to/libpdfium.so       # Linux
set PDFIUM_LIBRARY_PATH=C:\path\to\pdfium.dll          # Windows
```

---

## Quick Start

```python
from py_chunks import get_chunks, stream_chunks

# Batch — works for every supported format
chunks = get_chunks("document.pdf")
chunks = get_chunks("notes.md",     mode="semantic")
chunks = get_chunks("page.html",    mode="section")
chunks = get_chunks("deck.pptx",    mode="sliding_window", window_size=3, overlap=1)
chunks = get_chunks("report.docx",  mode="sentence",       sentences_per_chunk=3)

for chunk in chunks:
    print(chunk["content"])
    print(chunk["content_type"])   # e.g. "heading", "plain_paragraph", "semantic"
    print(chunk["metadata"])       # format- and mode-specific

# Streaming — works for every supported format
for chunk in stream_chunks("large.pdf", mode="section"):
    handle(chunk)
```

---

## Chunking Modes

The same seven modes are accepted by every format. The implementation is format-specific (e.g. PDF uses font-size analysis, PPTX uses slide structure, MD/HTML use block parsing), but the surface API is uniform:

| Mode | What it does |
|---|---|
| `default` / `structural` | One chunk per structural unit (heading, paragraph, list, table, code block, slide…). For PDF specifically, they are not aliases: `default` uses `chunk_pdf_fast` (lightweight extraction) while `structural` uses `chunk_pdf` (full font-size-weighted analysis), and output may differ. |
| `section` | Groups everything under a heading into a single chunk (≤ 2 000 chars). Adds `section_heading`, `section_level`, `heading_path`. |
| `semantic` | Heuristically merges adjacent blocks by topic continuity using 10 signals (reference pronouns, transition words, elaboration cues, examples, cause/effect, contrast continuation, question/answer, definition expansion, short-paragraph absorption, keyword overlap). Adds `merge_reasons`, `primary_merge_reason`, `paragraph_count`, `keyword_density`. |
| `sliding_window` | Overlapping windows of N blocks. Params: `window_size` (default 3), `overlap` (default 1, must be `< window_size`). |
| `sentence` | N sentences per chunk, detected without NLP (handles abbreviations like `Mr.`, `Dr.`, `e.g.`, numeric markers, initials). Param: `sentences_per_chunk` (default 3, must be `> 0`). |
| `page_aware` | Groups by page boundary where available (PDF page breaks, DOCX `w:pageBreak` / `w:sectPr`, PPTX slides), with a paragraph-count fallback. Param: `paragraphs_per_page` (default 15 for most formats, **5 for PPTX** where it means slides-per-chunk). |

### DOCX modes

Pass `mode` to any `get_chunks` / `stream_chunks` call:

```python
from py_chunks import get_chunks

chunks = get_chunks("file.docx", mode="default")        # structural (default)
chunks = get_chunks("file.docx", mode="structural")     # same as default
chunks = get_chunks("file.docx", mode="section")
chunks = get_chunks("file.docx", mode="semantic")
chunks = get_chunks("file.docx", mode="sliding_window", window_size=3, overlap=1)
chunks = get_chunks("file.docx", mode="sentence",       sentences_per_chunk=3)
chunks = get_chunks("file.docx", mode="page_aware",     paragraphs_per_page=15)
```

| Mode | Description |
|---|---|
| `default` / `structural` | One chunk per document element: heading, paragraph, table, list. Each element typed via `content_type`. |
| `section` | All content under a heading grouped into a single chunk (≤ 2 000 chars). `metadata` includes `section_heading`, `section_level`, `heading_path`. |
| `semantic` | Paragraphs merged by topic continuity using pure heuristics — reference pronouns, transition words, keyword overlap, short-paragraph absorption (≤ 1 500 chars). `metadata` includes `paragraph_count`, `merge_reason`. |
| `sliding_window` | Overlapping paragraph windows. Params: `window_size` (default 3), `overlap` (default 1). `metadata` includes `window_size`, `overlap`, `window_index`, `paragraph_indices`. |
| `sentence` | N sentences per chunk, detected without NLP. Handles common abbreviations. Param: `sentences_per_chunk` (default 3). `metadata` includes `sentences_per_chunk`, `actual_sentence_count`, `chunk_index`, `source_paragraph_index`. |
| `page_aware` | Chunks by explicit page breaks (`w:pageBreak`), section breaks (`w:sectPr`), then paragraph count fallback. Param: `paragraphs_per_page` (default 15). `metadata` includes `page_number`, `page_break_type`, `paragraph_count`. |

> **Streaming**: all 7 DOCX modes are supported by `stream_chunks`. `default`/`structural` use `DocxStructuralIterator`; the other five modes (`section`, `semantic`, `sliding_window`, `sentence`, `page_aware`) each have a dedicated Rust iterator that parses the document once upfront and emits chunks one at a time. Output is byte-for-byte identical to `get_chunks` for every mode.

---

### PDF modes

All 7 modes are supported for both batch (`get_chunks`) and streaming (`stream_chunks`):

For PDF, `default` and `structural` are intentionally different modes (not aliases): `default` uses a fast lightweight path, while `structural` uses the full font-size-weighted pipeline, so outputs can differ on the same file.

```python
from py_chunks import get_chunks, stream_chunks

# Batch
chunks = get_chunks("file.pdf", mode="default")
chunks = get_chunks("file.pdf", mode="structural")
chunks = get_chunks("file.pdf", mode="section")
chunks = get_chunks("file.pdf", mode="semantic")
chunks = get_chunks("file.pdf", mode="sliding_window", window_size=3, overlap=1)
chunks = get_chunks("file.pdf", mode="sentence",       sentences_per_chunk=3)
chunks = get_chunks("file.pdf", mode="page_aware",     paragraphs_per_page=15)

# Streaming — same modes, same parameters
for chunk in stream_chunks("file.pdf", mode="section"):
    print(chunk["content"])
```

| Mode | Rust function | Description |
|---|---|---|
| `default` | `chunk_pdf_fast` | Fast page-by-page text extraction with block splitting. Minimal font analysis. |
| `structural` | `chunk_pdf` | Font-size-weighted span pipeline. Heading detection via font size relative to document average. |
| `section` | `chunk_pdf_section` | Groups content under each heading into one chunk (≤ 2 000 chars). `metadata` includes `section_heading`, `section_level`, `heading_path`, `heading_font_size`. |
| `semantic` | `chunk_pdf_semantic` | Heuristic merging by reference pronouns, transition words, and keyword overlap (≤ 1 500 chars). `metadata` includes `paragraph_count`, `merge_reason`. |
| `sentence` | `chunk_pdf_sentence` | N sentences per chunk. `metadata` includes `sentences_per_chunk`, `actual_sentence_count`, `chunk_index`, `source_paragraph_index`. |
| `sliding_window` | `chunk_pdf_sliding_window` | Overlapping paragraph windows. `metadata` includes `window_size`, `overlap`, `window_index`, `paragraph_range`. |
| `page_aware` | `chunk_pdf_page_aware` | Chunks by real page boundaries; falls back to paragraph count for dense pages. `metadata` includes `page_number`, `page_break_type`, `paragraph_count`. |

> **Note**: PDFs without a text layer (scanned / image-only) will raise `RuntimeError: PDF appears to contain no extractable text`. PDFium can only extract text that is embedded as actual text, not rendered as images.

---

### PPTX modes

PPTX supports all 7 modes via the unified `mode` parameter:

```python
from py_chunks import get_chunks, stream_chunks

chunks = get_chunks("deck.pptx", mode="default")        # one chunk per slide (with short-slide merging)
chunks = get_chunks("deck.pptx", mode="structural")     # alias for default
chunks = get_chunks("deck.pptx", mode="section")        # group by PPTX sections / title-divider heuristic
chunks = get_chunks("deck.pptx", mode="semantic")       # merge consecutive slides by topic continuity
chunks = get_chunks("deck.pptx", mode="sliding_window", window_size=3, overlap=1)
chunks = get_chunks("deck.pptx", mode="sentence",       sentences_per_chunk=3)
chunks = get_chunks("deck.pptx", mode="page_aware",     paragraphs_per_page=5)   # slides per chunk

for chunk in stream_chunks("deck.pptx", mode="section"):
    ...
```

> **Note**: For PPTX, `paragraphs_per_page` is interpreted as **slides per chunk** (default **5**, not 15).
>
> **Legacy API**: `chunk_pptx_with_strategy(path, strategy=...)` still works and is a thin wrapper around `chunk_pptx(path, mode=...)`. It is **not** re-exported at the top level — import it from the submodule:
>
> ```python
> from py_chunks.chunkers.pptx import chunk_pptx_with_strategy
> ```
>
> New code should use `chunk_pptx(..., mode=...)` instead.

---

### Markdown, HTML, TXT modes

All three formats accept the full set of 7 modes:

```python
from py_chunks import get_chunks

chunks = get_chunks("notes.md",   mode="default")          # one chunk per block element
chunks = get_chunks("notes.md",   mode="semantic")         # topic-continuity merging (10 signals)
chunks = get_chunks("notes.md",   mode="section")          # grouped under each heading
chunks = get_chunks("notes.md",   mode="sliding_window", window_size=4, overlap=1)
chunks = get_chunks("notes.md",   mode="sentence",       sentences_per_chunk=3)
chunks = get_chunks("notes.md",   mode="page_aware",     paragraphs_per_page=15)

chunks = get_chunks("page.html",  mode="semantic")          # same modes for HTML
chunks = get_chunks("readme.txt", mode="section")           # same modes for plain text
```

These three formats also support **streaming in every mode** — see the Streaming section below.

---

## Streaming

### When to use streaming

Use `stream_chunks` (or the `stream_chunks_from_*` variants) when:
- Processing large documents and you want to forward / persist / embed each chunk before the whole document is parsed
- Building pipelines where chunks flow into a queue, vector store, database, or HTTP response
- You want bounded memory regardless of document size (PDF and the MD/HTML/TXT state machines)

### Streaming support matrix

| Format | Modes streamable | Mechanism | Notes |
|---|---|---|---|
| **PDF** | All 7 | Background Rust thread + `mpsc` channel | Owns the `PdfDocument` on the worker thread, sends one `RawChunk` at a time. Output is byte-for-byte identical to `get_chunks`. |
| **Markdown** | All 7 | Block-by-block state machine (`structural`, `semantic`) + batch-drain (others) | `structural` / `semantic` use O(blocks) memory; the other four modes compute the chunk list once and drain it one chunk per `__next__`. |
| **HTML** | All 7 | Same as Markdown | Identical hybrid model: state machine for `structural` / `semantic`, batch-drain for `section` / `sliding_window` / `sentence` / `page_aware`. |
| **TXT** | All 7 | Same as Markdown | Pure Rust, no threads. |
| **DOCX** | All 7 | `DocxStructuralIterator` for `default`/`structural`; dedicated per-mode Rust iterators for the other 5 | Full document parsed once upfront; chunks emitted lazily. Peak memory ≈ file size + chunk vec. Output equals `get_chunks` for every mode. |
| **PPTX** | All 7 | Batch-drain | PPTX requires the full ZIP up front, so chunks are computed once at construction and yielded one per `__next__`. |

> **Parity guarantee**: streaming output equals `list(get_chunks(...))` for every format and every supported mode (this is exercised by `test_pdf_streaming.py` for PDF and by the tests in `py_chunks/tests/test_source_apis.py`).

### Streaming examples

```python
from py_chunks import stream_chunks

# PDF — all 7 modes
for chunk in stream_chunks("large.pdf", mode="section"):
    store_in_db(chunk)

for chunk in stream_chunks("report.pdf", mode="sliding_window", window_size=4, overlap=1):
    embed_and_index(chunk)

# Markdown / HTML / TXT — all 7 modes
for chunk in stream_chunks("book.md",   mode="semantic"):       ...
for chunk in stream_chunks("page.html", mode="section"):        ...
for chunk in stream_chunks("log.txt",   mode="sentence", sentences_per_chunk=2): ...

# DOCX — all 7 modes
for chunk in stream_chunks("document.docx", mode="structural"):   send_to_queue(chunk)
for chunk in stream_chunks("document.docx", mode="semantic"):     process(chunk)
for chunk in stream_chunks("document.docx", mode="section"):      index(chunk)
for chunk in stream_chunks("document.docx", mode="sentence", sentences_per_chunk=3):   embed(chunk)
for chunk in stream_chunks("document.docx", mode="sliding_window", window_size=3, overlap=1): embed(chunk)
for chunk in stream_chunks("document.docx", mode="page_aware",   paragraphs_per_page=15): store(chunk)

# PPTX — any mode
for chunk in stream_chunks("deck.pptx", mode="semantic"):
    ...

# From bytes (e.g. FastAPI body)
for chunk in stream_chunks(request_body, filename="report.pdf", mode="semantic"):
    process(chunk)

# As a context manager (temp file cleanup for bytes sources)
with stream_chunks(data, filename="big.pdf", mode="section") as it:
    for chunk in it:
        ...
```

---

## Supported Input Sources

The unified `get_chunks` / `stream_chunks` entry points accept any of these automatically:

| Source | Example |
|---|---|
| Local file path (str or Path) | `get_chunks("report.pdf")` |
| HTTP / S3 presigned URL | `get_chunks("https://bucket.s3.amazonaws.com/file.pdf?sig=...")` |
| Raw bytes | `get_chunks(data, filename="report.pdf")` |
| `bytearray` / `memoryview` | `get_chunks(bytearray_data, filename="doc.docx")` |
| File-like object (`BytesIO`, open file) | `get_chunks(BytesIO(data), filename="doc.md")` |
| FastAPI / Starlette `UploadFile` | `get_chunks(upload_file)` |

Or use the explicit source-specific helpers:

| Function | Source |
|---|---|
| `get_chunks_from_path(file_path)` | Local path |
| `get_chunks_from_bytes(data, filename)` | Raw bytes |
| `get_chunks_from_fileobj(file_obj, filename=None)` | File-like object |
| `get_chunks_from_upload(upload_file)` | FastAPI UploadFile |
| `get_chunks_from_s3_presigned_url(url, filename=None, timeout=60)` | Presigned URL |
| `stream_chunks_from_path(file_path, ...)` | Local path (streaming) |
| `stream_chunks_from_bytes(data, filename, ...)` | Raw bytes (streaming) |
| `stream_chunks_from_fileobj(file_obj, ...)` | File-like object (streaming) |
| `stream_chunks_from_upload(upload_file, ...)` | FastAPI UploadFile (streaming) |
| `stream_chunks_from_s3_presigned_url(url, ...)` | Presigned URL (streaming) |

---

## Supported Formats

| Format | Extensions | Batch modes | Streaming modes |
|---|---|---|---|
| PDF        | `.pdf`            | All 7 | All 7 (background thread) |
| DOCX       | `.docx`           | All 7 | All 7 (dedicated iterator per mode) |
| PPTX       | `.pptx`           | All 7 | All 7 (batch-drain) |
| Markdown   | `.md`             | All 7 | All 7 (state machine for `structural` / `semantic`) |
| HTML       | `.html`, `.htm`   | All 7 | All 7 (state machine for `structural` / `semantic`) |
| Plain Text | `.txt`            | All 7 | All 7 (state machine for `structural` / `semantic`) |

The 7 modes are: `default`, `structural`, `section`, `semantic`, `sliding_window`, `sentence`, `page_aware`.

---

## API Reference

### Unified entry points

```python
get_chunks(
    source,
    *,
    filename: str | None = None,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
) -> list[dict]
```

```python
stream_chunks(
    source,
    *,
    filename: str | None = None,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
) -> Iterator[dict]
```

**Parameters**

| Parameter | Type | Default | Description |
|---|---|---|---|
| `source` | str, Path, bytes, file-like, upload, URL | — | Document source. Auto-detected. |
| `filename` | str \| None | None | Required when source is `bytes` or a file object without a `.name` attribute. |
| `mode` | str | `"default"` | Chunking mode. Applies to **every** supported format (PDF, DOCX, PPTX, MD, HTML, TXT). One of `default`, `structural`, `section`, `semantic`, `sliding_window`, `sentence`, `page_aware`. |
| `window_size` | int | 3 | Number of blocks per window (`sliding_window` mode). Must be `> 0`. |
| `overlap` | int | 1 | Overlapping blocks between windows (`sliding_window` mode). Must be `< window_size`. |
| `sentences_per_chunk` | int | 3 | Sentences per chunk (`sentence` mode). Must be `> 0`. |
| `paragraphs_per_page` | int | 15 | Block / paragraph quota before a page flush (`page_aware` mode). Must be `> 0`. For **PPTX** this means *slides per chunk* and the format-level default is `5`. |

**Returns** — `list[dict]` (batch) or `Iterator[dict]` (streaming). Each chunk dict:

```python
{
    "content":      str,   # extracted text
    "content_type": str,   # see content types below
    "metadata":     dict   # format- and mode-specific fields
}
```

**Raises**

| Exception | Condition |
|---|---|
| `FileNotFoundError` | Path does not exist |
| `ValueError` | Unsupported extension, invalid mode, or bad parameter |
| `TypeError` | Unsupported source type or async `.read()` on upload |
| `RuntimeError` | Rust-level failure (e.g. no extractable text in PDF) |
| `NotImplementedError` | Streaming requested for an unsupported format/mode |

---

### Format-specific chunkers (advanced)

Each format also has a direct module that returns `(chunks, timing)`, where `timing` is `{"rust_ms": float, "python_ms": float}`. Use these when you want per-call timing data or when you only need one format and want to skip source-type detection.

```python
from py_chunks.chunkers.pdf  import chunk_pdf,  stream_chunk_pdf
from py_chunks.chunkers.docx import chunk_docx, stream_chunk_docx
from py_chunks.chunkers.pptx import chunk_pptx, stream_chunk_pptx, chunk_pptx_with_strategy
from py_chunks.chunkers.html import chunk_html, stream_chunk_html
from py_chunks.chunkers.md   import chunk_md,   stream_chunk_md
from py_chunks.chunkers.txt  import chunk_txt,  stream_chunk_txt

# Batch with timing
chunks, timing = chunk_pdf("file.pdf", mode="section")
print(f"Rust: {timing['rust_ms']} ms  Python: {timing['python_ms']} ms")

chunks, timing = chunk_md("notes.md", mode="semantic")
chunks, timing = chunk_html("page.html", mode="sliding_window", window_size=4, overlap=1)
chunks, timing = chunk_txt("log.txt", mode="sentence", sentences_per_chunk=2)
chunks, timing = chunk_pptx("deck.pptx", mode="page_aware", paragraphs_per_page=5)

# Legacy PPTX strategy wrapper (kept for backward compatibility)
chunks, timing = chunk_pptx_with_strategy("deck.pptx", strategy="structural")

# Streaming — all formats
for chunk in stream_chunk_pdf("report.pdf", mode="semantic"):          ...
for chunk in stream_chunk_docx("doc.docx", mode="structural"):         ...  # all 7 modes supported
for chunk in stream_chunk_docx("doc.docx", mode="semantic"):           ...
for chunk in stream_chunk_docx("doc.docx", mode="section"):            ...
for chunk in stream_chunk_docx("doc.docx", mode="sentence", sentences_per_chunk=3): ...
for chunk in stream_chunk_md("book.md", mode="sentence", sentences_per_chunk=2): ...
for chunk in stream_chunk_html("page.html", mode="section"):           ...
for chunk in stream_chunk_txt("log.txt", mode="page_aware", paragraphs_per_page=20): ...
for chunk in stream_chunk_pptx("deck.pptx", mode="semantic"):          ...
```

---

## Output Schema

### Chunk structure

```python
{
    "content":      "The extracted text segment.",
    "content_type": "plain_paragraph",
    "metadata": { ... }   # keys depend on format and mode — see below
}
```

### content\_type values

| Value | Description |
|---|---|
| `heading` | Section heading (H1–H6, bold text, ALLCAPS line, etc.) |
| `plain_paragraph` | Regular prose paragraph |
| `bullet_list` | Unordered or numbered list |
| `table` | Tabular data |
| `code_block` | Code or preformatted text |
| `long_single_paragraph` | Paragraph > 500 characters |
| `short_disconnected_paragraph` | Paragraph < 80 characters |
| `mixed_content` | DOCX structural block that merges a heading with its immediately following body element (e.g. a heading run that shares a `<w:p>` with body text) |
| `section` | Heading-scoped grouped content (`section` mode) |
| `semantic` | Heuristic topic-continuity group (`semantic` mode) |
| `sliding_window` | Fixed-size overlapping window (`sliding_window` mode) |
| `sentence` | Sentence-count group (`sentence` mode) |
| `page_aware` | Page boundary group (`page_aware` mode) |

### Metadata fields by mode

Metadata is a `dict` whose keys depend on both the format and the mode. The most useful keys are listed below; treat any field as optional and use `chunk["metadata"].get(key)`.

| Mode | Format(s) | Notable metadata keys |
|---|---|---|
| `default` / `structural` | PDF | `page_number`, `is_heading`, `avg_font_size` |
| `default` / `structural` | DOCX | `section_heading`, `section_heading_level`, `footnotes` (list of `{id, text}`), `endnotes` (list of `{id, text}`), `page_number`, `document_metadata` (`header_text`, `footer_text`, `image_count`). Inline images are emitted as `[Image: <alt>]` (or `[Image]` when no alt text). Footnote / endnote ids reference `word/footnotes.xml` / `word/endnotes.xml` and are anchored to the chunk that contains the referring paragraph. |
| `default` / `structural` | MD / HTML / TXT | `section_heading`, `document_metadata.source_type` |
| `default` / `structural` | PPTX | `slide_number`, `section_heading` (when detectable) |
| `section` | PDF | `page_number`, `section_heading`, `section_level`, `heading_path`, `paragraph_count`, `heading_font_size` |
| `section` | DOCX | `section_heading`, `section_heading_level`, `section_level`, `heading_path`, `document_metadata` |
| `section` | MD / HTML / TXT / PPTX | `section_heading`, `section_level`, `heading_path`, `paragraph_count` |
| `semantic` | PDF | `page_number`, `paragraph_count`, `merge_reason` |
| `semantic` | DOCX | `section_heading`, `section_heading_level`, `paragraph_count`, `merge_reason`, `document_metadata` |
| `semantic` | MD / HTML / TXT / PPTX | `paragraph_count`, `merge_reasons` (list), `primary_merge_reason`, `keyword_density`, `avg_block_length` (MD/TXT), `section_heading`, `heading_path`, `chunk_index`, `document_metadata` |
| `sentence` | PDF | `sentences_per_chunk`, `actual_sentence_count`, `chunk_index`, `source_paragraph_index` |
| `sentence` | DOCX | `sentences_per_chunk`, `actual_sentence_count`, `chunk_index`, `source_paragraph_index`, `source_paragraph_is_heading`, `source_paragraph_heading_level`, `source_paragraph_is_list`, `source_paragraph_is_table`, `document_metadata` |
| `sentence` | MD / HTML / TXT / PPTX | `sentences_per_chunk`, `actual_sentence_count`, `chunk_index`, `source_paragraph_index` |
| `sliding_window` | PDF | `window_size`, `overlap`, `window_index`, `paragraph_count`, `paragraph_range`, `page_number` |
| `sliding_window` | DOCX | `window_size`, `overlap`, `window_index`, `paragraph_indices`, `list_item_count`, `heading_count`, `paragraph_meta`, `document_metadata` |
| `sliding_window` | MD / HTML / TXT / PPTX | `window_size`, `overlap`, `window_index`, `paragraph_count`, `paragraph_range` |
| `page_aware` | PDF | `page_number`, `page_break_type`, `paragraph_count`, `document_metadata` |
| `page_aware` | DOCX | `page_number`, `page_break_type`, `paragraph_count`, `section_heading_level`, `headings`, `list_item_count`, `table_count`, `document_metadata` |
| `page_aware` | MD / HTML / TXT | `page_number`, `page_break_type` (heading-boundary or paragraph-count), `paragraph_count` |
| `page_aware` | PPTX | `slide_numbers`, `paragraph_count` |

The DOCX `semantic` `merge_reason` is one of: `heading_merge`, `keyword_overlap`, `reference_continuity`, `short_paragraph`, `transition_break`.

The MD / HTML / TXT / PPTX `semantic` `primary_merge_reason` is one of: `reference_continuity`, `elaboration`, `example`, `cause_effect`, `contrast_continuation`, `question_answer`, `definition_expansion`, `short_paragraph`, `keyword_overlap`, or `initial` (singleton chunks).

---

## Usage Examples

### Local file

```python
from py_chunks import get_chunks

chunks = get_chunks("report.pdf")
for chunk in chunks:
    print(chunk["content"][:120])
```

### Streaming a large PDF section-by-section

```python
from py_chunks import stream_chunks

for chunk in stream_chunks("large.pdf", mode="section"):
    heading = chunk["metadata"].get("section_heading", "")
    print(f"[{heading}] {chunk['content'][:80]}")
```

### From bytes (API upload)

```python
from py_chunks import get_chunks_from_bytes

file_bytes = request.files['document'].read()
chunks = get_chunks_from_bytes(file_bytes, filename="report.pdf")
```

### From file-like object

```python
from py_chunks import get_chunks_from_fileobj
from io import BytesIO

bio = BytesIO(file_data)
chunks = get_chunks_from_fileobj(bio, filename="document.md")
```

### From S3 presigned URL

```python
from py_chunks import get_chunks_from_s3_presigned_url

url = "https://bucket.s3.amazonaws.com/file.docx?AWSAccessKeyId=..."
chunks = get_chunks_from_s3_presigned_url(url)
```

---

## Framework Integration

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

### FastAPI — streaming response

```python
from fastapi import FastAPI, File, UploadFile
from fastapi.responses import StreamingResponse
from py_chunks import stream_chunks_from_upload
import json

app = FastAPI()

@app.post("/chunk/stream/")
async def chunk_stream(file: UploadFile = File(...)):
    def generate():
        for chunk in stream_chunks_from_upload(file):
            yield json.dumps(chunk) + "\n"
    return StreamingResponse(generate(), media_type="application/x-ndjson")
```

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

### Django

```python
from django.http import JsonResponse
from py_chunks import get_chunks_from_upload

def chunk_view(request):
    if request.FILES:
        chunks = get_chunks_from_upload(request.FILES['document'])
        return JsonResponse({"chunks": chunks})
    return JsonResponse({"error": "No file"}, status=400)
```

### Celery background job

```python
import celery
from py_chunks import get_chunks

@celery.task
def process_document(file_path: str):
    chunks = get_chunks(file_path)
    # persist to database
    return len(chunks)
```

---

## Architecture

```
┌──────────────────────────────────────────────┐
│           Python Public API                  │
│         (py_chunks/__init__.py)              │
│   get_chunks()  /  stream_chunks()           │
│   *_from_path / _from_bytes / _from_fileobj  │
│   *_from_upload / _from_s3_presigned_url     │
└──────────────┬───────────────────────────────┘
               │  source detection + temp-file management + cleanup
               ↓
┌──────────────────────────────────────────────┐
│            Format Dispatcher                 │
│        (py_chunks/chunkers/*.py)             │
│   chunk_pdf / chunk_docx / chunk_pptx /      │
│   chunk_md  / chunk_html / chunk_txt   +     │
│   matching stream_chunk_* variants           │
└──────────────┬───────────────────────────────┘
               │  validates args, dispatches to the right Rust function,
               │  measures Python-side timing
               ↓
┌──────────────────────────────────────────────────────────────────┐
│                  Rust Extension  (_rust.so)                      │
│                  (src/extensions/<format>/*.rs)                  │
│                                                                  │
│  Each format submodule contains:                                 │
│    structural.rs   — default / structural chunker                │
│    section.rs      — section-grouped chunker                     │
│    semantic.rs     — 10-signal topic-continuity chunker          │
│    sliding_window.rs                                             │
│    sentence.rs                                                   │
│    page_aware.rs                                                 │
│    stream_iter.rs  — streaming iterator(s)                       │
│                                                                  │
│  PDF stream    — background thread owns PdfDocument; sends       │
│                  RawChunk through mpsc channel; __next__ recvs   │
│  MD/HTML/TXT   — block-by-block state machine for structural /   │
│                  semantic; batch-drain for the other 4 modes     │
│  DOCX stream   — DocxStructuralIterator (default/structural) +   │
│                  per-mode iterators for all other 5 modes        │
│  PPTX stream   — batch-drain (ZIP must be read upfront)          │
└──────────────────────────────────────────────────────────────────┘
```

### Design principles

- **Single responsibility** — each format has its own Rust submodule; modes never leak between formats
- **Framework-agnostic Python layer** — source detection (path / URL / bytes / file-like / upload) lives in `py_chunks/__init__.py`; the Rust layer only sees a file path
- **Temp-file strategy for bytes** — bytes / file-like / URL inputs are written to a `NamedTemporaryFile` (with the original extension), passed to Rust, then deleted; streaming variants wrap the iterator in `_StreamingFileCleanup` so the temp file is removed even on early exit
- **PDF streaming safety** — the background worker owns the `PdfDocument` for its full lifetime; chunks cross the thread boundary as plain `RawChunk` structs through `mpsc`, so no `unsafe` is needed
- **Streaming parity** — every streaming iterator yields the same chunks (and metadata) as the corresponding batch function

---

## Error Handling

```python
from py_chunks import get_chunks

# File not found
try:
    chunks = get_chunks("missing.pdf")
except FileNotFoundError as e:
    print(e)   # File not found: missing.pdf

# Unsupported format
try:
    chunks = get_chunks("image.png")
except ValueError as e:
    print(e)   # Unsupported file type '.png'. Supported: .docx, .htm, .html, .md, .pdf, .pptx, .txt

# Scanned / image-only PDF (no text layer)
try:
    chunks = get_chunks("scanned.pdf")
except RuntimeError as e:
    print(e)   # PDF appears to contain no extractable text

# Bytes source requires a filename so the extension can be detected
try:
    chunks = get_chunks(b"hello")
except ValueError as e:
    print(e)   # filename is required when source is bytes

# Invalid sliding_window parameters
try:
    chunks = get_chunks("notes.md", mode="sliding_window", window_size=2, overlap=2)
except ValueError as e:
    print(e)   # overlap must be less than window_size
```

### Exceptions raised

| Exception | When |
|---|---|
| `FileNotFoundError` | A path was given but does not exist on disk. |
| `ValueError` | Unsupported extension, unknown mode, empty bytes, invalid `window_size` / `overlap` / `sentences_per_chunk` / `paragraphs_per_page`, missing `filename` for bytes / fileobj / URL inputs. |
| `TypeError` | Unsupported source type, or `upload_file.read()` returned a coroutine (async). Pass `upload_file.file` instead, or `await` it yourself. |
| `RuntimeError` | Rust-level failure (e.g. PDF with no extractable text, malformed DOCX/PPTX ZIP, unreadable file). |
| `NotImplementedError` | A streaming mode/format combination that is not supported. |

---

## Development & Testing

### Build from source

```bash
cd py_chunks
pip install maturin
maturin develop
```

### Running tests

```bash
cd py_chunks
python -m pytest -v
```

### Full PDF strategy test (batch + streaming parity across all modes)

```bash
python test_pdf_streaming.py
```

Tests all 7 strategies × batch + streaming on every PDF in `test_files/`. Validates chunk count parity between batch and streaming paths.

### Code quality

```bash
python -m pylint py_chunks tests/test_source_apis.py
```

Expected: 10.00/10

---

## License

MIT

---

Built with Rust (performance) + Python (simplicity)
