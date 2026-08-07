"""Source-agnostic entry points: paths, bytes, file objects, uploads, URLs.

Batch (`get_chunks*`), streaming (`stream_chunks*`) and markdown
(`get_markdown`) helpers over the format routing tables in ``_dispatch``.
Bytes-shaped sources go straight to the engine's no-filesystem API
(`py_chunks._rust.get_chunks_from_bytes` and friends); only the streaming
bytes path still materialises a temp file, because the engine's streaming
surface is path-based.
"""

import os
import tempfile
from dataclasses import dataclass, field
from os import PathLike, fspath
from pathlib import Path
from typing import Any, Literal, overload
from urllib.parse import urlparse
from urllib.request import urlopen

from . import _rust
from ._dispatch import (
    _CHUNKS_IMAGE_DISPATCH,
    _DELIMITED_EXTS,
    _MD_DISPATCH,
    _MD_IMAGE_DISPATCH,
    _chunk_file,
    _csv_bytes_args,
    _open_stream,
    _resolve_chunker,
    _resolve_markdown_dispatch_ext,
    _validate_bytes_options,
)


@dataclass
class MarkdownResult:
    """Return type of get_markdown() when list_images=True."""

    markdown: str
    images: dict[str, bytes] = field(default_factory=dict)


@dataclass
class ChunksResult:
    """Return type of get_chunks() when list_images=True."""

    chunks: list[dict]
    images: dict[str, bytes] = field(default_factory=dict)


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


def _read_source_bytes(file_obj: Any, what: str) -> bytes:
    """`.read()` a file-like object and normalise the result to bytes."""
    data = file_obj.read()
    if isinstance(data, str):
        return data.encode("utf-8")
    if isinstance(data, bytearray):
        return bytes(data)
    if not isinstance(data, bytes):
        raise TypeError(f"{what}.read() must return bytes or str")
    return data


def _url_filename(url: str, filename: "str | None") -> str:
    """The explicit filename, or the last URL path segment."""
    inferred_name = filename
    if not inferred_name:
        path = urlparse(url).path
        inferred_name = path.rsplit("/", 1)[-1] if path else ""
    if not inferred_name:
        raise ValueError("filename is required when URL path has no filename")
    return inferred_name


# ── Batch entry points ───────────────────────────────────────────────────────


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
    list_images: bool = False,
) -> "list[dict] | ChunksResult":
    """Chunk any supported document from a local path.

    Supported extensions: all 36 supported formats (``sorted(_DISPATCH)`` in
    ``py_chunks._dispatch`` is the definitive list).

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

    _, ext = _resolve_chunker(file_path)

    if list_images:
        if ext in _CHUNKS_IMAGE_DISPATCH:
            chunk_list, images = _CHUNKS_IMAGE_DISPATCH[ext](
                file_path, mode=mode, window_size=window_size,
                overlap=overlap, sentences_per_chunk=sentences_per_chunk,
                paragraphs_per_page=paragraphs_per_page,
            )
            return ChunksResult(chunks=chunk_list, images=images)
        # Format without image extraction — silent fallback, no images.
        chunks = _chunk_file(
            ext, file_path, mode=mode, window_size=window_size, overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
            delimiter=delimiter, encoding=encoding,
        )
        return ChunksResult(chunks=chunks, images={})

    return _chunk_file(
        ext, file_path, mode=mode, window_size=window_size, overlap=overlap,
        sentences_per_chunk=sentences_per_chunk,
        paragraphs_per_page=paragraphs_per_page,
        delimiter=delimiter, encoding=encoding,
    )


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
    list_images: bool = False,
) -> "list[dict] | ChunksResult":
    """Chunk a document from raw bytes (e.g. an API file upload).

    The bytes go straight to the engine's no-filesystem API — nothing is
    written to disk. The filename is only used for extension detection.

    Supported extensions: all 36 supported formats (same set as
    ``get_chunks_from_path``).

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

    _, ext = _resolve_chunker(filename)

    if ext in _DELIMITED_EXTS:
        # csv/tsv keep the caller's delimiter/encoding overrides, so they get
        # the format's own bytes entry point.
        csv_mode, rows_per_chunk, delim = _csv_bytes_args(
            ext, mode, window_size, overlap, sentences_per_chunk,
            paragraphs_per_page, delimiter, encoding,
        )
        result = _rust.chunk_csv_from_bytes(
            data, csv_mode, rows_per_chunk, window_size, overlap,
            True, delim, encoding, True,
        )
        chunks = result["chunks"]
        if list_images:
            return ChunksResult(chunks=chunks, images={})
        return chunks

    _validate_bytes_options(
        ext, mode, window_size, overlap, sentences_per_chunk, paragraphs_per_page
    )

    if list_images and ext in _CHUNKS_IMAGE_DISPATCH:
        chunk_list, image_list = _rust.get_chunks_with_images_from_bytes(
            data, filename, mode, window_size,
            overlap, sentences_per_chunk, paragraphs_per_page,
        )
        return ChunksResult(chunks=chunk_list, images=dict(image_list))

    result = _rust.get_chunks_from_bytes(
        data, filename, mode, window_size,
        overlap, sentences_per_chunk, paragraphs_per_page,
    )
    if list_images:
        return ChunksResult(chunks=result["chunks"], images={})
    return result["chunks"]


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
    list_images: bool = False,
) -> "list[dict] | ChunksResult":
    """Chunk from a file-like object (open file, BytesIO, spooled temp file)."""
    inferred_name = filename or getattr(file_obj, "name", None)
    if not inferred_name:
        raise ValueError("filename is required when file object has no name")

    data = _read_source_bytes(file_obj, "file_obj")

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
        list_images=list_images,
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
    list_images: bool = False,
) -> "list[dict] | ChunksResult":
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
            list_images=list_images,
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
            list_images=list_images,
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
    list_images: bool = False,
) -> "list[dict] | ChunksResult":
    """Download from a pre-signed URL and chunk the file."""
    inferred_name = _url_filename(url, filename)

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
        list_images=list_images,
    )


# ── Streaming entry points ───────────────────────────────────────────────────


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

    Supports streaming for all 36 supported formats (same extension set as
    ``get_chunks_from_path``).

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

    return _open_stream(
        ext, file_path, mode=mode, window_size=window_size, overlap=overlap,
        sentences_per_chunk=sentences_per_chunk,
        paragraphs_per_page=paragraphs_per_page,
        delimiter=delimiter, encoding=encoding,
    )


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

    Writes the bytes to a temporary file (the engine's streaming surface is
    path-based), creates the iterator, and deletes the temp file when the
    iterator is exhausted or closed. The original filename is only used for
    extension detection — it is never written to disk under that name.

    Supports streaming for all 36 supported formats (same extension set as
    ``get_chunks_from_path``).

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

    try:
        iterator = _open_stream(
            ext, tmp_path, mode=mode, window_size=window_size, overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
            delimiter=delimiter, encoding=encoding,
        )
    except Exception:
        # Constructor-time failure (bad mode, unparseable file): don't leak
        # the temp file.
        os.unlink(tmp_path)
        raise
    return _StreamingFileCleanup(iterator, tmp_path)


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

    data = _read_source_bytes(file_obj, "file_obj")

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
    inferred_name = _url_filename(url, filename)

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


# ── Unified entry points ─────────────────────────────────────────────────────


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

    Supports streaming for all 36 supported formats (same extension set as
    ``get_chunks``).

    Args:
        source: File path, URL, bytes, file-like object, or upload object.
        filename: Original filename (required for bytes/fileobj/upload sources).
        mode: Chunking mode (format-specific; see each format's chunker for
            details).

    Returns:
        Iterator that yields chunk dicts with keys: content, content_type, metadata.

    Raises:
        FileNotFoundError: If the file path does not exist.
        ValueError: If source type is invalid, filename is missing when
            required, or mode is unavailable.
        TypeError: If source type is unsupported.
        NotImplementedError: If the requested mode/format combination is not
            yet implemented for streaming.
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


@overload
def get_chunks(source, *, filename: str | None = ..., mode: str = ...,
               window_size: int = ..., overlap: int = ...,
               sentences_per_chunk: int = ..., paragraphs_per_page: int = ...,
               delimiter: str | None = ..., encoding: str = ...,
               list_images: Literal[False] = ...) -> list[dict]: ...


@overload
def get_chunks(source, *, filename: str | None = ..., mode: str = ...,
               window_size: int = ..., overlap: int = ...,
               sentences_per_chunk: int = ..., paragraphs_per_page: int = ...,
               delimiter: str | None = ..., encoding: str = ...,
               list_images: Literal[True]) -> ChunksResult: ...


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
    list_images: bool = False,
) -> "list[dict] | ChunksResult":
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
                list_images=list_images,
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
            list_images=list_images,
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
            list_images=list_images,
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
            list_images=list_images,
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
            list_images=list_images,
        )

    raise TypeError(
        "Unsupported source type. Use path/URL, bytes, file-like object, or upload object."
    )


# ── Markdown ─────────────────────────────────────────────────────────────────


def _get_markdown_from_bytes_data(data: bytes, ext: str, list_images: bool = False):
    """Markdown conversion straight from bytes via the engine's no-filesystem
    dispatch — no temp file. The synthetic filename only carries the extension."""
    filename = f"data{ext}"
    if list_images and ext in _MD_IMAGE_DISPATCH:
        md, image_list = _rust.get_markdown_with_images_from_bytes(data, filename)
        return MarkdownResult(markdown=md, images=dict(image_list))
    md = _rust.get_markdown_from_bytes(data, filename)
    if list_images:
        return MarkdownResult(markdown=md, images={})
    return md


@overload
def get_markdown(source, *, filename: str | None = ...,
                 list_images: Literal[False] = ...) -> str: ...


@overload
def get_markdown(source, *, filename: str | None = ...,
                 list_images: Literal[True]) -> MarkdownResult: ...


def get_markdown(
    source,
    *,
    filename: str | None = None,
    list_images: bool = False,
) -> "str | MarkdownResult":
    """Convert a document to a Markdown string.

    Accepts the same source types as get_chunks(): file path, bytes,
    bytearray, memoryview, or file-like object.

    Args:
        list_images: If True, return a MarkdownResult with extracted image bytes.
                     If False (default), return a plain str (existing behaviour).
    """
    if isinstance(source, (str, Path)):
        path = Path(source)
        if not path.is_file():
            raise FileNotFoundError(f"File not found: {source}")
        ext = _resolve_markdown_dispatch_ext(path.suffix.lower())
        if list_images and ext in _MD_IMAGE_DISPATCH:
            md, images = _MD_IMAGE_DISPATCH[ext](str(path))
            return MarkdownResult(markdown=md, images=images)
        md = _MD_DISPATCH[ext](str(path))
        if list_images:
            return MarkdownResult(markdown=md, images={})
        return md

    # bytes / bytearray / memoryview
    if isinstance(source, (bytes, bytearray, memoryview)):
        if not filename:
            raise ValueError("filename is required when source is bytes")
        ext = _resolve_markdown_dispatch_ext(Path(filename).suffix.lower())
        return _get_markdown_from_bytes_data(bytes(source), ext, list_images)

    # File-like object
    if hasattr(source, "read"):
        name = filename or getattr(source, "name", None)
        if not name:
            raise ValueError(
                "filename is required for file-like objects without a .name")
        ext = _resolve_markdown_dispatch_ext(Path(name).suffix.lower())
        data = source.read()
        return _get_markdown_from_bytes_data(
            data if isinstance(data, bytes) else data.encode(),
            ext,
            list_images,
        )

    raise TypeError(f"Unsupported source type: {type(source).__name__}")
