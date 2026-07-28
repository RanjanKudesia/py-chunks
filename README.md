# py-chunks

[![Python](https://img.shields.io/badge/python-3.9+-blue)](https://www.python.org/downloads/) [![License](https://img.shields.io/badge/license-MIT-green)](LICENSE)

Fast, framework-agnostic document chunking library backed by Rust. Extract meaningful content segments from DOCX, DOC, PDF, PPTX, PPT, TXT, Markdown, HTML, CSV, TSV, XLSX, XLS, XLSM, XLSB, ODS, XLTX, and XLTM files — optimised for production use.

## Contents

- [Features](#features)
- [Installation](#installation)
- [Quick Start](#quick-start)
- [Chunking Modes](#chunking-modes)
  - [Pick your mode](#pick-your-mode)
  - [DOCX](#docx-modes)
  - [DOC (Word 97–2003)](#doc-modes-word-972003)
  - [PDF](#pdf-modes)
  - [PPTX](#pptx-modes)
  - [PPT (PowerPoint 97–2003)](#ppt-modes-powerpoint-972003)
  - [Markdown / HTML / TXT](#markdown-html-txt-modes)
  - [XLSX / XLS](#xlsx--xls-modes)
  - [CSV](#csv-modes)
- [Markdown Conversion](#markdown-conversion)
- [Streaming](#streaming)
- [Supported Input Sources](#supported-input-sources)
- [Supported Formats](#supported-formats)
- [API Reference](#api-reference)
- [Output Schema](#output-schema)
- [Usage Examples](#usage-examples)
- [Framework Integration](#framework-integration)
- [Architecture](#architecture)
- [Error Handling](#error-handling)
- [Development & Testing](#development--testing)

---

## Features

- **36 Document Formats**: PDF, DOCX, DOCM, DOTX, DOTM, DOC (Word 97–2003), PPTX, POTX, POTM, PPSX, PPSM, PPT (PowerPoint 97–2003), ODT/ODP (OpenDocument), Markdown, HTML, TXT, CSV, TSV, XLSX, XLS, XLSM, XLSB, ODS, XLTX, XLTM, JSON/JSONL/NDJSON, MSG (Outlook), EML/MBOX (MIME email), RTF, EPUB, IPYNB (Jupyter)
- **7 Chunking Modes for document formats** (incl. all Word `.docm`/`.dotx`/`.dotm` and PowerPoint `.potx`/`.potm`/`.ppsx`/`.ppsm` OOXML variants): `default`, `structural`, `section`, `semantic`, `sliding_window`, `sentence`, `page_aware`
- **6 Chunking Modes for spreadsheet formats** (XLSX / XLS / XLSM / XLSB / ODS / XLTX / XLTM): `row`, `table`, `sheet`, `sliding_window`, `page_aware`, `semantic`
- **4 Chunking Modes for delimited text** (CSV / TSV): `row`, `default`, `sliding_window`, `page_aware`
- **Streaming for every format** via a single `stream_chunks()` entry point
  - PDF: background Rust thread + `mpsc` channel (all 7 modes, true one-chunk-at-a-time)
  - Markdown / HTML / TXT: block-by-block state machine for `structural` + `semantic`; batch-drain for the rest
  - DOCX: all 7 modes — `DocxStructuralIterator` for `default`/`structural`; dedicated per-mode iterators for the remaining 5 modes (lazy chunk emission after a single upfront parse)
  - DOC: all 7 modes — `DocStructuralIterator` for `default`/`structural`; dedicated per-mode iterators for the remaining 5 modes
  - PPT (PowerPoint 97–2003): all 7 modes — reuses the DOC iterators after extracting slide text from the binary record stream
  - PPTX: batch-drain (ZIP must be read upfront, then chunks are yielded one at a time)
  - XLSX / XLS: `row` and `sliding_window` use true state machines (one chunk per `__next__`, O(parsed_rows) memory); `table`, `sheet`, `page_aware`, and `semantic` use batch-drain (global sheet analysis required before first chunk)
  - CSV: true line-by-line worker for `row` / `default`, `sliding_window`, and `page_aware`; delimiter auto-detection and encoding-aware decoding are supported
- **Markdown conversion** via `get_markdown()` — converts any supported document to a Markdown string (26 extensions: `.doc`, `.docx`, `.docm`, `.dotx`, `.dotm`, `.pptx`, `.potx`, `.potm`, `.ppsx`, `.ppsm`, `.ppt`, `.pdf`, `.html`, `.htm`, `.xlsx`, `.xls`, `.xlsm`, `.xlsb`, `.ods`, `.xltx`, `.xltm`, `.csv`, `.tsv`, `.txt`, `.md`)
- **Image extraction** for DOCX/DOCM/DOTX/DOTM, PPTX/POTX/POTM/PPSX/PPSM, XLSX/XLSM/XLSB/ODS/XLTX/XLTM, HTML, PDF, DOC, and PPT in two modes:
  - `get_chunks(..., list_images=True)` — returns a `ChunksResult` with the normal chunk list plus a `dict[str, bytes]` of every embedded image; each image also appears as a dedicated `content_type="image"` chunk whose `content` is the image's hash filename. All 7 chunking modes supported for document formats; all 6 for spreadsheet formats. Spreadsheet images carry per-sheet `sheet_name`/`sheet_index` and `alt_text` metadata; `.xlsb` resolves them via a `sheetN.bin.rels` fallback and `.ods` via a dedicated ODF `Pictures/` walker. `.xls` has no extractable images.
  - `get_markdown(..., list_images=True)` — returns a `MarkdownResult` with the rendered Markdown plus the same image dict. Images keyed by a stable content hash; web-renderable formats only: `.png`, `.jpg`, `.jpeg`, `.gif`, `.webp`.
  - **PDF images are page-scoped**: each extracted raster is re-encoded to `.png` and tagged with the `page_number` it appears on (rather than `alt_text`), and image chunks are grouped ahead of the text chunks. Scanned / image-only PDFs (no text layer) still return their page images instead of raising.
  - **Legacy binary formats (`.doc`, `.ppt`)**: JPEG and PNG images are decoded from the OfficeArt (MS-ODRAW) BLIP records embedded in the binary streams. `.ppt` images carry the `page_number` (slide) they appear on when it can be determined safely; `.doc` inline pictures carry a `paragraph_index` anchor. Vector/legacy raster payloads (EMF, WMF, PICT, DIB, TIFF) are silently skipped.
- **Multiple Input Sources**: local file paths, raw `bytes` / `bytearray` / `memoryview`, file-like objects (`BytesIO`, open files), FastAPI / Starlette `UploadFile`, HTTP(S) / S3 pre-signed URLs
- **Consistent Output Schema**: every chunk is a `dict` with `content`, `content_type`, and `metadata` keys
- **Minimal Python dependencies**: all parsing happens in the Rust extension; PDF support uses [`pypdfium2`](https://pypi.org/project/pypdfium2/) (installed automatically), which bundles the PDFium native binary for every platform

## Installation

```bash
pip install py-chunks
```

**Requirements**: Python 3.9+

### PDF native library

PDF chunking uses the PDFium native library. It is provided by [`pypdfium2`](https://pypi.org/project/pypdfium2/), which is installed automatically as a dependency and bundles the correct PDFium binary for your platform (macOS, Linux, Windows) — no separate installation needed.

To use a custom PDFium binary instead, set the environment variable before importing:

```bash
export PDFIUM_LIBRARY_PATH=/path/to/libpdfium.dylib   # macOS
export PDFIUM_LIBRARY_PATH=/path/to/libpdfium.so       # Linux
set PDFIUM_LIBRARY_PATH=C:\path\to\pdfium.dll          # Windows
```

---

## Quick Start

```python
from py_chunks import get_chunks, stream_chunks, get_markdown

# Batch — works for every supported format
chunks = get_chunks("document.pdf")
chunks = get_chunks("notes.md",     mode="semantic")
chunks = get_chunks("page.html",    mode="section")
chunks = get_chunks("deck.pptx",    mode="sliding_window", window_size=3, overlap=1)
chunks = get_chunks("report.docx",  mode="sentence",       sentences_per_chunk=3)
chunks = get_chunks("legacy.doc",   mode="default")        # DOC (Word 97-2003)
chunks = get_chunks("legacy.ppt",   mode="default")        # PPT (PowerPoint 97-2003)
chunks = get_chunks("data.xlsx",    mode="row")                                    # XLSX (use chunk_xlsx() for rows_per_chunk control)
chunks = get_chunks("legacy.xls",   mode="row")                                    # XLS
chunks = get_chunks("data.csv",     mode="row")                                    # CSV (use chunk_csv() for rows_per_chunk control)
chunks = get_chunks("data.csv",     mode="sliding_window", window_size=5, overlap=1)

for chunk in chunks:
    print(chunk["content"])
    print(chunk["content_type"])   # e.g. "heading", "plain_paragraph", "semantic"
    print(chunk["metadata"])       # format- and mode-specific

# Streaming — works for every supported format
for chunk in stream_chunks("large.pdf", mode="section"):
    handle(chunk)

# Markdown conversion — get a Markdown string from any supported document
md = get_markdown("report.docx")
md = get_markdown("legacy.doc")
md = get_markdown("data.csv")
md = get_markdown("notes.txt")
md = get_markdown(file_bytes, filename="report.pdf")  # bytes also supported

# Image extraction via get_chunks — DOCX, PPTX, XLSX, HTML, PDF, DOC, and PPT
from py_chunks import ChunksResult
result = get_chunks("report.docx", list_images=True)   # returns ChunksResult
result = get_chunks("report.docx", mode="semantic", list_images=True)
result = get_chunks("deck.pptx",   list_images=True)
result = get_chunks("deck.pptx",   mode="section", list_images=True)
result = get_chunks("data.xlsx",   list_images=True)   # all 6 XLSX modes supported
result = get_chunks("page.html",   list_images=True)   # base64 data URIs + local file refs
result = get_chunks("page.html",   mode="semantic", list_images=True)
result = get_chunks("report.pdf",  list_images=True)   # page-scoped images, re-encoded to .png
result = get_chunks("report.pdf",  mode="page_aware", list_images=True)
result = get_chunks("legacy.doc",  list_images=True)   # inline PICF + floating BLIP images
result = get_chunks("deck.ppt",    list_images=True)   # Pictures stream, slide-attributed
isinstance(result, ChunksResult)   # True
result.chunks   # list of dicts — text chunks plus image chunks (content_type="image")
result.images   # {"8c4a2b4ccec6f521.jpeg": b"...", ...}

# Image extraction via get_markdown — DOCX, PPTX, XLSX, HTML, PDF, DOC, and PPT
from py_chunks import MarkdownResult
result = get_markdown("report.docx", list_images=True)  # returns MarkdownResult
print(result.markdown)       # full Markdown string with ![](hash.ext) refs
print(result.images)         # {"8c4a2b4ccec6f521.png": b"...", ...}
result = get_markdown("deck.pptx", list_images=True)
result = get_markdown("data.xlsx", list_images=True)    # refs added after each sheet table
result = get_markdown("page.html", list_images=True)    # refs appended at end of markdown
result = get_markdown("report.pdf", list_images=True)   # refs added at the top of each page
result = get_markdown("legacy.doc", list_images=True)   # refs placed at the anchoring paragraph
result = get_markdown("deck.ppt",  list_images=True)    # refs placed inside the owning slide
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

### Pick your mode

Not sure which mode to choose? Use this as a starting point:

| I want to… | Best mode |
|---|---|
| Index every paragraph and heading individually | `default` / `structural` |
| Keep all content under a heading in one chunk (e.g. for section-level search) | `section` |
| Feed semantically coherent passages to an LLM or embedding model | `semantic` |
| Enforce a fixed sentence count per chunk (tight token budget) | `sentence` |
| Build overlapping chunks for dense retrieval or sliding-context inference | `sliding_window` |
| Preserve the document's page layout (e.g. for page-referenced citations) | `page_aware` |
| Chunk a spreadsheet N rows at a time | `row` (XLSX / XLS / CSV) |
| Chunk by detected table region or named table | `table` (XLSX / XLS only) |
| One chunk per sheet | `sheet` (XLSX / XLS only) |

> **Default recommendation**: start with `semantic` when feeding an LLM and `section` when building a document search index. Use `default`/`structural` when you need fine-grained, element-level control.

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

> **Image-aware chunking**: all 7 DOCX modes support `list_images=True`. Pass it to `get_chunks` to receive a `ChunksResult` instead of a plain list. Image chunks (one per embedded image, in document order) have `content_type="image"`, `content` equal to the image's hash filename (e.g. `"8c4a2b4ccec6f521.png"`), and metadata keys `image_name`, `alt_text`, `page_number`, and `document_metadata`. Text chunks are byte-for-byte identical to the `list_images=False` output — only extra image chunks are added.

> **Streaming**: all 7 DOCX modes are supported by `stream_chunks`. `default`/`structural` use `DocxStructuralIterator`; the other five modes (`section`, `semantic`, `sliding_window`, `sentence`, `page_aware`) each have a dedicated Rust iterator that parses the document once upfront and emits chunks one at a time. Output is byte-for-byte identical to `get_chunks` for every mode.

---

### DOC modes (Word 97–2003)

`.doc` files use the same 7-mode API as DOCX. The parser is a pure Rust implementation using the Compound Binary File (`cfb`) crate — no LibreOffice, no external processes.

```python
from py_chunks import get_chunks, stream_chunks

chunks = get_chunks("file.doc", mode="default")        # structural (default)
chunks = get_chunks("file.doc", mode="structural")     # same as default
chunks = get_chunks("file.doc", mode="section")
chunks = get_chunks("file.doc", mode="semantic")
chunks = get_chunks("file.doc", mode="sliding_window", window_size=3, overlap=1)
chunks = get_chunks("file.doc", mode="sentence",       sentences_per_chunk=3)
chunks = get_chunks("file.doc", mode="page_aware",     paragraphs_per_page=15)

# Streaming — all 7 modes supported
for chunk in stream_chunks("file.doc", mode="semantic"):
    process(chunk)
```

| Mode | Description |
|---|---|
| `default` / `structural` | One chunk per paragraph element: heading, normal paragraph, table row, list item. Short paragraphs (< 80 chars) are merged into a single `short_disconnected_paragraph` chunk. |
| `section` | All paragraphs under a heading grouped into one chunk (≤ 2 000 chars, split if longer). |
| `semantic` | Paragraphs merged by keyword overlap and reference continuity (≤ 1 200 chars). |
| `sliding_window` | Overlapping windows of N paragraphs. Params: `window_size` (default 3), `overlap` (default 1). |
| `sentence` | N sentences per chunk, split without NLP. Param: `sentences_per_chunk` (default 3). |
| `page_aware` | Chunks by explicit page-break markers in the binary stream, with paragraph-count fallback. Param: `paragraphs_per_page` (default 15). |

**Format notes**:
- Only Word 97–2003 (`.doc`) binary format is supported. Pre-Word 97 files raise `RuntimeError: Pre-Word 97 .doc files are not supported. Convert to .docx first.`
- Text is reconstructed from the piece table (CLX), supporting both compressed CP1252 and Unicode UTF-16LE pieces.
- Heading levels are inferred from the stylesheet (`Stshf`) style index. All-caps or title-case short lines are promoted to `Heading(2)` as a fallback.
- Table cells (separated by `\x07` in the binary stream) are joined with ` | ` in chunk content.
- Page breaks (`\x0C`) flush the current page group; they are not emitted as content.

> **Markdown conversion**: `get_markdown("file.doc")` converts the `.doc` to Markdown — headings become `#`/`##`/`###`, list items become `- item`, table rows become `| cell | cell |`, and page breaks become `---`.

> **Image extraction**: all 7 DOC modes support `list_images=True`. Two storage mechanisms are covered: **inline pictures** (PICF structures in the `Data` stream, located via `sprmCPicLocation` in the character-run properties) and **floating shapes** (the OfficeArt blip store referenced by the FIB's `fcDggInfo`). Image chunks are grouped ahead of the text chunks with `content_type="image"`, `content` equal to the image's hash filename, and metadata keys `image_name` and `paragraph_index` (the raw paragraph the inline picture is anchored to, or `null` for floating/unanchored images). Only JPEG and PNG payloads are extracted; EMF/WMF/PICT/DIB/TIFF BLIPs are silently skipped. Text chunks are byte-for-byte identical to the `list_images=False` output. `get_markdown("file.doc", list_images=True)` inserts `![](hash.ext)` refs at the anchoring paragraph (unanchored images go at the end).

> **Streaming**: all 7 modes are supported. `default`/`structural` use `DocStructuralIterator`; the other five modes each have a dedicated Rust iterator.

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

> **Note**: PDFs without a text layer (scanned / image-only) will raise `RuntimeError: PDF appears to contain no extractable text` for the text-only chunkers. PDFium can only extract text that is embedded as actual text, not rendered as images. The image-aware variants (`list_images=True`) are the exception — they still return the page images for such PDFs instead of raising (see below).

**PDF image extraction**

`get_chunks(..., list_images=True)` and `get_markdown(..., list_images=True)` work for PDFs across all 7 modes. PDF images are **page-scoped** (PDFs have no inline alt text or document-flow position), so:

- Every embedded raster is decoded and **re-encoded to `.png`**, keyed by a stable content hash and deduplicated across the document.
- Each image becomes a `content_type="image"` chunk carrying `page_number` and `image_name` in its `metadata` (no `alt_text`). Image chunks are **grouped ahead of** the text chunks rather than interleaved.
- In Markdown, `![name](name)` references are inserted at the **top of each page's** content.
- Scanned / image-only PDFs still return their page images (one PNG per page) instead of raising.

```python
from py_chunks import get_chunks, get_markdown

result = get_chunks("report.pdf", mode="page_aware", list_images=True)
images = [c for c in result.chunks if c["content_type"] == "image"]
images[0]["metadata"]   # {"page_number": 1, "image_name": "bb5f9202….png", "document_metadata": {...}}
result.images           # {"bb5f9202….png": b"\x89PNG…", ...}

md = get_markdown("report.pdf", list_images=True)
md.markdown             # contains ![bb5f9202….png](bb5f9202….png) at the top of each page
md.images               # {"bb5f9202….png": b"\x89PNG…", ...}
```

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

> **Image-aware chunking**: all 7 PPTX modes support `list_images=True`. Pass it to `get_chunks` to receive a `ChunksResult`. Image chunks have `content_type="image"`, `content` equal to the image's hash filename, and metadata keys `slide_number`, `image_name`, `alt_text`, and `document_metadata`. Text chunks are byte-for-byte identical to the `list_images=False` output — only extra image chunks are added. Supported image formats: `.png`, `.jpg`, `.jpeg`, `.gif`, `.webp`; vector formats (`.emf`, `.wmf`) are silently skipped.

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

### PPT modes (PowerPoint 97–2003)

`.ppt` files use the same 7-mode API as DOCX/DOC and produce the same output schema as `.doc`. The parser is a pure Rust implementation: it opens the Compound Binary File (`cfb`) container, walks the "PowerPoint Document" record stream, and extracts slide text — no LibreOffice, no external processes.

```python
from py_chunks import get_chunks, stream_chunks

chunks = get_chunks("deck.ppt", mode="default")        # structural (default)
chunks = get_chunks("deck.ppt", mode="structural")     # same as default
chunks = get_chunks("deck.ppt", mode="section")        # one chunk per slide
chunks = get_chunks("deck.ppt", mode="semantic")       # one chunk per slide (continuity merge)
chunks = get_chunks("deck.ppt", mode="sliding_window", window_size=3, overlap=1)
chunks = get_chunks("deck.ppt", mode="sentence",       sentences_per_chunk=3)
chunks = get_chunks("deck.ppt", mode="page_aware",     paragraphs_per_page=15)

# Streaming — all 7 modes supported
for chunk in stream_chunks("deck.ppt", mode="section"):
    process(chunk)
```

| Mode | Description |
|---|---|
| `default` / `structural` | One chunk per paragraph: slide title (`heading`), bullet (`bullet_list`), or prose. Short paragraphs (< 80 chars) merge into a `short_disconnected_paragraph` chunk. |
| `section` | All paragraphs of a slide grouped into one chunk (≤ 2 000 chars) — effectively one chunk per slide. |
| `semantic` | Paragraphs merged by keyword overlap / reference continuity (≤ 1 200 chars). |
| `sliding_window` | Overlapping windows of N paragraphs. Params: `window_size` (default 3), `overlap` (default 1). |
| `sentence` | N sentences per chunk. Param: `sentences_per_chunk` (default 3). |
| `page_aware` | Chunks at slide boundaries (one chunk per slide), with paragraph-count fallback. Param: `paragraphs_per_page` (default 15). |

**Format notes**:
- Slides are read from the `SlideListWithText` (slides list) in presentation order; speaker notes and master text are excluded.
- Slide titles become `Heading(2)`; body placeholders that contain multiple lines are emitted as bullet list items (`- item` in Markdown), matching how PowerPoint renders them. Single-paragraph bodies stay prose.
- Freeform (non-placeholder) text boxes are merged in and de-duplicated, so text added directly onto a slide is not dropped.
- A `---` separator is inserted between slides in the chunk stream and in Markdown.

> **Markdown conversion**: `get_markdown("deck.ppt")` converts the `.ppt` to Markdown — slide titles become `##`, bullets become `- item`, and slides are separated by `---`.

> **Image extraction**: all 7 PPT modes support `list_images=True`. Image bytes are read from the `Pictures` stream of the CFB container; each slide's shapes are matched to the OfficeArt blip store through their `Pib` properties. Image chunks are grouped ahead of the text chunks with `content_type="image"`, `content` equal to the image's hash filename, and metadata keys `image_name` and `page_number` (the 1-based slide the image appears on). Slide attribution is applied only when the slide drawings can be matched 1:1 to the extracted slide list — otherwise images are still extracted but `page_number` is `null` (never misattributed). Master/notes images are excluded. Only JPEG and PNG payloads are extracted; EMF/WMF/PICT/DIB/TIFF BLIPs are silently skipped. Text chunks are byte-for-byte identical to the `list_images=False` output. `get_markdown("deck.ppt", list_images=True)` inserts `![](hash.ext)` refs inside the owning slide's section (unattributed images go at the end).

> **Streaming**: all 7 modes are supported, reusing the DOC iterators after the slide text is extracted.

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

> **Image-aware chunking**: HTML (`.html`, `.htm`) supports `list_images=True`. Pass it to `get_chunks` to receive a `ChunksResult`. Two image sources are supported: base64 data URIs (`data:image/...;base64,...` embedded in `src`) and local file references resolved relative to the HTML file's parent directory. Remote URLs are silently skipped. Image chunks have `content_type="image"`, `content` equal to the image's hash filename, and metadata keys `image_name`, `alt_text`, and `document_metadata`. Text chunks are byte-for-byte identical to the `list_images=False` output. `get_markdown(..., list_images=True)` appends `![alt](hash.ext)` references at the end of the Markdown string.

---

### XLSX / XLS modes

Both `.xlsx` and `.xls` files are handled by the same chunker. All 6 modes are available for batch and streaming:

```python
from py_chunks import get_chunks, stream_chunks
from py_chunks.chunkers.xlsx import chunk_xlsx, stream_chunk_xlsx

# Batch — via unified API
# Row count is set via sentences_per_chunk (it maps to rows_per_chunk for XLSX/XLS).
# For max_chunk_chars, sheet_names, or other XLSX-specific params, use chunk_xlsx() directly.
chunks = get_chunks("data.xlsx",   mode="row", sentences_per_chunk=5)   # → 5 rows/chunk
chunks = get_chunks("legacy.xls",  mode="row", sentences_per_chunk=5)   # same for XLS

# Batch — via format-specific chunker (returns chunks + timing)
chunks, timing = chunk_xlsx("data.xlsx", mode="row",            rows_per_chunk=5)
chunks, timing = chunk_xlsx("data.xlsx", mode="table",          max_chunk_chars=3000)
chunks, timing = chunk_xlsx("data.xlsx", mode="sheet",          max_chunk_chars=5000)
chunks, timing = chunk_xlsx("data.xlsx", mode="sliding_window", window_size=4, overlap=1)
chunks, timing = chunk_xlsx("data.xlsx", mode="page_aware",     max_chunk_chars=3000)
chunks, timing = chunk_xlsx("data.xlsx", mode="semantic",       rows_per_chunk=10)

# Filter to specific sheets
chunks, _ = chunk_xlsx("data.xlsx", mode="row", sheet_names=["Sales", "Q4"])

# Streaming — identical output to batch
# Use sentences_per_chunk via unified API, or stream_chunk_xlsx for full control.
for chunk in stream_chunks("data.xlsx", mode="row", sentences_per_chunk=5):  # → 5 rows/chunk
    print(chunk["content"])

for chunk in stream_chunk_xlsx("data.xlsx", mode="sliding_window", window_size=4, overlap=1):
    embed_and_store(chunk)
```

| Mode | `content_type` | Description |
|---|---|---|
| `row` | `row_document` | Groups N consecutive data rows into one chunk. Header row is auto-detected and excluded from content. Param: `rows_per_chunk` (default 1). |
| `table` | `table_region` | Named Excel tables (XLSX only) or heuristic contiguous data regions per sheet. For XLS and sheets without named tables, falls back to bounding-box detection. Param: `max_chunk_chars`. |
| `sheet` | `sheet` | One chunk per sheet (split by `max_chunk_chars` if needed). Includes named-table metadata. Param: `max_chunk_chars`. |
| `sliding_window` | `row_window` | Overlapping windows of N rows. Params: `window_size` (default 3), `overlap` (default 1, must be `< window_size`). |
| `page_aware` | `sheet_region` | Chunks by Excel print areas (XLSX only); falls back to the full sheet when no print area is defined. For XLS, always uses the full-sheet fallback. Param: `max_chunk_chars`. |
| `semantic` | `semantic_group` | Detects the column with the lowest cardinality of string values, sorts by it, and groups rows sharing the same category value. Falls back to fixed-size chunking when no suitable column is found. Param: `rows_per_chunk` (used for the fallback). |

**Image-aware chunking**: all 6 XLSX modes support `list_images=True`. Pass it to `get_chunks` to receive a `ChunksResult`. Image chunks have `content_type="image"`, `content` equal to the image hash filename, and metadata keys `sheet_name`, `sheet_index`, `image_name`, and `alt_text`. XLS files return `images={}`.

**Parameters accepted by `chunk_xlsx` and `stream_chunk_xlsx`:**

| Parameter | Type | Default | Description |
|---|---|---|---|
| `file_path` | str | — | Path to `.xlsx` or `.xls` file |
| `mode` | str | `"row"` | One of the 6 modes above |
| `rows_per_chunk` | int | 1 | Rows per chunk (`row` mode and `semantic` fallback). Must be `> 0`. |
| `window_size` | int | 3 | Window size in rows (`sliding_window` mode). Must be `>= 1`. |
| `overlap` | int | 1 | Overlapping rows between windows. Must be `< window_size`. |
| `include_headers` | bool | True | Prefix each row value with its column header (`key: value` format). |
| `sheet_names` | list[str] \| None | None | Process only the named sheets; processes all sheets when `None` or `[]`. |
| `skip_empty_rows` | bool | True | Skip rows where every cell is empty. |
| `max_chunk_chars` | int | 2000 | Character limit per chunk (`table`, `sheet`, `page_aware` modes). |
| `serialize_as` | str | `"key_value"` | Row serialization format. Only `"key_value"` is currently supported (each cell rendered as `"Header: value"`). Only applies to `chunk_xlsx`; `stream_chunk_xlsx` always uses `"key_value"`. |

**XLS vs XLSX differences:**

| Feature | XLSX | XLS |
|---|---|---|
| Named table detection (`table` mode) | ZIP XML (`table1.xml`) — full named-table metadata | Not available — heuristic bounding-box only; `is_named_table` is always `false` |
| Print area detection (`page_aware` mode) | Parsed from `xl/workbook.xml` | Not available — always uses full-sheet fallback; `has_print_area` is always `false` |
| Named table metadata in `sheet` mode | `has_named_tables: true/false`, `named_tables: [...]` | Always `has_named_tables: false`, `named_tables: []` |
| All other modes | Identical | Identical |

**XLSX / XLS metadata fields by mode:**

| Mode | Notable metadata keys |
|---|---|
| `row` | `sheet_name`, `sheet_index`, `row_index`, `header_row`, `col_count`, `rows_per_chunk`, `actual_row_count`, `chunk_index` |
| `table` | `sheet_name`, `sheet_index`, `table_name`, `is_named_table`, `header_row`, `start_row`, `end_row`, `start_col`, `end_col`, `row_count`, `col_count`, `chunk_index`, `is_split`, `split_part` |
| `sheet` | `sheet_name`, `sheet_index`, `row_count`, `col_count`, `header_row`, `has_named_tables`, `named_tables`, `chunk_index`, `is_split`, `split_part` |
| `sliding_window` | `sheet_name`, `sheet_index`, `window_size`, `overlap`, `actual_row_count`, `window_index`, `start_row`, `end_row`, `header_row`, `col_count`, `chunk_index` |
| `page_aware` | `sheet_name`, `sheet_index`, `has_print_area`, `print_area_ref`, `start_row`, `end_row`, `start_col`, `end_col`, `row_count`, `col_count`, `header_row`, `region_index`, `chunk_index`, `is_split`, `split_part` |
| `semantic` | `sheet_name`, `sheet_index`, `category_column`, `category_value`, `used_fallback`, `low_grouping_quality`, `avg_group_size`, `start_row`, `end_row`, `actual_row_count`, `header_row`, `col_count`, `group_index`, `chunk_index` |

> **Unified API limitations**: the `get_chunks` / `stream_chunks` entry points do not expose `rows_per_chunk` or `max_chunk_chars` as named parameters for XLSX / XLS. Row count is controlled via `sentences_per_chunk` (which maps to `rows_per_chunk` for these formats). Character limits (`max_chunk_chars`) for `table`, `sheet`, and `page_aware` modes cannot be customised through the unified API — use `chunk_xlsx` / `stream_chunk_xlsx` directly for full parameter control.

> **Streaming memory profile**: `row` and `sliding_window` pre-parse all sheet data once (calamine reads the entire file on open — there is no incremental I/O at the format level), then build and yield one chunk per `__next__`. The other four modes require global sheet analysis before the first chunk can be emitted, so they materialise all chunks at construction time and drain them lazily. In both cases the streaming iterator yields one chunk at a time.

> **Header detection**: the first all-string row in each sheet is automatically detected as the header row and excluded from chunk content. Columns without a header label are named `Column 1`, `Column 2`, etc.

---

### CSV modes

CSV files support a smaller mode set than the spreadsheet formats, but the API shape is the same for batch and streaming:

```python
from py_chunks import get_chunks, stream_chunks
from py_chunks.chunkers.csv import chunk_csv, stream_chunk_csv

chunks = get_chunks("data.csv", mode="default")
chunks = get_chunks("data.csv", mode="sliding_window", window_size=4, overlap=1)
chunks = get_chunks("data.csv", mode="page_aware", paragraphs_per_page=3)

chunks, timing = chunk_csv("data.csv", mode="row", rows_per_chunk=10, delimiter=",")

for chunk in stream_chunk_csv("data.csv", mode="row", rows_per_chunk=10):
    print(chunk["content"])
```

| Mode | `content_type` | Description |
|---|---|---|
| `row` / `default` | `row_group` | Groups N consecutive data rows into one chunk. Header row is preserved in metadata and included in content when `include_headers=True`. |
| `sliding_window` | `row_window` | Overlapping windows of N rows. Params: `window_size` and `overlap`. |
| `page_aware` | `row_group` | CSV-friendly alias for row chunking. The unified API maps `paragraphs_per_page` to CSV row count for this mode. |

CSV-specific options:

- `delimiter`: one of `None`, `,`, `\t`, `;`, or `|`. When omitted, the first non-empty non-comment line is scanned to detect the delimiter.
- `encoding`: one of `utf-8`, `utf-8-bom`, `latin-1`, or `windows-1252`.
- `skip_empty_rows`: skips rows whose cells are all empty or whitespace-only.

---

## Markdown Conversion

`get_markdown()` converts a document to a Markdown string in a single call. Use it when you want the full document as Markdown rather than split into chunks — for example, to feed into an LLM context window, display in a UI, or pipe into another tool.

```python
from py_chunks import get_markdown

# From a file path
md = get_markdown("report.docx")
md = get_markdown("legacy.doc")
md = get_markdown("deck.pptx")
md = get_markdown("paper.pdf")
md = get_markdown("page.html")
md = get_markdown("notes.md")       # returned as-is
md = get_markdown("readme.txt")     # returned as-is
md = get_markdown("data.xlsx")
md = get_markdown("data.csv")

# From bytes (e.g. API upload)
md = get_markdown(file_bytes, filename="report.docx")

# From a file-like object
from io import BytesIO
md = get_markdown(BytesIO(data), filename="document.pdf")
```

### Supported extensions for `get_markdown`

| Extension(s) | What is produced |
|---|---|
| `.docx` | Full fidelity: headings (`#`–`######` from Word heading styles / outline levels), unordered lists (`- item`), ordered lists (`1. item`) with per-level indentation, pipe tables, fenced code blocks, hyperlinks as `[text](url)`, page/section breaks as `---`, footnotes and endnotes as `[^id]: text` appended at the end. Images: rendered as `[Image: alt]` / `[Image]` by default; use `list_images=True` to get `![](hash.ext)` refs and the raw image bytes in `MarkdownResult.images` |
| `.doc` | H1 → `#`, H2–H3 → `##`, H4+ → `###`; lists → `- item`; each table paragraph → pipe row with `\| --- \|` separator; page breaks → `---`; plain paragraphs as-is. Images: ignored by default; use `list_images=True` to get `![](hash.ext)` refs at the anchoring paragraph and raw JPEG/PNG bytes in `MarkdownResult.images` |
| `.pptx` | Presentation title → `# Title`; PPTX sections → `# Section Name`; each slide → `## Slide N: Title` (or `## Slide N`); paragraphs as plain text; unordered bullets (`- item`) and ordered bullets (`1. item`) with per-level indentation; pipe tables; speaker notes as `> **Notes:** …`; slides/sections separated by `---`. Images: rendered as `[Image: alt]` by default; use `list_images=True` to get `![](hash.ext)` refs and the raw image bytes in `MarkdownResult.images` |
| `.ppt` | Slide titles → `##`; body placeholders → `- item` bullets (multi-line) or prose; slides separated by `---`. Freeform text boxes are included. Images: ignored by default; use `list_images=True` to get `![](hash.ext)` refs inside the owning slide and raw JPEG/PNG bytes in `MarkdownResult.images` |
| `.pdf` | Headings inferred from font size vs document average → `#` / `##` / `###`; bullet lists preserved or normalized to `- item`; tables detected by tab/multi-space alignment → pipe tables; page boundaries → `---`. Images: ignored by default; use `list_images=True` to get `![](hash.png)` refs at the top of each page and the raw PNG bytes in `MarkdownResult.images` |
| `.html`, `.htm` | H1–H6 → `#`–`######`; paragraphs as plain text; ordered lists → `1. item`; unordered lists → `- item`; code blocks → fenced ` ``` `; pipe tables with auto-detected header row; `\|` in cells escaped. Images: ignored by default; use `list_images=True` to extract base64 data URIs and local file references as `![alt](hash.ext)` refs appended at the end, plus raw bytes in `MarkdownResult.images` |
| `.xlsx`, `.xls` | Each non-empty sheet → `## SheetName` heading + pipe table; sheets separated by `---`. Images: ignored by default; use `list_images=True` to get `![](hash.ext)` refs after each sheet table and raw bytes in `MarkdownResult.images` (XLSX only; XLS returns `images={}`). |
| `.csv` | Single pipe table; first row = header with `\| --- \|` separator; `\|` in cells escaped; empty rows skipped; delimiter auto-detected or manually set; accepts `delimiter` and `encoding` params — see `csv_to_markdown` below |
| `.md` | Returned as-is (already Markdown — no transformation) |
| `.txt` | Returned as-is (plain text — no transformation) |

### `get_markdown` signature

```python
# list_images=False (default) — returns a plain Markdown string
get_markdown(source, *, filename: str | None = None, list_images: bool = False) -> str

# list_images=True — returns a MarkdownResult with markdown + image bytes
get_markdown(source, *, filename: str | None = None, list_images: bool = True) -> MarkdownResult
```

| Parameter | Type | Description |
|---|---|---|
| `source` | str, Path, bytes, bytearray, memoryview, file-like | Document source. Local file path, raw bytes, or file-like object. |
| `filename` | str \| None | Required when source is `bytes`, `bytearray`, `memoryview`, or a file-like object without a `.name` attribute. |
| `list_images` | bool | `False` (default) returns a plain `str`. `True` returns a `MarkdownResult`. Image extraction is active for `.docx`, `.pptx`, `.xlsx`, `.html`, `.htm`, `.pdf`, `.doc`, and `.ppt`; other formats return an empty `images` dict. |

**Return types**

`get_chunks` with `list_images=False` (default) → `list[dict]` — the chunk list as usual.

`get_chunks` with `list_images=True` → `ChunksResult` — a dataclass with two fields:

```python
@dataclass
class ChunksResult:
    chunks: list[dict]        # all chunks: image chunks first, then text chunks
    images: dict[str, bytes]  # {hash_filename: raw_bytes} — populated for .docx, .pptx, .xlsx, .html/.htm, .pdf
```

Image chunks inside `ChunksResult.chunks` have `content_type="image"`, `content` equal to the hash filename (e.g. `"8c4a2b4ccec6f521.jpeg"`), and format-specific metadata:

- **DOCX**: `image_name`, `alt_text`, `page_number`, `document_metadata`
- **PPTX**: `slide_number`, `image_name`, `alt_text`, `document_metadata`
- **XLSX**: `sheet_name`, `sheet_index`, `image_name`, `alt_text`
- **HTML**: `image_name`, `alt_text`, `document_metadata` (`source_type: "html"`)
- **PDF**: `page_number`, `image_name`, `document_metadata` (`source_type: "pdf"`, `total_pages`) — no `alt_text`; images are always re-encoded to `.png`

`get_markdown` with `list_images=False` (default) → `str` — the full document as a Markdown string.

`get_markdown` with `list_images=True` → `MarkdownResult` — a dataclass with two fields:

```python
@dataclass
class MarkdownResult:
    markdown: str             # full Markdown string; images referenced as ![](hash.ext)
    images: dict[str, bytes]  # {filename: raw_bytes} — populated for .docx, .pptx, .xlsx, .html/.htm, .pdf, .doc, .ppt
```

**Image extraction details (DOCX / PPTX / XLSX / HTML / PDF / DOC / PPT)**

- Each embedded image is hashed (content hash, not path) and named `{16-char hex}.{ext}`, e.g. `8c4a2b4ccec6f521.png`.
- The same hash means the same file: if an image appears multiple times in the document it is stored once in `images` but referenced at every occurrence in `markdown`.
- For DOCX / PPTX / XLSX / HTML, only web-renderable formats are extracted: `.png`, `.jpg`, `.jpeg`, `.gif`, `.webp`. Vector/metafile formats (`.emf`, `.wmf`, etc.) are silently skipped.
- For **PDF**, each image is decoded and re-encoded to `.png` regardless of how it was stored, so every key in `images` ends in `.png`. References are placed at the top of each page's Markdown, and scanned/image-only PDFs still yield one PNG per page.
- For legacy **DOC / PPT**, images are decoded from the OfficeArt (MS-ODRAW) BLIP records in the binary streams; only JPEG and PNG payloads are supported (EMF/WMF/PICT/DIB/TIFF are silently skipped). `.doc` refs are placed at the anchoring paragraph; `.ppt` refs inside the owning slide; unanchored images go at the end.
- Image references in `markdown` use `![](hash.ext)` — you can serve or embed the corresponding bytes directly.

**Raises**

| Exception | Condition |
|---|---|
| `FileNotFoundError` | Path does not exist |
| `ValueError` | Unsupported extension or missing `filename` for bytes / fileobj inputs |
| `TypeError` | Unsupported source type |
| `RuntimeError` | Rust-level failure (e.g. scanned PDF with no text layer) |

> **Note**: `get_markdown` does not support URLs. For URL sources, download the bytes first and pass them with a `filename`.

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
| **DOC** | All 7 | `DocStructuralIterator` for `default`/`structural`; dedicated per-mode Rust iterators for the other 5 | Binary stream parsed once upfront via piece table reconstruction; chunks emitted lazily. Output equals `get_chunks` for every mode. |
| **PPTX** | All 7 | Batch-drain | PPTX requires the full ZIP up front, so chunks are computed once at construction and yielded one per `__next__`. |
| **PPT** | All 7 | Reuses the DOC iterators (`default`/`structural` + dedicated per-mode) | Slide text extracted once from the binary record stream, then chunks emitted lazily. Output equals `get_chunks` for every mode. |
| **XLSX / XLS** | All 6 | State machine for `row` / `sliding_window`; batch-drain for `table` / `sheet` / `page_aware` / `semantic` | calamine reads the full file on open (no incremental I/O at format level). `row` and `sliding_window` build one chunk per `__next__` from pre-parsed row data. The other four modes require global analysis first and materialise all chunks at iterator construction. Output is identical to `chunk_xlsx` for every mode. |
| **CSV** | All 3 | Background thread + `mpsc` channel for `row` / `default` / `page_aware`; `VecDeque` rolling buffer for `sliding_window` | True line-by-line worker — never loads the full file. `sliding_window` streaming uses an O(window_size) rolling buffer. Output is identical to `chunk_csv` for every mode. |

> **Parity guarantee**: streaming output equals `list(get_chunks(...))` for every format and every supported mode. This is verified by the test suite in `py_chunks/tests/`.

### Streaming examples

```python
from py_chunks import stream_chunks
from py_chunks.chunkers.csv import stream_chunk_csv

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

# DOC (Word 97-2003) — all 7 modes
for chunk in stream_chunks("legacy.doc", mode="structural"):      send_to_queue(chunk)
for chunk in stream_chunks("legacy.doc", mode="semantic"):        process(chunk)
for chunk in stream_chunks("legacy.doc", mode="section"):         index(chunk)
for chunk in stream_chunks("legacy.doc", mode="sentence", sentences_per_chunk=3): embed(chunk)
for chunk in stream_chunks("legacy.doc", mode="page_aware", paragraphs_per_page=15): store(chunk)

# PPTX — any mode
for chunk in stream_chunks("deck.pptx", mode="semantic"):
    ...

# PPT (PowerPoint 97-2003) — all 7 modes
for chunk in stream_chunks("legacy.ppt", mode="section"):
    index(chunk)

# XLSX / XLS — all 6 modes
# For row/semantic count control via unified API, use sentences_per_chunk (it maps to rows_per_chunk).
# For max_chunk_chars or sheet_names control, use stream_chunk_xlsx() directly.
for chunk in stream_chunks("data.xlsx", mode="row", sentences_per_chunk=10):       # → 10 rows/chunk
    embed_and_index(chunk)

for chunk in stream_chunks("report.xls", mode="sliding_window", window_size=5, overlap=2):
    process(chunk)

for chunk in stream_chunk_xlsx("data.xlsx", mode="table", max_chunk_chars=3000):   # max_chunk_chars requires stream_chunk_xlsx
    store_in_db(chunk)

for chunk in stream_chunks("data.xlsx", mode="semantic", sentences_per_chunk=20):  # → 20-row fallback groups
    handle(chunk)

# CSV — all 3 modes
# For row count control via unified API, use sentences_per_chunk (it maps to rows_per_chunk).
# For delimiter/encoding control, use stream_chunk_csv() directly.
for chunk in stream_chunks("data.csv", mode="row", sentences_per_chunk=50):        # → 50 rows/chunk
    embed_and_index(chunk)

for chunk in stream_chunks("data.csv", mode="sliding_window", window_size=5, overlap=1):
    process(chunk)

for chunk in stream_chunk_csv("data.csv", mode="page_aware", rows_per_chunk=100, delimiter="\t"):
    store_in_db(chunk)

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

> **Note**: `get_markdown` accepts file paths, bytes, and file-like objects, but does **not** support URLs. Download the content first and pass as bytes with a `filename`.

---

## Supported Formats

| Format | Extensions | Batch modes | Streaming modes |
|---|---|---|---|
| PDF        | `.pdf`            | All 7 | All 7 (background thread) |
| DOCX       | `.docx`           | All 7 | All 7 (dedicated iterator per mode) |
| DOC        | `.doc`            | All 7 | All 7 (dedicated iterator per mode) |
| PPTX       | `.pptx`           | All 7 | All 7 (batch-drain) |
| PPT        | `.ppt`            | All 7 | All 7 (dedicated iterator per mode) |
| Markdown   | `.md`             | All 7 | All 7 (state machine for `structural` / `semantic`) |
| HTML       | `.html`, `.htm`   | All 7 | All 7 (state machine for `structural` / `semantic`) |
| Plain Text | `.txt`            | All 7 | All 7 (state machine for `structural` / `semantic`) |
| Excel      | `.xlsx`, `.xls`   | All 6 | All 6 (`row` / `sliding_window` state machine; others batch-drain) |
| CSV        | `.csv`            | All 3 | All 3 (background thread; `VecDeque` rolling buffer for `sliding_window`) |

The 7 document modes are: `default`, `structural`, `section`, `semantic`, `sliding_window`, `sentence`, `page_aware`.

The 6 spreadsheet modes are: `row`, `table`, `sheet`, `sliding_window`, `page_aware`, `semantic`.

The 3 CSV modes are: `row` / `default`, `sliding_window`, `page_aware`.

**`get_markdown` supported extensions**: `.doc`, `.docx`, `.pptx`, `.ppt`, `.pdf`, `.html`, `.htm`, `.xlsx`, `.xls`, `.csv`, `.txt`, `.md`

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
    list_images: bool = False,
) -> list[dict] | ChunksResult
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

```python
get_markdown(
    source,
    *,
    filename: str | None = None,
    list_images: bool = False,
) -> str | MarkdownResult
```

**Parameters**

| Parameter | Type | Default | Description |
|---|---|---|---|
| `source` | str, Path, bytes, file-like, upload, URL | — | Document source. Auto-detected. (`get_markdown` does not support URLs.) |
| `filename` | str \| None | None | Required when source is `bytes` or a file object without a `.name` attribute. |
| `mode` | str | `"default"` | Chunking mode. Applies to **every** supported format (PDF, DOCX, DOC, PPTX, MD, HTML, TXT). One of `default`, `structural`, `section`, `semantic`, `sliding_window`, `sentence`, `page_aware`. |
| `window_size` | int | 3 | Number of blocks per window (`sliding_window` mode). Must be `> 0`. |
| `overlap` | int | 1 | Overlapping blocks between windows (`sliding_window` mode). Must be `< window_size`. |
| `sentences_per_chunk` | int | 3 | Sentences per chunk (`sentence` mode). Must be `> 0`. **For XLSX / XLS and CSV** via the unified API, this value is re-used as `rows_per_chunk` (those formats have no sentence concept). Use `chunk_xlsx` / `chunk_csv` directly when you need explicit `rows_per_chunk` control. |
| `paragraphs_per_page` | int | 15 | Block / paragraph quota before a page flush (`page_aware` mode). Must be `> 0`. For **PPTX** this means *slides per chunk* and the format-level default is `5`. |
| `list_images` | bool | `False` | `get_chunks`: when `True` returns a `ChunksResult` instead of `list[dict]`. `get_markdown`: when `True` returns a `MarkdownResult` instead of `str`. Image extraction is active for `.docx`, `.pptx`, `.xlsx`, `.html`, `.htm`, `.pdf`, `.doc`, and `.ppt`; all other formats return an empty `images` dict. **Not available for `stream_chunks`** — use `get_chunks(..., list_images=True)` if you need image bytes. |

**Returns** — `list[dict]` (batch, `list_images=False`) or `ChunksResult` (batch, `list_images=True`) or `Iterator[dict]` (streaming) or `str` / `MarkdownResult` (`get_markdown`). Each chunk dict:

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
| `RuntimeError` | Rust-level failure (e.g. no extractable text in PDF, pre-Word 97 `.doc` file) |
| `NotImplementedError` | Streaming requested for an unsupported format/mode |

---

### When to use the unified API vs format-specific chunkers

| Use `get_chunks` / `stream_chunks` when… | Use `chunk_<fmt>` / `stream_chunk_<fmt>` directly when… |
|---|---|
| You don't know the format ahead of time | You need `max_chunk_chars`, `delimiter`, `sheet_names`, or other format-specific params |
| You're building a multi-format pipeline | You want per-call timing data (`{"rust_ms": …, "python_ms": …}`) |
| You need URL, bytes, or upload source support | You only process one format and want to skip source detection overhead |
| The default parameter values are sufficient | You need `rows_per_chunk` for XLSX/CSV (instead of the `sentences_per_chunk` proxy) |

> **Rule of thumb**: start with `get_chunks`. Switch to `chunk_xlsx`, `chunk_csv`, etc. only when you hit a parameter the unified API doesn't expose.

### Format-specific chunkers (advanced)

Each format also has a direct module that returns `(chunks, timing)`, where `timing` is `{"rust_ms": float, "python_ms": float}`. Use these when you want per-call timing data or when you only need one format and want to skip source-type detection.

```python
from py_chunks.chunkers.pdf  import chunk_pdf,  chunk_pdf_with_images, stream_chunk_pdf, pdf_to_markdown, pdf_to_markdown_with_images
from py_chunks.chunkers.docx import chunk_docx, chunk_docx_with_images, stream_chunk_docx, docx_to_markdown, docx_to_markdown_with_images
from py_chunks.chunkers.doc  import chunk_doc,  stream_chunk_doc,  doc_to_markdown
from py_chunks.chunkers.pptx import chunk_pptx, chunk_pptx_with_images, stream_chunk_pptx, chunk_pptx_with_strategy, pptx_to_markdown, pptx_to_markdown_with_images
from py_chunks.chunkers.ppt  import chunk_ppt,  stream_chunk_ppt,  ppt_to_markdown
from py_chunks.chunkers.html import chunk_html, chunk_html_with_images, stream_chunk_html, html_to_markdown, html_to_markdown_with_images
from py_chunks.chunkers.md   import chunk_md,   stream_chunk_md,   md_to_markdown
from py_chunks.chunkers.txt  import chunk_txt,  stream_chunk_txt,  txt_to_markdown
from py_chunks.chunkers.xlsx import chunk_xlsx, chunk_xlsx_with_images, stream_chunk_xlsx, xlsx_to_markdown, xlsx_to_markdown_with_images  # handles both .xlsx and .xls
from py_chunks.chunkers.csv  import chunk_csv,  stream_chunk_csv,  csv_to_markdown

# Batch with timing
chunks, timing = chunk_pdf("file.pdf", mode="section")
print(f"Rust: {timing['rust_ms']} ms  Python: {timing['python_ms']} ms")

chunks, timing = chunk_md("notes.md", mode="semantic")
chunks, timing = chunk_html("page.html", mode="sliding_window", window_size=4, overlap=1)
chunks, timing = chunk_txt("log.txt", mode="sentence", sentences_per_chunk=2)
chunks, timing = chunk_pptx("deck.pptx", mode="page_aware", paragraphs_per_page=5)
chunks, timing = chunk_doc("legacy.doc", mode="section")
chunks, timing = chunk_ppt("legacy.ppt", mode="section")

# Legacy PPTX strategy wrapper (kept for backward compatibility)
chunks, timing = chunk_pptx_with_strategy("deck.pptx", strategy="structural")

# Streaming — all formats
for chunk in stream_chunk_pdf("report.pdf", mode="semantic"):          ...
for chunk in stream_chunk_docx("doc.docx", mode="structural"):         ...
for chunk in stream_chunk_docx("doc.docx", mode="semantic"):           ...
for chunk in stream_chunk_docx("doc.docx", mode="section"):            ...
for chunk in stream_chunk_docx("doc.docx", mode="sentence", sentences_per_chunk=3): ...
for chunk in stream_chunk_doc("legacy.doc", mode="structural"):        ...
for chunk in stream_chunk_doc("legacy.doc", mode="semantic"):          ...
for chunk in stream_chunk_doc("legacy.doc", mode="section"):           ...
for chunk in stream_chunk_doc("legacy.doc", mode="sentence", sentences_per_chunk=3): ...
for chunk in stream_chunk_md("book.md", mode="sentence", sentences_per_chunk=2): ...
for chunk in stream_chunk_html("page.html", mode="section"):           ...
for chunk in stream_chunk_txt("log.txt", mode="page_aware", paragraphs_per_page=20): ...
for chunk in stream_chunk_pptx("deck.pptx", mode="semantic"):          ...

# XLSX / XLS — all 6 modes, batch and streaming
chunks, timing = chunk_xlsx("data.xlsx", mode="row",            rows_per_chunk=5)
chunks, timing = chunk_xlsx("data.xlsx", mode="table",          max_chunk_chars=3000)
chunks, timing = chunk_xlsx("data.xlsx", mode="sheet",          max_chunk_chars=5000)
chunks, timing = chunk_xlsx("data.xlsx", mode="sliding_window", window_size=4, overlap=1)
chunks, timing = chunk_xlsx("data.xlsx", mode="page_aware",     max_chunk_chars=3000)
chunks, timing = chunk_xlsx("data.xlsx", mode="semantic",       rows_per_chunk=10)
chunks, timing = chunk_xlsx("legacy.xls", mode="row",           rows_per_chunk=5)  # XLS works identically

for chunk in stream_chunk_xlsx("data.xlsx",  mode="row",            rows_per_chunk=10):  ...
for chunk in stream_chunk_xlsx("data.xlsx",  mode="sliding_window", window_size=4, overlap=1): ...
for chunk in stream_chunk_xlsx("legacy.xls", mode="semantic",       rows_per_chunk=20): ...

# CSV — batch with timing
chunks, timing = chunk_csv("data.csv", mode="row",            rows_per_chunk=10)
chunks, timing = chunk_csv("data.csv", mode="sliding_window", window_size=5, overlap=1)
chunks, timing = chunk_csv("data.csv", mode="page_aware",     rows_per_chunk=100)
chunks, timing = chunk_csv("data.csv", mode="row",            delimiter="\t", encoding="utf-8")

# CSV — streaming
for chunk in stream_chunk_csv("data.csv", mode="row",            rows_per_chunk=50):          ...
for chunk in stream_chunk_csv("data.csv", mode="sliding_window", window_size=5, overlap=1):   ...
for chunk in stream_chunk_csv("data.csv", mode="page_aware",     rows_per_chunk=100):         ...

# Markdown conversion — direct format wrappers
md = docx_to_markdown("report.docx")          # full DOCX → Markdown (headings, lists, tables, footnotes, hyperlinks…)
md = doc_to_markdown("legacy.doc")            # DOC binary → Markdown (headings, lists, tables, page breaks)
md = ppt_to_markdown("legacy.ppt")           # PPT binary → Markdown (slide titles as ##, bullets, slides split by ---)
md = pdf_to_markdown("paper.pdf")             # PDF → Markdown (headings by font size, lists, tables, page separators)
md = pptx_to_markdown("deck.pptx")           # PPTX → Markdown (title, sections, slides as ##, notes as blockquote)

# Image-aware chunking (DOCX, PPTX, HTML, PDF) — all 7 modes, returns (chunks, images)
chunks, images = chunk_docx_with_images("report.docx")                                # default/structural
chunks, images = chunk_docx_with_images("report.docx", mode="semantic")
chunks, images = chunk_docx_with_images("report.docx", mode="sliding_window", window_size=3, overlap=1)
chunks, images = chunk_pptx_with_images("deck.pptx")                                  # default/structural
chunks, images = chunk_pptx_with_images("deck.pptx", mode="section")
chunks, images = chunk_pptx_with_images("deck.pptx", mode="page_aware", paragraphs_per_page=5)
chunks, images = chunk_html_with_images("page.html")                                  # default/structural
chunks, images = chunk_html_with_images("page.html", mode="semantic")
chunks, images = chunk_pdf_with_images("report.pdf")                                  # default (fast path)
chunks, images = chunk_pdf_with_images("report.pdf", mode="page_aware", paragraphs_per_page=15)
# images: dict[str, bytes] — same as ChunksResult.images from get_chunks(list_images=True)

# Image-aware Markdown conversion (DOCX, PPTX, HTML, PDF)
md, images = docx_to_markdown_with_images("report.docx")  # images: dict[str, bytes]
md, images = pptx_to_markdown_with_images("deck.pptx")
md, images = html_to_markdown_with_images("page.html")     # base64 + local-file images
md, images = pdf_to_markdown_with_images("report.pdf")     # page-scoped images re-encoded to .png
# Each key in `images` is a content-hashed filename (e.g. "8c4a2b4ccec6f521.png");
# the same key appears as ![alt](8c4a2b4ccec6f521.png) in the markdown string.
md = html_to_markdown("page.html")            # HTML → Markdown (H1-H6, lists, tables, code blocks)
md = xlsx_to_markdown("data.xlsx")            # each sheet → ## heading + pipe table, separated by ---
md = csv_to_markdown("data.csv")              # pipe table, first row = header
md = csv_to_markdown("data.csv", delimiter=",", encoding="utf-8")   # explicit delimiter + encoding
md = txt_to_markdown("notes.txt")             # returned as-is
md = md_to_markdown("readme.md")              # returned as-is
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

**Concrete examples**

```python
# PDF — section mode
{
    "content": "Introduction\n\nThis document covers the key design principles ...",
    "content_type": "section",
    "metadata": {
        "section_heading": "Introduction",
        "section_level": 1,
        "heading_path": ["Introduction"],
        "page_number": 1,
        "paragraph_count": 3,
        "heading_font_size": 16.0,
    }
}

# DOCX — semantic mode
{
    "content": "The new authentication flow replaces the legacy token store. It uses short-lived JWTs ...",
    "content_type": "semantic",
    "metadata": {
        "section_heading": "Authentication",
        "section_heading_level": 2,
        "paragraph_count": 2,
        "merge_reason": "keyword_overlap",
        "document_metadata": {"header_text": "", "footer_text": "Page 4", "image_count": 0},
    }
}

# XLSX — row mode (3 rows/chunk via sentences_per_chunk=3)
{
    "content": "name: Alice\ndepartment: Engineering\nsalary: 95000\n---\nname: Bob\ndepartment: Sales\nsalary: 78000\n---\nname: Carol\ndepartment: Engineering\nsalary: 102000",
    "content_type": "row_document",
    "metadata": {
        "sheet_name": "Employees",
        "sheet_index": 0,
        "row_index": 1,
        "rows_per_chunk": 3,
        "actual_row_count": 3,
        "chunk_index": 0,
        "header_row": ["name", "department", "salary"],
        "col_count": 3,
    }
}

# CSV — sliding_window mode
{
    "content": "id,product,revenue\n101,Widget A,4200\n102,Widget B,3800\n103,Widget C,5100",
    "content_type": "row_window",
    "metadata": {
        "window_index": 0,
        "window_size": 3,
        "overlap": 1,
        "row_start": 1,
        "row_end": 3,
        "actual_row_count": 3,
        "col_count": 3,
        "header_row": ["id", "product", "revenue"],
        "chunk_index": 0,
    }
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
| `short_disconnected_paragraph` | Paragraph < 80 characters (also used for merged short paragraphs in DOC/DOCX structural mode) |
| `mixed_content` | DOCX structural block that merges a heading with its immediately following body element (e.g. a heading run that shares a `<w:p>` with body text) |
| `section` | Heading-scoped grouped content (`section` mode) |
| `semantic` | Heuristic topic-continuity group (`semantic` mode) |
| `sliding_window` | Fixed-size overlapping window (`sliding_window` mode) |
| `sentence` | Sentence-count group (`sentence` mode) |
| `page_aware` | Page boundary group (`page_aware` mode for document formats) |
| `row_document` | XLSX/XLS: N consecutive data rows (`row` mode) |
| `table_region` | XLSX/XLS: named table or heuristic data region (`table` mode) |
| `sheet` | XLSX/XLS: full sheet or split part (`sheet` mode) |
| `row_window` | XLSX/XLS and CSV: overlapping row window (`sliding_window` mode) |
| `sheet_region` | XLSX/XLS: print area or full sheet (`page_aware` mode) |
| `semantic_group` | XLSX/XLS: category-grouped rows or fallback fixed-size group (`semantic` mode) |
| `row_group` | CSV: N consecutive data rows (`row`, `default`, `page_aware` modes) |
| `image` | Embedded image extracted from DOCX, PPTX, XLSX, HTML, or PDF when `list_images=True`; `content` is the image's hash filename (e.g. `"8c4a2b4ccec6f521.png"`), bytes are in `ChunksResult.images`. For PDF the image is page-scoped (metadata carries `page_number`, no `alt_text`) and always re-encoded to `.png` |

### Metadata fields by mode

Metadata is a `dict` whose keys depend on both the format and the mode. The most useful keys are listed below; treat any field as optional and use `chunk["metadata"].get(key)`.

| Mode | Format(s) | Notable metadata keys |
|---|---|---|
| `default` / `structural` | PDF | `page_number`, `is_heading`, `avg_font_size` |
| `default` / `structural` | DOCX | `section_heading`, `section_heading_level`, `footnotes` (list of `{id, text}`), `endnotes` (list of `{id, text}`), `page_number`, `document_metadata` (`header_text`, `footer_text`, `image_count`). Inline images are emitted as `[Image: <alt>]` (or `[Image]` when no alt text). Footnote / endnote ids reference `word/footnotes.xml` / `word/endnotes.xml` and are anchored to the chunk that contains the referring paragraph. |
| `default` / `structural` | DOC | `source`, `chunk_index`, `total_chunks`, `paragraph_type` (`heading`/`normal`/`table`/`list_item`), `heading_level` (1–6 or null), `page_number` (always null — not available in the binary format) |
| `default` / `structural` | MD / HTML / TXT | `section_heading`, `document_metadata.source_type` |
| `default` / `structural` | PPTX | `slide_number`, `section_heading` (when detectable) |
| `section` | PDF | `page_number`, `section_heading`, `section_level`, `heading_path`, `paragraph_count`, `heading_font_size` |
| `section` | DOCX | `section_heading`, `section_heading_level`, `section_level`, `heading_path`, `document_metadata` |
| `section` | DOC | `source`, `chunk_index`, `total_chunks`, `paragraph_type`, `heading_level`, `page_number` |
| `section` | MD / HTML / TXT / PPTX | `section_heading`, `section_level`, `heading_path`, `paragraph_count` |
| `semantic` | PDF | `page_number`, `paragraph_count`, `merge_reason` |
| `semantic` | DOCX | `section_heading`, `section_heading_level`, `paragraph_count`, `merge_reason`, `document_metadata` |
| `semantic` | DOC | `source`, `chunk_index`, `total_chunks`, `paragraph_type`, `heading_level`, `page_number` |
| `semantic` | MD / HTML / TXT / PPTX | `paragraph_count`, `merge_reasons` (list), `primary_merge_reason`, `keyword_density`, `avg_block_length` (MD/TXT), `section_heading`, `heading_path`, `chunk_index`, `document_metadata` |
| `sentence` | PDF | `sentences_per_chunk`, `actual_sentence_count`, `chunk_index`, `source_paragraph_index` |
| `sentence` | DOCX | `sentences_per_chunk`, `actual_sentence_count`, `chunk_index`, `source_paragraph_index`, `source_paragraph_is_heading`, `source_paragraph_heading_level`, `source_paragraph_is_list`, `source_paragraph_is_table`, `document_metadata` |
| `sentence` | DOC | `source`, `chunk_index`, `total_chunks`, `paragraph_type`, `heading_level`, `page_number` |
| `sentence` | MD / HTML / TXT / PPTX | `sentences_per_chunk`, `actual_sentence_count`, `chunk_index`, `source_paragraph_index` |
| `sliding_window` | PDF | `window_size`, `overlap`, `window_index`, `paragraph_count`, `paragraph_range`, `page_number` |
| `sliding_window` | DOCX | `window_size`, `overlap`, `window_index`, `paragraph_indices`, `list_item_count`, `heading_count`, `paragraph_meta`, `document_metadata` |
| `sliding_window` | DOC | `source`, `chunk_index`, `total_chunks`, `paragraph_type`, `heading_level`, `page_number` |
| `sliding_window` | MD / HTML / TXT / PPTX | `window_size`, `overlap`, `window_index`, `paragraph_count`, `paragraph_range` |
| `page_aware` | PDF | `page_number`, `page_break_type`, `paragraph_count`, `document_metadata` |
| `page_aware` | DOCX | `page_number`, `page_break_type`, `paragraph_count`, `section_heading_level`, `headings`, `list_item_count`, `table_count`, `document_metadata` |
| `page_aware` | DOC | `source`, `chunk_index`, `total_chunks`, `paragraph_type`, `heading_level`, `page_number` |
| `page_aware` | MD / HTML / TXT | `page_number`, `page_break_type` (heading-boundary or paragraph-count), `paragraph_count` |
| `page_aware` | PPTX | `slide_numbers`, `paragraph_count` |

**CSV metadata fields by mode:**

| Mode | Notable metadata keys |
|---|---|
| `row` / `default` / `page_aware` | `row_start`, `row_end`, `row_count`, `col_count`, `header_row`, `delimiter_detected`, `encoding`, `chunk_index` |
| `sliding_window` | `window_index`, `window_size`, `overlap`, `row_start`, `row_end`, `actual_row_count`, `col_count`, `header_row`, `delimiter_detected`, `encoding`, `chunk_index` |

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

### Legacy `.doc` file (Word 97-2003)

```python
from py_chunks import get_chunks, get_markdown

# Chunk the document
chunks = get_chunks("contract.doc", mode="section")
for chunk in chunks:
    print(chunk["metadata"]["heading_level"], chunk["content"][:80])

# Or convert the whole thing to Markdown
md = get_markdown("contract.doc")
print(md)
```

### Convert any document to Markdown

```python
from py_chunks import get_markdown

# Works for all 12 supported extensions
for path in ["report.docx", "legacy.doc", "deck.pptx", "legacy.ppt", "paper.pdf",
             "data.xlsx", "data.csv", "notes.txt", "readme.md"]:
    md = get_markdown(path)
    print(f"--- {path} ---")
    print(md[:500])
```

### Extract images from DOCX, PPTX, XLSX, HTML, or PDF

Two APIs are available depending on whether you want chunks or Markdown.

#### Via `get_chunks` — receive image chunks alongside text chunks

```python
from py_chunks import get_chunks, ChunksResult

# Works for all 7 modes on DOCX, PPTX, HTML, and PDF; all 6 modes on XLSX
result = get_chunks("report.docx", list_images=True)
result = get_chunks("report.docx", mode="semantic", list_images=True)
result = get_chunks("deck.pptx",   list_images=True)
result = get_chunks("deck.pptx",   mode="section", list_images=True)
result = get_chunks("data.xlsx",   list_images=True)
result = get_chunks("page.html",   list_images=True)   # base64 data URIs + local file refs
result = get_chunks("page.html",   mode="semantic", list_images=True)
result = get_chunks("report.pdf",  list_images=True)   # page-scoped images, re-encoded to .png
result = get_chunks("report.pdf",  mode="page_aware", list_images=True)

assert isinstance(result, ChunksResult)

# Image bytes keyed by content-hash filename
for name, data in result.images.items():
    print(name, len(data))   # e.g. "8c4a2b4ccec6f521.jpeg  42891"

# Image chunks appear in result.chunks with content_type="image"
image_chunks = [c for c in result.chunks if c["content_type"] == "image"]
for chunk in image_chunks:
    print(chunk["content"])                    # hash filename — key into result.images
    # DOCX/PPTX/XLSX/HTML carry alt_text from the document:
    print(chunk["metadata"].get("alt_text"))
    # DOCX also has: page_number
    # PPTX also has: slide_number
    # XLSX also has: sheet_name, sheet_index
    # HTML also has: document_metadata (source_type: "html")
    # PDF  has:      page_number, document_metadata (source_type: "pdf") — no alt_text

# Text chunks are byte-for-byte identical to list_images=False
text_chunks = [c for c in result.chunks if c["content_type"] != "image"]
```

#### Via `get_markdown` — receive Markdown with image references

```python
from py_chunks import get_markdown, MarkdownResult

result = get_markdown("report.docx", list_images=True)
assert isinstance(result, MarkdownResult)

print(result.markdown)   # Markdown string; images referenced as ![](hash.ext)
print(result.images)     # {"8c4a2b4ccec6f521.png": b"\x89PNG...", ...}

# Save images to disk alongside the Markdown
import pathlib
out = pathlib.Path("output")
out.mkdir(exist_ok=True)
(out / "report.md").write_text(result.markdown)
for name, data in result.images.items():
    (out / name).write_bytes(data)

# Works the same for PPTX, XLSX, HTML, and PDF
result = get_markdown("deck.pptx", list_images=True)
result = get_markdown("data.xlsx", list_images=True)   # refs appended after each sheet table
result = get_markdown("page.html", list_images=True)   # refs appended at end of document
result = get_markdown("report.pdf", list_images=True)  # refs added at the top of each page

# Also works from bytes or file-like objects
data = open("report.docx", "rb").read()
result = get_markdown(data, filename="report.docx", list_images=True)
```

> **Supported image formats**: `.png`, `.jpg`, `.jpeg`, `.gif`, `.webp`. Vector/metafile formats (`.emf`, `.wmf`, etc.) are silently skipped — they are not renderable in browser contexts.
>
> **HTML image sources**: base64 data URIs (`data:image/...;base64,...`) are decoded inline; local file references are resolved relative to the HTML file's parent directory; remote URLs are silently skipped.
>
> **Deduplication**: if the same image appears multiple times in a document, it is stored once in `images` but an image chunk is emitted at each occurrence (for `get_chunks`) or referenced at each occurrence in `markdown` (for `get_markdown`).
>
> **Text parity guarantee**: non-image chunks from `list_images=True` are byte-for-byte identical to the output of `list_images=False` for every mode and every supported format.

### RAG ingestion pipeline

A common pattern: chunk documents from a folder, embed each chunk, and store in a vector database.

```python
import pathlib
from py_chunks import stream_chunks

SUPPORTED = {".pdf", ".docx", ".doc", ".pptx", ".md", ".html", ".htm",
             ".txt", ".xlsx", ".xls", ".csv"}

def ingest(file_path: str, embed_fn, store_fn):
    """Chunk a document and write each chunk to a vector store."""
    for chunk in stream_chunks(file_path, mode="semantic"):
        text = chunk["content"].strip()
        if not text:
            continue
        store_fn({
            "text": text,
            "embedding": embed_fn(text),
            "source": file_path,
            "content_type": chunk["content_type"],
            **chunk["metadata"],
        })

# Process a mixed-format document folder
docs = pathlib.Path("docs/")
for path in docs.rglob("*"):
    if path.suffix.lower() in SUPPORTED:
        ingest(str(path), my_embed_fn, my_store_fn)
```

> **Tip**: use `mode="section"` for document search (preserves heading context in metadata) and `mode="semantic"` for LLM retrieval (merges related paragraphs). For large spreadsheets use `stream_chunks(..., mode="row", sentences_per_chunk=10)` to keep memory flat.

### Streaming a large PDF section-by-section

```python
from py_chunks import stream_chunks

for chunk in stream_chunks("large.pdf", mode="section"):
    heading = chunk["metadata"].get("section_heading", "")
    print(f"[{heading}] {chunk['content'][:80]}")
```

### From bytes (API upload)

```python
from py_chunks import get_chunks_from_bytes, get_markdown

file_bytes = request.files['document'].read()
chunks = get_chunks_from_bytes(file_bytes, filename="report.pdf")

# Or get Markdown from bytes
md = get_markdown(file_bytes, filename="report.docx")
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

### FastAPI — Markdown conversion endpoint

```python
from fastapi import FastAPI, File, UploadFile
from py_chunks import get_markdown

app = FastAPI()

@app.post("/markdown/")
async def to_markdown(file: UploadFile = File(...)):
    data = await file.read()
    md = get_markdown(data, filename=file.filename)
    return {"markdown": md}
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
│   get_markdown()                             │
│   *_from_path / _from_bytes / _from_fileobj  │
│   *_from_upload / _from_s3_presigned_url     │
└──────────────┬───────────────────────────────┘
               │  source detection + temp-file management + cleanup
               ↓
┌──────────────────────────────────────────────┐
│            Format Dispatcher                 │
│        (py_chunks/chunkers/*.py)             │
│   chunk_pdf / chunk_docx / chunk_doc  /      │
│   chunk_pptx / chunk_md / chunk_html /       │
│   chunk_txt / chunk_xlsx (xlsx + xls) /      │
│   chunk_csv                           +      │
│   matching stream_chunk_* variants    +      │
│   *_to_markdown conversion wrappers          │
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
│    to_markdown.rs  — Markdown conversion function                │
│                                                                  │
│  PDF stream    — background thread owns PdfDocument; sends       │
│                  RawChunk through mpsc channel; __next__ recvs   │
│  MD/HTML/TXT   — block-by-block state machine for structural /   │
│                  semantic; batch-drain for the other 4 modes     │
│  DOCX stream   — DocxStructuralIterator (default/structural) +   │
│                  per-mode iterators for all other 5 modes        │
│  DOC           — cfb crate opens Compound Binary File;           │
│                  FIB → piece table (CLX) → paragraph props       │
│                  (PlcfBtePapx) → stylesheet (Stshf);             │
│                  DocStructuralIterator + per-mode iterators      │
│  PPTX stream   — batch-drain (ZIP must be read upfront)          │
│  XLSX/XLS      — open_workbook_auto() handles both formats;      │
│    row / sliding_window: state machine, one chunk per __next__   │
│    table / sheet / page_aware / semantic: batch-drain            │
│    table mode: ZIP XML for named tables (XLSX) or heuristic      │
│    page_aware: print-area XML (XLSX) or full-sheet fallback      │
│  CSV           — csv crate (not calamine); encoding_rs decoding  │
│    row / default / page_aware: background thread + mpsc channel  │
│    sliding_window: VecDeque rolling buffer, O(window_size) memory│
└──────────────────────────────────────────────────────────────────┘
```

### Design principles

- **Single responsibility** — each format has its own Rust submodule; modes never leak between formats
- **Framework-agnostic Python layer** — source detection (path / URL / bytes / file-like / upload) lives in `py_chunks/__init__.py`; the Rust layer only sees a file path
- **Temp-file strategy for bytes** — bytes / file-like / URL inputs are written to a `NamedTemporaryFile` (with the original extension), passed to Rust, then deleted; streaming variants wrap the iterator in `_StreamingFileCleanup` so the temp file is removed even on early exit
- **PDF streaming safety** — the background worker owns the `PdfDocument` for its full lifetime; chunks cross the thread boundary as plain `RawChunk` structs through `mpsc`, so no `unsafe` is needed
- **Streaming parity** — every streaming iterator yields the same chunks (and metadata) as the corresponding batch function
- **Pure Rust DOC parsing** — the `.doc` binary format is parsed entirely in Rust using the `cfb` crate with no external processes; text reconstruction, heading detection, and paragraph classification all happen at compile-time-checked byte offsets

---

## Error Handling

```python
from py_chunks import get_chunks, get_markdown

# File not found
try:
    chunks = get_chunks("missing.pdf")
except FileNotFoundError as e:
    print(e)   # File not found: missing.pdf

# Unsupported format
try:
    chunks = get_chunks("image.png")
except ValueError as e:
    print(e)   # Unsupported file type '.png'. Supported: .csv, .doc, .docx, .htm, .html, .md, .pdf, .ppt, .pptx, .txt, .xls, .xlsx

# Pre-Word 97 .doc file (not supported)
try:
    chunks = get_chunks("ancient.doc")
except RuntimeError as e:
    print(e)   # Pre-Word 97 .doc files are not supported. Convert to .docx first.

# Scanned / image-only PDF (no text layer)
try:
    chunks = get_chunks("scanned.pdf")
except RuntimeError as e:
    print(e)   # PDF appears to contain no extractable text
# ...but with list_images=True it succeeds and returns one image per page:
result = get_chunks("scanned.pdf", list_images=True)   # ChunksResult, no exception
print(len(result.images))   # one PNG per scanned page

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

# get_markdown with unsupported extension
try:
    md = get_markdown("image.png")
except ValueError as e:
    print(e)   # get_markdown does not support '.png'. Supported: ['.csv', '.doc', ...]
```

### Exceptions raised

| Exception | When |
|---|---|
| `FileNotFoundError` | A path was given but does not exist on disk. |
| `ValueError` | Unsupported extension, unknown mode, empty bytes, invalid `window_size` / `overlap` / `sentences_per_chunk` / `paragraphs_per_page`, missing `filename` for bytes / fileobj / URL inputs. |
| `TypeError` | Unsupported source type, or `upload_file.read()` returned a coroutine (async). Pass `upload_file.file` instead, or `await` it yourself. |
| `RuntimeError` | Rust-level failure (e.g. PDF with no extractable text, malformed DOCX/PPTX ZIP, pre-Word 97 `.doc` file, unreadable file). |
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

### Code quality

```bash
python -m pylint py_chunks
```

Expected: 10.00/10

---

## License

MIT

---

Built with Rust (performance) + Python (simplicity)
