"""Public Python API for py_chunks.

This module exposes both format-specific chunkers and source-agnostic helpers
that accept paths, bytes, file-like objects, upload objects, and pre-signed
URLs.
"""

import os
import sys
import tempfile
from os import PathLike, fspath
from pathlib import Path
from typing import Any
from urllib.parse import urlparse
from urllib.request import urlopen

_pkg_dir = Path(__file__).parent

# Tell the Rust layer where to find the bundled PDFium binary.
os.environ.setdefault("PY_CHUNKS_PACKAGE_DIR", str(_pkg_dir))

# Directly resolve the bundled binary and set PDFIUM_LIBRARY_PATH to its
# absolute path.  This hits the highest-priority branch in the Rust resolver
# so no directory scanning is needed — the path is always exact.
_PDFIUM_NAMES = {
    "win32":  "pdfium.dll",
    "darwin": "libpdfium.dylib",
    "linux":  "libpdfium.so",
}
_pdfium_bin = _pkg_dir / _PDFIUM_NAMES.get(sys.platform, "")
if _pdfium_bin.exists():
    os.environ.setdefault("PDFIUM_LIBRARY_PATH", str(_pdfium_bin))

# On Windows, register the package directory as a DLL search directory so
# pdfium.dll's own dependencies (vcruntime140.dll, msvcp140.dll …) are found
# in py_chunks/ rather than failing with LoadLibrary error 126.
# os.add_dll_directory() wraps AddDllDirectory() — available on Python 3.8+,
# which we always satisfy (requires-python = ">=3.9").
if sys.platform == "win32" and hasattr(os, "add_dll_directory"):
    os.add_dll_directory(str(_pkg_dir))

from .chunkers.docx import chunk_docx, stream_chunk_docx
from .chunkers.csv import chunk_csv, stream_chunk_csv
from .chunkers.html import chunk_html, stream_chunk_html
from .chunkers.md import chunk_md, stream_chunk_md
from .chunkers.pdf import chunk_pdf, stream_chunk_pdf
from .chunkers.pptx import chunk_pptx, stream_chunk_pptx
from .chunkers.txt import chunk_txt, stream_chunk_txt
from .chunkers.xlsx import chunk_xlsx, stream_chunk_xlsx


_DISPATCH = {
    ".docx": chunk_docx,
    ".csv": chunk_csv,
    ".html": chunk_html,
    ".htm": chunk_html,
    ".md": chunk_md,
    ".pdf": chunk_pdf,
    ".pptx": chunk_pptx,
    ".txt": chunk_txt,
    ".xlsx": chunk_xlsx,
    ".xls": chunk_xlsx,
}

_EXT_DOCX = ".docx"
_EXT_CSV = ".csv"
_EXT_PDF = ".pdf"
_EXT_MD = ".md"
_EXT_TXT = ".txt"
_EXT_PPTX = ".pptx"
_EXT_HTML = ".html"
_EXT_HTM = ".htm"
_EXT_XLSX = ".xlsx"
_EXT_XLS = ".xls"

_SUPPORTED = ", ".join(sorted(_DISPATCH))


class _StreamingFileCleanup:
    """Iterator wrapper that guarantees temp-file cleanup."""

    def __init__(self, iterator, filepath: str):
        self._iterator = iterator
        self._filepath = filepath
        self._closed = False

    def __iter__(self):
        return self

    def __next__(self):
        if self._closed:
            raise StopIteration
        try:
            return next(self._iterator)
        except StopIteration:
            self.close()
            raise
        except Exception:
            self.close()
            raise

    def close(self) -> None:
        if self._closed:
            return
        self._closed = True
        try:
            if os.path.exists(self._filepath):
                os.unlink(self._filepath)
        except OSError:
            # Best-effort cleanup; iterator semantics should still complete.
            pass

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc, tb):
        self.close()
        return False

    def __del__(self):
        self.close()


def _resolve_chunker(filename: str):
    ext = os.path.splitext(filename)[1].lower()
    chunker = _DISPATCH.get(ext)
    if chunker is None:
        raise ValueError(
            f"Unsupported file type '{ext}'. Supported: {_SUPPORTED}"
        )
    return chunker, ext


def _run_chunker(
    chunker,
    file_path: str,
    mode: str,
    window_size: int,
    overlap: int,
    sentences_per_chunk: int,
    paragraphs_per_page: int,
):
    return chunker(
        file_path,
        mode=mode,
        window_size=window_size,
        overlap=overlap,
        sentences_per_chunk=sentences_per_chunk,
        paragraphs_per_page=paragraphs_per_page,
    )


def _xlsx_rows_per_chunk(sentences_per_chunk: int) -> int:
    return 1 if sentences_per_chunk == 3 else sentences_per_chunk


def _csv_rows_per_chunk(sentences_per_chunk: int) -> int:
    return max(1, sentences_per_chunk)


def get_chunks_from_path(
    file_path: str,
    *,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
    delimiter: str | None = None,
    encoding: str = "utf-8",
) -> list[dict]:
    """Chunk any supported document from a local path.

    Supported extensions: .csv, .docx, .htm, .html, .md, .pdf, .pptx, .txt, .xlsx

    Args:
        file_path: Path to the document file.

    Returns:
        List of chunk dicts, each with keys: content, content_type, metadata.

    Raises:
        FileNotFoundError: If the file does not exist.
        ValueError: If the file extension is not supported.
    """
    if not os.path.isfile(file_path):
        raise FileNotFoundError(f"File not found: {file_path}")

    chunker, ext = _resolve_chunker(file_path)

    if os.path.splitext(file_path)[1].lower() in (_EXT_XLSX, _EXT_XLS):
        chunks, _ = chunk_xlsx(
            file_path,
            mode="row" if mode == "default" else mode,
            rows_per_chunk=_xlsx_rows_per_chunk(sentences_per_chunk),
            window_size=window_size,
            overlap=overlap,
        )
        return chunks

    if ext == _EXT_CSV:
        csv_mode = "row" if mode == "default" else mode
        rows_per_chunk = (
            paragraphs_per_page if csv_mode == "page_aware" else _csv_rows_per_chunk(sentences_per_chunk)
        )
        chunks, _ = chunk_csv(
            file_path,
            mode=csv_mode,
            rows_per_chunk=rows_per_chunk,
            window_size=window_size,
            overlap=overlap,
            include_headers=True,
            delimiter=delimiter,
            encoding=encoding,
            skip_empty_rows=True,
        )
        return chunks

    chunks, _ = _run_chunker(
        chunker,
        file_path,
        mode,
        window_size,
        overlap,
        sentences_per_chunk,
        paragraphs_per_page,
    )
    return chunks


def get_chunks_from_bytes(
    data: bytes,
    filename: str,
    *,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
    delimiter: str | None = None,
    encoding: str = "utf-8",
) -> list[dict]:
    """Chunk a document from raw bytes (e.g. an API file upload).

    Writes the bytes to a temporary file, runs the chunker, then deletes
    the temp file.  The original filename is only used for extension
    detection — it is never written to disk under that name.

    Supported extensions: .docx, .htm, .html, .md, .pdf, .pptx, .txt, .xlsx

    Args:
        data:     Raw bytes of the document.
        filename: Original filename (e.g. ``"report.pdf"``). Used to
                  determine the file type.

    Returns:
        List of chunk dicts, each with keys: content, content_type, metadata.

    Raises:
        ValueError: If the file extension is not supported or data is empty.
    """
    if not data:
        raise ValueError("data is empty")

    chunker, ext = _resolve_chunker(filename)

    with tempfile.NamedTemporaryFile(suffix=ext, delete=False) as tmp:
        tmp.write(data)
        tmp_path = tmp.name

    try:
        if ext in (_EXT_XLSX, _EXT_XLS):
            chunks, _ = chunk_xlsx(
                tmp_path,
                mode="row" if mode == "default" else mode,
                rows_per_chunk=_xlsx_rows_per_chunk(sentences_per_chunk),
                window_size=window_size,
                overlap=overlap,
            )
            return chunks

        if ext == _EXT_CSV:
            csv_mode = "row" if mode == "default" else mode
            rows_per_chunk = (
                paragraphs_per_page if csv_mode == "page_aware" else _csv_rows_per_chunk(sentences_per_chunk)
            )
            chunks, _ = chunk_csv(
                tmp_path,
                mode=csv_mode,
                rows_per_chunk=rows_per_chunk,
                window_size=window_size,
                overlap=overlap,
                include_headers=True,
                delimiter=delimiter,
                encoding=encoding,
                skip_empty_rows=True,
            )
            return chunks

        chunks, _ = _run_chunker(
            chunker,
            tmp_path,
            mode,
            window_size,
            overlap,
            sentences_per_chunk,
            paragraphs_per_page,
        )
    finally:
        os.unlink(tmp_path)

    return chunks


def get_chunks_from_fileobj(
    file_obj: Any,
    filename: str | None = None,
    *,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
    delimiter: str | None = None,
    encoding: str = "utf-8",
) -> list[dict]:
    """Chunk from a file-like object (open file, BytesIO, spooled temp file)."""
    inferred_name = filename or getattr(file_obj, "name", None)
    if not inferred_name:
        raise ValueError("filename is required when file object has no name")

    data = file_obj.read()
    if isinstance(data, str):
        data = data.encode("utf-8")
    elif isinstance(data, bytearray):
        data = bytes(data)
    elif not isinstance(data, bytes):
        raise TypeError("file_obj.read() must return bytes or str")

    return get_chunks_from_bytes(
        data,
        inferred_name,
        mode=mode,
        window_size=window_size,
        overlap=overlap,
        sentences_per_chunk=sentences_per_chunk,
        paragraphs_per_page=paragraphs_per_page,
        delimiter=delimiter,
        encoding=encoding,
    )


def get_chunks_from_upload(
    upload_file: Any,
    *,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
    delimiter: str | None = None,
    encoding: str = "utf-8",
) -> list[dict]:
    """Chunk from framework upload objects (e.g. FastAPI UploadFile)."""
    filename = getattr(upload_file, "filename", None)
    if not filename:
        raise ValueError("upload_file.filename is required")

    inner_file = getattr(upload_file, "file", None)
    if inner_file is not None and hasattr(inner_file, "read"):
        return get_chunks_from_fileobj(
            inner_file,
            filename=filename,
            mode=mode,
            window_size=window_size,
            overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
            delimiter=delimiter,
            encoding=encoding,
        )

    if hasattr(upload_file, "read"):
        data = upload_file.read()
        if hasattr(data, "__await__"):
            raise TypeError(
                "upload_file.read() is async; pass upload_file.file or use bytes API"
            )
        if isinstance(data, str):
            data = data.encode("utf-8")
        elif isinstance(data, bytearray):
            data = bytes(data)
        elif not isinstance(data, bytes):
            raise TypeError("upload_file.read() must return bytes or str")
        return get_chunks_from_bytes(
            data,
            filename,
            mode=mode,
            window_size=window_size,
            overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
            delimiter=delimiter,
            encoding=encoding,
        )

    raise TypeError("upload_file must provide .file.read() or .read()")


def get_chunks_from_s3_presigned_url(
    url: str,
    filename: str | None = None,
    timeout: int = 60,
    *,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
    delimiter: str | None = None,
    encoding: str = "utf-8",
) -> list[dict]:
    """Download from a pre-signed URL and chunk the file."""
    inferred_name = filename
    if not inferred_name:
        path = urlparse(url).path
        inferred_name = path.rsplit("/", 1)[-1] if path else ""

    if not inferred_name:
        raise ValueError("filename is required when URL path has no filename")

    with urlopen(url, timeout=timeout) as response:
        data = response.read()

    return get_chunks_from_bytes(
        data,
        inferred_name,
        mode=mode,
        window_size=window_size,
        overlap=overlap,
        sentences_per_chunk=sentences_per_chunk,
        paragraphs_per_page=paragraphs_per_page,
        delimiter=delimiter,
        encoding=encoding,
    )


def stream_chunks_from_path(
    file_path: str,
    *,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
    delimiter: str | None = None,
    encoding: str = "utf-8",
) -> Any:
    """Stream chunks from any supported document at a local path.

    Supported extensions: .docx, .htm, .html, .md, .pdf, .pptx, .txt, .xlsx

    Args:
        file_path: Path to the document file.
        mode: Chunking mode (format-specific; see each format's chunker for details).

    Returns:
        Iterator that yields chunk dicts with keys: content, content_type, metadata.

    Raises:
        FileNotFoundError: If the file does not exist.
        ValueError: If the file extension is not supported or mode not available.
    """
    if not os.path.isfile(file_path):
        raise FileNotFoundError(f"File not found: {file_path}")

    _, ext = _resolve_chunker(file_path)

    if ext == _EXT_DOCX:
        return stream_chunk_docx(
            file_path,
            mode=mode,
            window_size=window_size,
            overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
        )
    if ext == _EXT_PDF:
        return stream_chunk_pdf(
            file_path,
            mode=mode,
            window_size=window_size,
            overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
        )
    if ext == _EXT_MD:
        return stream_chunk_md(
            file_path,
            mode=mode,
            window_size=window_size,
            overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
        )
    if ext == _EXT_TXT:
        return stream_chunk_txt(
            file_path,
            mode=mode,
            window_size=window_size,
            overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
        )
    if ext == _EXT_PPTX:
        return stream_chunk_pptx(
            file_path,
            mode=mode,
            window_size=window_size,
            overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
        )
    if ext in (_EXT_XLSX, _EXT_XLS):
        return stream_chunk_xlsx(
            file_path,
            mode="row" if mode == "default" else mode,
            rows_per_chunk=_xlsx_rows_per_chunk(sentences_per_chunk),
            window_size=window_size,
            overlap=overlap,
        )
    if ext == _EXT_CSV:
        csv_mode = "row" if mode == "default" else mode
        rows_per_chunk = (
            paragraphs_per_page if csv_mode == "page_aware" else _csv_rows_per_chunk(sentences_per_chunk)
        )
        return stream_chunk_csv(
            file_path,
            mode=csv_mode,
            rows_per_chunk=rows_per_chunk,
            window_size=window_size,
            overlap=overlap,
            include_headers=True,
            delimiter=delimiter,
            encoding=encoding,
            skip_empty_rows=True,
        )
    if ext in (_EXT_HTML, _EXT_HTM):
        return stream_chunk_html(
            file_path,
            mode=mode,
            window_size=window_size,
            overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
        )

    raise NotImplementedError(f"Streaming not yet supported for {ext} files")


def stream_chunks_from_bytes(
    data: bytes,
    filename: str,
    *,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
    delimiter: str | None = None,
    encoding: str = "utf-8",
) -> Any:
    """Stream chunks from raw bytes (e.g. an API file upload).

    Writes the bytes to a temporary file, creates a streaming iterator,
    then deletes the temp file. The original filename is only used for
    extension detection — it is never written to disk under that name.

    Supported extensions: .docx, .htm, .html, .md, .pdf, .pptx, .txt, .xlsx

    Args:
        data:     Raw bytes of the document.
        filename: Original filename (e.g. ``"report.pdf"``). Used to
                  determine the file type.
        mode: Chunking mode (format-specific; see each format's chunker for details).

    Returns:
        Iterator that yields chunk dicts with keys: content, content_type, metadata.

    Raises:
        ValueError: If the file extension is not supported, data is empty, or mode not available.
    """
    if not data:
        raise ValueError("data is empty")

    _, ext = _resolve_chunker(filename)

    with tempfile.NamedTemporaryFile(suffix=ext, delete=False) as tmp:
        tmp.write(data)
        tmp_path = tmp.name

    if ext == _EXT_DOCX:
        iterator = stream_chunk_docx(
            tmp_path,
            mode=mode,
            window_size=window_size,
            overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
        )
        return _StreamingFileCleanup(iterator, tmp_path)
    if ext == _EXT_PDF:
        iterator = stream_chunk_pdf(
            tmp_path,
            mode=mode,
            window_size=window_size,
            overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
        )
        return _StreamingFileCleanup(iterator, tmp_path)
    if ext == _EXT_MD:
        iterator = stream_chunk_md(
            tmp_path,
            mode=mode,
            window_size=window_size,
            overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
        )
        return _StreamingFileCleanup(iterator, tmp_path)
    if ext == _EXT_TXT:
        iterator = stream_chunk_txt(
            tmp_path,
            mode=mode,
            window_size=window_size,
            overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
        )
        return _StreamingFileCleanup(iterator, tmp_path)
    if ext == _EXT_PPTX:
        iterator = stream_chunk_pptx(
            tmp_path,
            mode=mode,
            window_size=window_size,
            overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
        )
        return _StreamingFileCleanup(iterator, tmp_path)
    if ext in (_EXT_XLSX, _EXT_XLS):
        iterator = stream_chunk_xlsx(
            tmp_path,
            mode="row" if mode == "default" else mode,
            rows_per_chunk=_xlsx_rows_per_chunk(sentences_per_chunk),
            window_size=window_size,
            overlap=overlap,
        )
        return _StreamingFileCleanup(iterator, tmp_path)
    if ext == _EXT_CSV:
        csv_mode = "row" if mode == "default" else mode
        rows_per_chunk = (
            paragraphs_per_page if csv_mode == "page_aware" else _csv_rows_per_chunk(sentences_per_chunk)
        )
        iterator = stream_chunk_csv(
            tmp_path,
            mode=csv_mode,
            rows_per_chunk=rows_per_chunk,
            window_size=window_size,
            overlap=overlap,
            include_headers=True,
            delimiter=delimiter,
            encoding=encoding,
            skip_empty_rows=True,
        )
        return _StreamingFileCleanup(iterator, tmp_path)
    if ext in (_EXT_HTML, _EXT_HTM):
        iterator = stream_chunk_html(
            tmp_path,
            mode=mode,
            window_size=window_size,
            overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
        )
        return _StreamingFileCleanup(iterator, tmp_path)

    raise NotImplementedError(f"Streaming not yet supported for {ext} files")


def stream_chunks_from_fileobj(
    file_obj: Any,
    filename: str | None = None,
    *,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
    delimiter: str | None = None,
    encoding: str = "utf-8",
) -> Any:
    """Stream chunks from a file-like object (open file, BytesIO, etc.)."""
    inferred_name = filename or getattr(file_obj, "name", None)
    if not inferred_name:
        raise ValueError("filename is required when file object has no name")

    data = file_obj.read()
    if isinstance(data, str):
        data = data.encode("utf-8")
    elif isinstance(data, bytearray):
        data = bytes(data)
    elif not isinstance(data, bytes):
        raise TypeError("file_obj.read() must return bytes or str")

    return stream_chunks_from_bytes(
        data,
        inferred_name,
        mode=mode,
        window_size=window_size,
        overlap=overlap,
        sentences_per_chunk=sentences_per_chunk,
        paragraphs_per_page=paragraphs_per_page,
        delimiter=delimiter,
        encoding=encoding,
    )


def stream_chunks_from_upload(
    upload_file: Any,
    *,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
    delimiter: str | None = None,
    encoding: str = "utf-8",
) -> Any:
    """Stream chunks from framework upload objects (e.g. FastAPI UploadFile)."""
    filename = getattr(upload_file, "filename", None)
    if not filename:
        raise ValueError("upload_file.filename is required")

    inner_file = getattr(upload_file, "file", None)
    if inner_file is not None and hasattr(inner_file, "read"):
        return stream_chunks_from_fileobj(
            inner_file,
            filename=filename,
            mode=mode,
            window_size=window_size,
            overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
            delimiter=delimiter,
            encoding=encoding,
        )

    if hasattr(upload_file, "read"):
        data = upload_file.read()
        if hasattr(data, "__await__"):
            raise TypeError(
                "upload_file.read() is async; pass upload_file.file or use bytes API"
            )
        if isinstance(data, str):
            data = data.encode("utf-8")
        elif isinstance(data, bytearray):
            data = bytes(data)
        elif not isinstance(data, bytes):
            raise TypeError("upload_file.read() must return bytes or str")
        return stream_chunks_from_bytes(
            data,
            filename,
            mode=mode,
            window_size=window_size,
            overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
            delimiter=delimiter,
            encoding=encoding,
        )

    raise TypeError("upload_file must provide .file.read() or .read()")


def stream_chunks_from_s3_presigned_url(
    url: str,
    filename: str | None = None,
    timeout: int = 60,
    *,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
    delimiter: str | None = None,
    encoding: str = "utf-8",
) -> Any:
    """Stream chunks from a document downloaded via pre-signed URL."""
    inferred_name = filename
    if not inferred_name:
        path = urlparse(url).path
        inferred_name = path.rsplit("/", 1)[-1] if path else ""

    if not inferred_name:
        raise ValueError("filename is required when URL path has no filename")

    with urlopen(url, timeout=timeout) as response:
        data = response.read()

    return stream_chunks_from_bytes(
        data,
        inferred_name,
        mode=mode,
        window_size=window_size,
        overlap=overlap,
        sentences_per_chunk=sentences_per_chunk,
        paragraphs_per_page=paragraphs_per_page,
        delimiter=delimiter,
        encoding=encoding,
    )


def stream_chunks(
    source: Any,
    *,
    filename: str | None = None,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
    delimiter: str | None = None,
    encoding: str = "utf-8",
) -> Any:
    """Unified streaming chunking entrypoint across paths, bytes, file objects, uploads, and URLs.

    Returns an iterator that yields chunks one at a time without buffering
    the entire result list in memory. Useful for large documents.

    Supports streaming for all formats: .docx, .htm, .html, .md, .pdf, .pptx, .txt, .xlsx

    Args:
        source: File path, URL, bytes, file-like object, or upload object.
        filename: Original filename (required for bytes/fileobj/upload sources).
        mode: Chunking mode. DOCX supports "default" and "structural"
            (equivalent behavior); PDF supports all current PDF modes.

    Returns:
        Iterator that yields chunk dicts with keys: content, content_type, metadata.

    Raises:
        FileNotFoundError: If the file path does not exist.
        ValueError: If source type is invalid, filename is missing when required, or mode is unavailable.
        TypeError: If source type is unsupported.
        NotImplementedError: If the requested mode/format combination is not yet implemented for streaming.
    """
    if isinstance(source, (str, PathLike)):
        source_path = fspath(source)
        parsed = urlparse(source_path)
        if parsed.scheme in {"http", "https"}:
            return stream_chunks_from_s3_presigned_url(
                source_path,
                filename=filename,
                mode=mode,
                window_size=window_size,
                overlap=overlap,
                sentences_per_chunk=sentences_per_chunk,
                paragraphs_per_page=paragraphs_per_page,
                delimiter=delimiter,
                encoding=encoding,
            )
        return stream_chunks_from_path(
            source_path,
            mode=mode,
            window_size=window_size,
            overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
            delimiter=delimiter,
            encoding=encoding,
        )

    if isinstance(source, memoryview):
        source = source.tobytes()

    if isinstance(source, bytearray):
        source = bytes(source)

    if isinstance(source, bytes):
        if not filename:
            raise ValueError("filename is required when source is bytes")
        return stream_chunks_from_bytes(
            source,
            filename,
            mode=mode,
            window_size=window_size,
            overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
            delimiter=delimiter,
            encoding=encoding,
        )

    if hasattr(source, "filename"):
        return stream_chunks_from_upload(
            source,
            mode=mode,
            window_size=window_size,
            overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
            delimiter=delimiter,
            encoding=encoding,
        )

    if hasattr(source, "read"):
        return stream_chunks_from_fileobj(
            source,
            filename=filename,
            mode=mode,
            window_size=window_size,
            overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
            delimiter=delimiter,
            encoding=encoding,
        )

    raise TypeError(
        "Unsupported source type. Use path/URL, bytes, file-like object, or upload object."
    )


def get_chunks(
    source: Any,
    *,
    filename: str | None = None,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
    delimiter: str | None = None,
    encoding: str = "utf-8",
) -> list[dict]:
    """Unified chunking entrypoint across paths, bytes, file objects, uploads, and URLs."""
    if isinstance(source, (str, PathLike)):
        source_path = fspath(source)
        parsed = urlparse(source_path)
        if parsed.scheme in {"http", "https"}:
            return get_chunks_from_s3_presigned_url(
                source_path,
                filename=filename,
                mode=mode,
                window_size=window_size,
                overlap=overlap,
                sentences_per_chunk=sentences_per_chunk,
                paragraphs_per_page=paragraphs_per_page,
                delimiter=delimiter,
                encoding=encoding,
            )
        return get_chunks_from_path(
            source_path,
            mode=mode,
            window_size=window_size,
            overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
            delimiter=delimiter,
            encoding=encoding,
        )

    if isinstance(source, memoryview):
        source = source.tobytes()

    if isinstance(source, bytearray):
        source = bytes(source)

    if isinstance(source, bytes):
        if not filename:
            raise ValueError("filename is required when source is bytes")
        return get_chunks_from_bytes(
            source,
            filename,
            mode=mode,
            window_size=window_size,
            overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
            delimiter=delimiter,
            encoding=encoding,
        )

    if hasattr(source, "filename"):
        return get_chunks_from_upload(
            source,
            mode=mode,
            window_size=window_size,
            overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
            delimiter=delimiter,
            encoding=encoding,
        )

    if hasattr(source, "read"):
        return get_chunks_from_fileobj(
            source,
            filename=filename,
            mode=mode,
            window_size=window_size,
            overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
            delimiter=delimiter,
            encoding=encoding,
        )

    raise TypeError(
        "Unsupported source type. Use path/URL, bytes, file-like object, or upload object."
    )


__all__ = [
    "get_chunks_from_path",
    "get_chunks_from_fileobj",
    "get_chunks_from_upload",
    "get_chunks_from_s3_presigned_url",
    "get_chunks",
    "get_chunks_from_bytes",
    "stream_chunks_from_path",
    "stream_chunks_from_fileobj",
    "stream_chunks_from_upload",
    "stream_chunks_from_s3_presigned_url",
    "stream_chunks",
    "chunk_docx",
    "stream_chunk_docx",
    "chunk_csv",
    "stream_chunk_csv",
    "chunk_html",
    "stream_chunk_html",
    "chunk_md",
    "stream_chunk_md",
    "chunk_pdf",
    "stream_chunk_pdf",
    "chunk_pptx",
    "stream_chunk_pptx",
    "chunk_txt",
    "stream_chunk_txt",
    "chunk_xlsx",
    "stream_chunk_xlsx",
]
