"""Public Python API for py_chunks.

This module exposes both format-specific chunkers and source-agnostic helpers
that accept paths, bytes, file-like objects, upload objects, and pre-signed
URLs.
"""

from .chunkers.xlsx import (
    chunk_xlsx,
    chunk_xlsx_with_images as _chunk_xlsx_with_images,
    stream_chunk_xlsx,
    xlsx_to_markdown as _xlsx_to_markdown,
    xlsx_to_markdown_with_images as _xlsx_to_markdown_with_images,
)
from .chunkers.txt import chunk_txt, stream_chunk_txt, txt_to_markdown as _txt_to_markdown
from .chunkers.pptx import chunk_pptx, chunk_pptx_with_images as _chunk_pptx_with_images, pptx_to_markdown as _pptx_to_markdown, pptx_to_markdown_with_images as _pptx_to_markdown_with_images, stream_chunk_pptx
from .chunkers.pdf import (
    chunk_pdf,
    chunk_pdf_with_images as _chunk_pdf_with_images,
    pdf_to_markdown as _pdf_to_markdown,
    pdf_to_markdown_with_images as _pdf_to_markdown_with_images,
    stream_chunk_pdf,
)
from .chunkers.ppt import chunk_ppt, chunk_ppt_with_images as _chunk_ppt_with_images, ppt_to_markdown as _ppt_to_markdown, ppt_to_markdown_with_images as _ppt_to_markdown_with_images, stream_chunk_ppt
from .chunkers.md import chunk_md, md_to_markdown as _md_to_markdown, stream_chunk_md
from .chunkers.html import (
    chunk_html,
    stream_chunk_html,
    html_to_markdown as _html_to_markdown,
    chunk_html_with_images as _chunk_html_with_images,
    html_to_markdown_with_images as _html_to_markdown_with_images,
)
from .chunkers.csv import chunk_csv, csv_to_markdown as _csv_to_markdown, stream_chunk_csv
from .chunkers.tsv import chunk_tsv, tsv_to_markdown as _tsv_to_markdown, stream_chunk_tsv
from .chunkers.msg import (
    chunk_msg,
    chunk_msg_with_images as _chunk_msg_with_images,
    msg_to_markdown as _msg_to_markdown,
    msg_to_markdown_with_images as _msg_to_markdown_with_images,
    stream_chunk_msg,
)
from .chunkers.eml import (
    chunk_eml,
    chunk_eml_with_images as _chunk_eml_with_images,
    eml_to_markdown as _eml_to_markdown,
    eml_to_markdown_with_images as _eml_to_markdown_with_images,
    stream_chunk_eml,
)
from .chunkers.odf import (
    chunk_odf,
    chunk_odf_with_images as _chunk_odf_with_images,
    odf_to_markdown as _odf_to_markdown,
    odf_to_markdown_with_images as _odf_to_markdown_with_images,
    stream_chunk_odf,
)
from .chunkers.json import (
    chunk_json,
    json_to_markdown as _json_to_markdown,
    stream_chunk_json,
)
from .chunkers.rtf import chunk_rtf, rtf_to_markdown as _rtf_to_markdown, stream_chunk_rtf
from .chunkers.epub import (
    chunk_epub,
    chunk_epub_with_images as _chunk_epub_with_images,
    epub_to_markdown as _epub_to_markdown,
    epub_to_markdown_with_images as _epub_to_markdown_with_images,
    stream_chunk_epub,
)
from .chunkers.ipynb import (
    chunk_ipynb,
    chunk_ipynb_with_images as _chunk_ipynb_with_images,
    ipynb_to_markdown as _ipynb_to_markdown,
    ipynb_to_markdown_with_images as _ipynb_to_markdown_with_images,
    stream_chunk_ipynb,
)
from .chunkers.doc import chunk_doc, chunk_doc_with_images as _chunk_doc_with_images, doc_to_markdown as _doc_to_markdown, doc_to_markdown_with_images as _doc_to_markdown_with_images, stream_chunk_doc
from .chunkers.docx import chunk_docx, chunk_docx_with_images as _chunk_docx_with_images, docx_to_markdown as _docx_to_markdown, docx_to_markdown_with_images as _docx_to_markdown_with_images, stream_chunk_docx
import os
import sys
import tempfile
from dataclasses import dataclass, field
from os import PathLike, fspath
from pathlib import Path
from typing import Any, Literal, overload
from urllib.parse import urlparse
from urllib.request import urlopen

_pkg_dir = Path(__file__).parent

os.environ.setdefault("PY_CHUNKS_PACKAGE_DIR", str(_pkg_dir))

# PDF parsing is handled by the `liteparse` crate, which vendors its own PDFium
# binding — no external PDFium binary resolution is required here anymore.


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


_DISPATCH = {
    ".doc": chunk_doc,
    ".docx": chunk_docx,
    ".docm": chunk_docx,
    ".dotx": chunk_docx,
    ".dotm": chunk_docx,
    ".csv": chunk_csv,
    ".tsv": chunk_tsv,
    ".msg": chunk_msg,
    ".eml": chunk_eml,
    ".mbox": chunk_eml,
    ".odt": chunk_odf,
    ".odp": chunk_odf,
    ".json": chunk_json,
    ".jsonl": chunk_json,
    ".ndjson": chunk_json,
    ".rtf": chunk_rtf,
    ".epub": chunk_epub,
    ".ipynb": chunk_ipynb,
    ".html": chunk_html,
    ".htm": chunk_html,
    ".md": chunk_md,
    ".pdf": chunk_pdf,
    ".ppt": chunk_ppt,
    ".pptx": chunk_pptx,
    ".potx": chunk_pptx,
    ".potm": chunk_pptx,
    ".ppsx": chunk_pptx,
    ".ppsm": chunk_pptx,
    ".txt": chunk_txt,
    ".xlsx": chunk_xlsx,
    ".xls": chunk_xlsx,
    ".xlsm": chunk_xlsx,
    ".xlsb": chunk_xlsx,
    ".ods": chunk_xlsx,
    ".xltx": chunk_xlsx,
    ".xltm": chunk_xlsx,
}

_MD_DISPATCH = {
    ".doc":  _doc_to_markdown,
    ".docx": _docx_to_markdown,
    ".docm": _docx_to_markdown,
    ".dotx": _docx_to_markdown,
    ".dotm": _docx_to_markdown,
    ".ppt":  _ppt_to_markdown,
    ".pptx": _pptx_to_markdown,
    ".potx": _pptx_to_markdown,
    ".potm": _pptx_to_markdown,
    ".ppsx": _pptx_to_markdown,
    ".ppsm": _pptx_to_markdown,
    ".pdf":  _pdf_to_markdown,
    ".html": _html_to_markdown,
    ".htm":  _html_to_markdown,
    ".xlsx": _xlsx_to_markdown,
    ".xls":  _xlsx_to_markdown,
    ".xlsm": _xlsx_to_markdown,
    ".xlsb": _xlsx_to_markdown,
    ".ods":  _xlsx_to_markdown,
    ".xltx": _xlsx_to_markdown,
    ".xltm": _xlsx_to_markdown,
    ".txt":  _txt_to_markdown,
    ".md":   _md_to_markdown,
    ".csv":  _csv_to_markdown,
    ".tsv":  _tsv_to_markdown,
    ".msg":  _msg_to_markdown,
    ".eml":  _eml_to_markdown,
    ".mbox": _eml_to_markdown,
    ".odt":  _odf_to_markdown,
    ".odp":  _odf_to_markdown,
    ".json": _json_to_markdown,
    ".jsonl": _json_to_markdown,
    ".ndjson": _json_to_markdown,
    ".rtf":  _rtf_to_markdown,
    ".epub": _epub_to_markdown,
    ".ipynb": _ipynb_to_markdown,
}

_MD_IMAGE_DISPATCH: dict[str, Any] = {
    ".doc":  _doc_to_markdown_with_images,
    ".docx": _docx_to_markdown_with_images,
    ".docm": _docx_to_markdown_with_images,
    ".dotx": _docx_to_markdown_with_images,
    ".dotm": _docx_to_markdown_with_images,
    ".ppt":  _ppt_to_markdown_with_images,
    ".pptx": _pptx_to_markdown_with_images,
    ".potx": _pptx_to_markdown_with_images,
    ".potm": _pptx_to_markdown_with_images,
    ".ppsx": _pptx_to_markdown_with_images,
    ".ppsm": _pptx_to_markdown_with_images,
    ".xlsx": _xlsx_to_markdown_with_images,
    ".xlsm": _xlsx_to_markdown_with_images,
    ".xltx": _xlsx_to_markdown_with_images,
    ".xltm": _xlsx_to_markdown_with_images,
    ".xlsb": _xlsx_to_markdown_with_images,
    ".ods":  _xlsx_to_markdown_with_images,
    ".html": _html_to_markdown_with_images,
    ".htm":  _html_to_markdown_with_images,
    ".pdf":  _pdf_to_markdown_with_images,
    ".epub": _epub_to_markdown_with_images,
    ".ipynb": _ipynb_to_markdown_with_images,
    ".eml":  _eml_to_markdown_with_images,
    ".mbox": _eml_to_markdown_with_images,
    ".msg":  _msg_to_markdown_with_images,
    ".odt":  _odf_to_markdown_with_images,
    ".odp":  _odf_to_markdown_with_images,
}

_CHUNKS_IMAGE_DISPATCH: dict[str, Any] = {
    ".doc":  _chunk_doc_with_images,
    ".docx": _chunk_docx_with_images,
    ".docm": _chunk_docx_with_images,
    ".dotx": _chunk_docx_with_images,
    ".dotm": _chunk_docx_with_images,
    ".ppt":  _chunk_ppt_with_images,
    ".pptx": _chunk_pptx_with_images,
    ".potx": _chunk_pptx_with_images,
    ".potm": _chunk_pptx_with_images,
    ".ppsx": _chunk_pptx_with_images,
    ".ppsm": _chunk_pptx_with_images,
    ".xlsx": _chunk_xlsx_with_images,
    ".xlsm": _chunk_xlsx_with_images,
    ".xltx": _chunk_xlsx_with_images,
    ".xltm": _chunk_xlsx_with_images,
    ".xlsb": _chunk_xlsx_with_images,
    ".ods":  _chunk_xlsx_with_images,
    ".html": _chunk_html_with_images,
    ".htm":  _chunk_html_with_images,
    ".pdf":  _chunk_pdf_with_images,
    ".epub": _chunk_epub_with_images,
    ".ipynb": _chunk_ipynb_with_images,
    ".eml":  _chunk_eml_with_images,
    ".mbox": _chunk_eml_with_images,
    ".msg":  _chunk_msg_with_images,
    ".odt":  _chunk_odf_with_images,
    ".odp":  _chunk_odf_with_images,
}


def _resolve_markdown_dispatch_ext(ext: str) -> str:
    if ext not in _MD_DISPATCH:
        raise ValueError(
            f"get_markdown does not support '{ext}'. "
            f"Supported: {sorted(_MD_DISPATCH)}"
        )
    return ext


def _get_markdown_from_temp_data(data: bytes, ext: str, list_images: bool = False):
    with tempfile.NamedTemporaryFile(suffix=ext, delete=False) as tmp:
        tmp.write(data)
        tmp_path = tmp.name
    try:
        if list_images and ext in _MD_IMAGE_DISPATCH:
            md, images = _MD_IMAGE_DISPATCH[ext](tmp_path)
            return MarkdownResult(markdown=md, images=images)
        md = _MD_DISPATCH[ext](tmp_path)
        if list_images:
            return MarkdownResult(markdown=md, images={})
        return md
    finally:
        os.unlink(tmp_path)


_EXT_DOCX = ".docx"
_EXT_CSV = ".csv"
_EXT_PDF = ".pdf"
_EXT_MD = ".md"
_EXT_TXT = ".txt"
_EXT_DOC = ".doc"
_EXT_PPT = ".ppt"
_EXT_PPTX = ".pptx"
_EXT_MSG = ".msg"
_EXT_EML = ".eml"
_EXT_MBOX = ".mbox"
_EXT_ODT = ".odt"
_EXT_ODP = ".odp"
_EXT_JSON = ".json"
_EXT_JSONL = ".jsonl"
_EXT_NDJSON = ".ndjson"
_EXT_RTF = ".rtf"
_EXT_EPUB = ".epub"
_EXT_IPYNB = ".ipynb"

# OOXML Word/PowerPoint variant families routed through the docx / pptx chunkers.
# `.ppt` (legacy binary) is intentionally excluded — it uses the separate ppt chunker.
_WORD_OOXML_EXTS = (".docx", ".docm", ".dotx", ".dotm")
_PPTX_OOXML_EXTS = (".pptx", ".potx", ".potm", ".ppsx", ".ppsm")
_EXT_HTML = ".html"
_EXT_HTM = ".htm"
_EXT_XLSX = ".xlsx"
_EXT_XLS = ".xls"
_EXT_TSV = ".tsv"

# All calamine-backed spreadsheet formats routed through the xlsx chunker, and
# all delimited-text formats routed through the csv chunker. Membership drives
# the source-agnostic entry points (get_chunks / stream_chunks / …).
_SPREADSHEET_EXTS = (_EXT_XLSX, _EXT_XLS, ".xlsm", ".xlsb", ".ods", ".xltx", ".xltm")
_DELIMITED_EXTS = (_EXT_CSV, _EXT_TSV)


def _delimiter_for(ext: str, delimiter: str | None) -> str | None:
    """Default a .tsv file to a tab delimiter unless the caller overrode it."""
    if delimiter is None and ext == _EXT_TSV:
        return "\t"
    return delimiter


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
    list_images: bool = False,
) -> "list[dict] | ChunksResult":
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

    if list_images:
        if ext in _CHUNKS_IMAGE_DISPATCH:
            chunk_list, images = _CHUNKS_IMAGE_DISPATCH[ext](
                file_path, mode=mode, window_size=window_size,
                overlap=overlap, sentences_per_chunk=sentences_per_chunk,
                paragraphs_per_page=paragraphs_per_page,
            )
            return ChunksResult(chunks=chunk_list, images=images)
        else:
            # Unsupported format — silent fallback, no images
            if os.path.splitext(file_path)[1].lower() in _SPREADSHEET_EXTS:
                chunks, _ = chunk_xlsx(
                    file_path,
                    mode="row" if mode == "default" else mode,
                    rows_per_chunk=_xlsx_rows_per_chunk(sentences_per_chunk),
                    window_size=window_size,
                    overlap=overlap,
                )
                return ChunksResult(chunks=chunks, images={})
            if ext in _DELIMITED_EXTS:
                csv_mode = "row" if mode == "default" else mode
                rows_per_chunk = (
                    paragraphs_per_page if csv_mode == "page_aware" else _csv_rows_per_chunk(
                        sentences_per_chunk)
                )
                chunks, _ = chunk_csv(
                    file_path,
                    mode=csv_mode,
                    rows_per_chunk=rows_per_chunk,
                    window_size=window_size,
                    overlap=overlap,
                    include_headers=True,
                    delimiter=_delimiter_for(ext, delimiter),
                    encoding=encoding,
                    skip_empty_rows=True,
                )
                return ChunksResult(chunks=chunks, images={})
            chunks, _ = _run_chunker(
                chunker, file_path, mode, window_size,
                overlap, sentences_per_chunk, paragraphs_per_page,
            )
            return ChunksResult(chunks=chunks, images={})

    if os.path.splitext(file_path)[1].lower() in _SPREADSHEET_EXTS:
        chunks, _ = chunk_xlsx(
            file_path,
            mode="row" if mode == "default" else mode,
            rows_per_chunk=_xlsx_rows_per_chunk(sentences_per_chunk),
            window_size=window_size,
            overlap=overlap,
        )
        return chunks

    if ext in _DELIMITED_EXTS:
        csv_mode = "row" if mode == "default" else mode
        rows_per_chunk = (
            paragraphs_per_page if csv_mode == "page_aware" else _csv_rows_per_chunk(
                sentences_per_chunk)
        )
        chunks, _ = chunk_csv(
            file_path,
            mode=csv_mode,
            rows_per_chunk=rows_per_chunk,
            window_size=window_size,
            overlap=overlap,
            include_headers=True,
            delimiter=_delimiter_for(ext, delimiter),
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
    list_images: bool = False,
) -> "list[dict] | ChunksResult":
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
        if list_images:
            if ext in _CHUNKS_IMAGE_DISPATCH:
                chunk_list, images = _CHUNKS_IMAGE_DISPATCH[ext](
                    tmp_path, mode=mode, window_size=window_size,
                    overlap=overlap, sentences_per_chunk=sentences_per_chunk,
                    paragraphs_per_page=paragraphs_per_page,
                )
                return ChunksResult(chunks=chunk_list, images=images)
            # Unsupported format — fall through to existing logic, wrap below

        if ext in _SPREADSHEET_EXTS:
            chunks, _ = chunk_xlsx(
                tmp_path,
                mode="row" if mode == "default" else mode,
                rows_per_chunk=_xlsx_rows_per_chunk(sentences_per_chunk),
                window_size=window_size,
                overlap=overlap,
            )
            if list_images:
                return ChunksResult(chunks=chunks, images={})
            return chunks

        if ext in _DELIMITED_EXTS:
            csv_mode = "row" if mode == "default" else mode
            rows_per_chunk = (
                paragraphs_per_page if csv_mode == "page_aware" else _csv_rows_per_chunk(
                    sentences_per_chunk)
            )
            chunks, _ = chunk_csv(
                tmp_path,
                mode=csv_mode,
                rows_per_chunk=rows_per_chunk,
                window_size=window_size,
                overlap=overlap,
                include_headers=True,
                delimiter=_delimiter_for(ext, delimiter),
                encoding=encoding,
                skip_empty_rows=True,
            )
            if list_images:
                return ChunksResult(chunks=chunks, images={})
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

    if list_images:
        return ChunksResult(chunks=chunks, images={})
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
    list_images: bool = False,
) -> "list[dict] | ChunksResult":
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
        list_images=list_images,
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

    if ext in _WORD_OOXML_EXTS:
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
    if ext == _EXT_MSG:
        return stream_chunk_msg(
            file_path,
            mode=mode,
            window_size=window_size,
            overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
        )
    if ext in (_EXT_EML, _EXT_MBOX):
        return stream_chunk_eml(
            file_path,
            mode=mode,
            window_size=window_size,
            overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
        )
    if ext in (_EXT_ODT, _EXT_ODP):
        return stream_chunk_odf(
            file_path,
            mode=mode,
            window_size=window_size,
            overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
        )
    if ext in (_EXT_JSON, _EXT_JSONL, _EXT_NDJSON):
        return stream_chunk_json(
            file_path,
            mode=mode,
            window_size=window_size,
            overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
        )
    if ext == _EXT_RTF:
        return stream_chunk_rtf(
            file_path,
            mode=mode,
            window_size=window_size,
            overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
        )
    if ext == _EXT_EPUB:
        return stream_chunk_epub(
            file_path,
            mode=mode,
            window_size=window_size,
            overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
        )
    if ext == _EXT_IPYNB:
        return stream_chunk_ipynb(
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
    if ext == _EXT_DOC:
        return stream_chunk_doc(
            file_path,
            mode=mode,
            window_size=window_size,
            overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
        )
    if ext == _EXT_PPT:
        return stream_chunk_ppt(
            file_path,
            mode=mode,
            window_size=window_size,
            overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
        )
    if ext in _PPTX_OOXML_EXTS:
        return stream_chunk_pptx(
            file_path,
            mode=mode,
            window_size=window_size,
            overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
        )
    if ext in _SPREADSHEET_EXTS:
        return stream_chunk_xlsx(
            file_path,
            mode="row" if mode == "default" else mode,
            rows_per_chunk=_xlsx_rows_per_chunk(sentences_per_chunk),
            window_size=window_size,
            overlap=overlap,
        )
    if ext in _DELIMITED_EXTS:
        csv_mode = "row" if mode == "default" else mode
        rows_per_chunk = (
            paragraphs_per_page if csv_mode == "page_aware" else _csv_rows_per_chunk(
                sentences_per_chunk)
        )
        return stream_chunk_csv(
            file_path,
            mode=csv_mode,
            rows_per_chunk=rows_per_chunk,
            window_size=window_size,
            overlap=overlap,
            include_headers=True,
            delimiter=_delimiter_for(ext, delimiter),
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

    if ext in _WORD_OOXML_EXTS:
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
    if ext == _EXT_MSG:
        iterator = stream_chunk_msg(
            tmp_path,
            mode=mode,
            window_size=window_size,
            overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
        )
        return _StreamingFileCleanup(iterator, tmp_path)
    if ext in (_EXT_EML, _EXT_MBOX):
        iterator = stream_chunk_eml(
            tmp_path,
            mode=mode,
            window_size=window_size,
            overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
        )
        return _StreamingFileCleanup(iterator, tmp_path)
    if ext in (_EXT_ODT, _EXT_ODP):
        iterator = stream_chunk_odf(
            tmp_path,
            mode=mode,
            window_size=window_size,
            overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
        )
        return _StreamingFileCleanup(iterator, tmp_path)
    if ext in (_EXT_JSON, _EXT_JSONL, _EXT_NDJSON):
        iterator = stream_chunk_json(
            tmp_path,
            mode=mode,
            window_size=window_size,
            overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
        )
        return _StreamingFileCleanup(iterator, tmp_path)
    if ext == _EXT_RTF:
        iterator = stream_chunk_rtf(
            tmp_path,
            mode=mode,
            window_size=window_size,
            overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
        )
        return _StreamingFileCleanup(iterator, tmp_path)
    if ext == _EXT_EPUB:
        iterator = stream_chunk_epub(
            tmp_path,
            mode=mode,
            window_size=window_size,
            overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
        )
        return _StreamingFileCleanup(iterator, tmp_path)
    if ext == _EXT_IPYNB:
        iterator = stream_chunk_ipynb(
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
    if ext == _EXT_DOC:
        iterator = stream_chunk_doc(
            tmp_path,
            mode=mode,
            window_size=window_size,
            overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
        )
        return _StreamingFileCleanup(iterator, tmp_path)
    if ext == _EXT_PPT:
        iterator = stream_chunk_ppt(
            tmp_path,
            mode=mode,
            window_size=window_size,
            overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
        )
        return _StreamingFileCleanup(iterator, tmp_path)
    if ext in _PPTX_OOXML_EXTS:
        iterator = stream_chunk_pptx(
            tmp_path,
            mode=mode,
            window_size=window_size,
            overlap=overlap,
            sentences_per_chunk=sentences_per_chunk,
            paragraphs_per_page=paragraphs_per_page,
        )
        return _StreamingFileCleanup(iterator, tmp_path)
    if ext in _SPREADSHEET_EXTS:
        iterator = stream_chunk_xlsx(
            tmp_path,
            mode="row" if mode == "default" else mode,
            rows_per_chunk=_xlsx_rows_per_chunk(sentences_per_chunk),
            window_size=window_size,
            overlap=overlap,
        )
        return _StreamingFileCleanup(iterator, tmp_path)
    if ext in _DELIMITED_EXTS:
        csv_mode = "row" if mode == "default" else mode
        rows_per_chunk = (
            paragraphs_per_page if csv_mode == "page_aware" else _csv_rows_per_chunk(
                sentences_per_chunk)
        )
        iterator = stream_chunk_csv(
            tmp_path,
            mode=csv_mode,
            rows_per_chunk=rows_per_chunk,
            window_size=window_size,
            overlap=overlap,
            include_headers=True,
            delimiter=_delimiter_for(ext, delimiter),
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
    # Resolve to a file path (reuse the same temp-file pattern as get_chunks)
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
        return _get_markdown_from_temp_data(bytes(source), ext, list_images)

    # File-like object
    if hasattr(source, "read"):
        name = filename or getattr(source, "name", None)
        if not name:
            raise ValueError(
                "filename is required for file-like objects without a .name")
        ext = _resolve_markdown_dispatch_ext(Path(name).suffix.lower())
        data = source.read()
        return _get_markdown_from_temp_data(
            data if isinstance(data, bytes) else data.encode(),
            ext,
            list_images,
        )

    raise TypeError(f"Unsupported source type: {type(source).__name__}")


__all__ = [
    "ChunksResult",
    "MarkdownResult",
    "get_chunks_from_path",
    "get_chunks_from_fileobj",
    "get_chunks_from_upload",
    "get_chunks_from_s3_presigned_url",
    "get_chunks",
    "get_markdown",
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
