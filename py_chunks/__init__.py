"""Public Python API for py_chunks.

This module exposes both format-specific chunkers and source-agnostic helpers
that accept paths, bytes, file-like objects, upload objects, and pre-signed
URLs.
"""

import os
import tempfile
from os import PathLike, fspath
from typing import Any
from urllib.parse import urlparse
from urllib.request import urlopen

from .chunkers.docx import chunk_docx
from .chunkers.html import chunk_html
from .chunkers.md import chunk_md
from .chunkers.pdf import chunk_pdf
from .chunkers.pptx import chunk_pptx
from .chunkers.txt import chunk_txt


_DISPATCH = {
    ".docx": chunk_docx,
    ".html": chunk_html,
    ".htm": chunk_html,
    ".md": chunk_md,
    ".pdf": chunk_pdf,
    ".pptx": chunk_pptx,
    ".txt": chunk_txt,
}

_SUPPORTED = ", ".join(sorted(_DISPATCH))


def _resolve_chunker(filename: str):
    ext = os.path.splitext(filename)[1].lower()
    chunker = _DISPATCH.get(ext)
    if chunker is None:
        raise ValueError(
            f"Unsupported file type '{ext}'. Supported: {_SUPPORTED}"
        )
    return chunker, ext


def get_chunks_from_path(file_path: str) -> list[dict]:
    """Chunk any supported document from a local path.

    Supported extensions: .docx, .htm, .html, .md, .pdf, .pptx, .txt

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

    chunker, _ = _resolve_chunker(file_path)

    chunks, _ = chunker(file_path)
    return chunks


def get_chunks_from_bytes(data: bytes, filename: str) -> list[dict]:
    """Chunk a document from raw bytes (e.g. an API file upload).

    Writes the bytes to a temporary file, runs the chunker, then deletes
    the temp file.  The original filename is only used for extension
    detection — it is never written to disk under that name.

    Supported extensions: .docx, .htm, .html, .md, .pdf, .pptx, .txt

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
        chunks, _ = chunker(tmp_path)
    finally:
        os.unlink(tmp_path)

    return chunks


def get_chunks_from_fileobj(file_obj: Any, filename: str | None = None) -> list[dict]:
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

    return get_chunks_from_bytes(data, inferred_name)


def get_chunks_from_upload(upload_file: Any) -> list[dict]:
    """Chunk from framework upload objects (e.g. FastAPI UploadFile)."""
    filename = getattr(upload_file, "filename", None)
    if not filename:
        raise ValueError("upload_file.filename is required")

    inner_file = getattr(upload_file, "file", None)
    if inner_file is not None and hasattr(inner_file, "read"):
        return get_chunks_from_fileobj(inner_file, filename=filename)

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
        return get_chunks_from_bytes(data, filename)

    raise TypeError("upload_file must provide .file.read() or .read()")


def get_chunks_from_s3_presigned_url(
    url: str, filename: str | None = None, timeout: int = 60
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

    return get_chunks_from_bytes(data, inferred_name)


def get_chunks(source: Any, *, filename: str | None = None) -> list[dict]:
    """Unified chunking entrypoint across paths, bytes, file objects, uploads, and URLs."""
    if isinstance(source, (str, PathLike)):
        source_path = fspath(source)
        parsed = urlparse(source_path)
        if parsed.scheme in {"http", "https"}:
            return get_chunks_from_s3_presigned_url(
                source_path, filename=filename
            )
        return get_chunks_from_path(source_path)

    if isinstance(source, memoryview):
        source = source.tobytes()

    if isinstance(source, bytearray):
        source = bytes(source)

    if isinstance(source, bytes):
        if not filename:
            raise ValueError("filename is required when source is bytes")
        return get_chunks_from_bytes(source, filename)

    if hasattr(source, "filename"):
        return get_chunks_from_upload(source)

    if hasattr(source, "read"):
        return get_chunks_from_fileobj(source, filename=filename)

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
    "chunk_docx",
    "chunk_html",
    "chunk_md",
    "chunk_pdf",
    "chunk_pptx",
    "chunk_txt",
]
