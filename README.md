# py-chunks

> **Part of [chunk-engine](https://github.com/RanjanKudesia/chunk-engine)** — one Rust engine, three byte-identical SDKs ([py-chunks](https://pypi.org/project/py-chunks/) · [js-chunks](https://www.npmjs.com/package/js-chunks) · [rs-chunks](https://crates.io/crates/rs-chunks)).
> Full documentation, playground and benchmarks: **[chunkengine.dev](https://www.chunkengine.dev)**

[![PyPI](https://img.shields.io/pypi/v/py-chunks?style=flat-square&color=e8511e)](https://pypi.org/project/py-chunks/)
[![Python](https://img.shields.io/badge/python-3.9+-blue?style=flat-square)](https://www.python.org/downloads/)
[![License](https://img.shields.io/badge/license-MIT-green?style=flat-square)](LICENSE)

The **Python** binding for chunk-engine. Turn any of **36 document formats** into
typed, structure-aware chunks for RAG — parsing and chunking run in a compiled
Rust core, not a stack of Python dependencies.

## Install

```bash
pip install py-chunks
```

**Python 3.9+, and no runtime dependencies.** The Rust engine ships compiled
inside the wheel: it parses PDF itself, and vendors PDFium for the one job that
needs a rasteriser — rendering a scanned PDF's pages when the file carries no
embedded page image of its own.

## Quick start

```python
from py_chunks import get_chunks, stream_chunks, get_markdown

# Batch — works for every supported format
chunks = get_chunks("report.pdf", mode="semantic")

for chunk in chunks:
    print(chunk["content_type"])   # "heading", "table", "semantic", …
    print(chunk["content"])
    print(chunk["metadata"])       # format- and mode-specific

# Streaming — one chunk at a time
for chunk in stream_chunks("large.pdf", mode="section"):
    handle(chunk)

# Markdown conversion
md = get_markdown("report.docx")
```

Every chunk is a `dict` with `content`, `content_type`, and `metadata`.

📖 **[Chunking modes](https://www.chunkengine.dev/docs/chunking-modes)** ·
**[Supported formats](https://www.chunkengine.dev/docs/supported-formats)** ·
**[Output schema](https://www.chunkengine.dev/docs/output-schema)** ·
**[Metadata reference](https://www.chunkengine.dev/docs/metadata-reference)**

## Input sources

`get_chunks` / `stream_chunks` auto-detect the source:

| Source | Example |
|---|---|
| Path (`str` / `Path`) | `get_chunks("report.pdf")` |
| `bytes` / `bytearray` / `memoryview` | `get_chunks(data, filename="report.pdf")` |
| File-like (`BytesIO`, open file) | `get_chunks(BytesIO(data), filename="doc.md")` |
| FastAPI / Starlette `UploadFile` | `get_chunks(upload_file)` |
| HTTP(S) / S3 pre-signed URL | `get_chunks("https://bucket.s3…/f.pdf?sig=…")` |

Explicit helpers are also exported — `get_chunks_from_path`,
`get_chunks_from_bytes`, `get_chunks_from_fileobj`, `get_chunks_from_upload`,
`get_chunks_from_s3_presigned_url`, plus the matching `stream_chunks_from_*`.

> A `filename` is required for bytes and unnamed file objects — dispatch is by
> extension. `get_markdown` accepts paths, bytes, and file objects, but not URLs.

## Signatures

```python
get_chunks(
    source, *,
    filename: str | None = None,
    mode: str = "default",
    window_size: int = 3,           # sliding_window
    overlap: int = 1,               # sliding_window (must be < window_size)
    sentences_per_chunk: int = 3,   # sentence
    paragraphs_per_page: int = 15,  # page_aware
    list_images: bool = False,
) -> list[dict] | ChunksResult

stream_chunks(source, *, filename=None, mode="default", ...) -> Iterator[dict]

get_markdown(source, *, filename=None, list_images=False) -> str | MarkdownResult
```

## Images

```python
from py_chunks import get_chunks

result = get_chunks("report.docx", list_images=True)   # -> ChunksResult
result.chunks   # text chunks + content_type="image" chunks
result.images   # {"7fdc906103e95537.png": b"...", ...}  <16-hex content hash>.<ext>
```

`get_markdown(..., list_images=True)` returns a `MarkdownResult` with `.markdown`
(carrying `![](hash.ext)` refs) and `.images`. Not available on `stream_chunks`.

## Format-specific chunkers

For parameters the unified API doesn't expose (`rows_per_chunk`,
`max_chunk_chars`, `sheet_names`, `delimiter`, `encoding`) or for per-call
timing, import the format module directly. Each returns `(chunks, timing)` where
`timing` is `{"rust_ms": …, "python_ms": …}`:

```python
from py_chunks.chunkers.xlsx import chunk_xlsx, stream_chunk_xlsx
from py_chunks.chunkers.csv  import chunk_csv

chunks, timing = chunk_xlsx("data.xlsx", mode="row", rows_per_chunk=5)
chunks, timing = chunk_csv("data.csv", mode="row", delimiter="\t")
```

Modules: `pdf`, `docx`, `doc`, `pptx`, `ppt`, `html`, `md`, `txt`, `xlsx`
(the whole spreadsheet family), `csv`.

## Errors

| Exception | When |
|---|---|
| `FileNotFoundError` | Path doesn't exist |
| `ValueError` | Unsupported extension, invalid mode/parameter, missing `filename` |
| `TypeError` | Unsupported source type, or an async `upload.read()` |
| `RuntimeError` | Engine-level failure (e.g. a PDF with no text layer) |
| `NotImplementedError` | Unsupported streaming format/mode combination |

📖 **[Error handling](https://www.chunkengine.dev/docs/error-handling)** ·
**[FastAPI / Flask / Django / Litestar / aiohttp / Celery recipes](https://www.chunkengine.dev/docs/framework-integration/python)**

## Develop

```bash
pip install maturin
maturin develop --release    # rebuild the extension after any Rust change
python -m pytest -v
python -m pylint py_chunks   # expected: 10.00/10
```

## License

MIT
