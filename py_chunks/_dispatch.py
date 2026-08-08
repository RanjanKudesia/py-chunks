"""Format routing tables and shared helpers for the source-agnostic API.

Single source of truth for which extension goes to which chunker, markdown
converter, image extractor, streaming entry point, and Python-side option
validator. The entry points in ``_sources.py`` consume these tables;
``py_chunks/__init__.py`` re-exports the public names.
"""

from __future__ import annotations

import os

from .chunkers.csv import (
    _parse_delimiter,
    _validate_csv_options,
    chunk_csv,
    csv_to_markdown,
    stream_chunk_csv,
)
from .chunkers.doc import (
    _validate_doc_options,
    chunk_doc,
    chunk_doc_with_images,
    doc_to_markdown,
    doc_to_markdown_with_images,
    stream_chunk_doc,
)
from .chunkers.docx import (
    _validate_docx_options,
    chunk_docx,
    chunk_docx_with_images,
    docx_to_markdown,
    docx_to_markdown_with_images,
    stream_chunk_docx,
)
from .chunkers.eml import (
    _validate_mode as _validate_eml_mode,
    chunk_eml,
    chunk_eml_with_images,
    eml_to_markdown,
    eml_to_markdown_with_images,
    stream_chunk_eml,
)
from .chunkers.epub import (
    _validate_mode as _validate_epub_mode,
    chunk_epub,
    chunk_epub_with_images,
    epub_to_markdown,
    epub_to_markdown_with_images,
    stream_chunk_epub,
)
from .chunkers.html import (
    _validate_html_options,
    chunk_html,
    chunk_html_with_images,
    html_to_markdown,
    html_to_markdown_with_images,
    stream_chunk_html,
)
from .chunkers.ipynb import (
    _validate_mode as _validate_ipynb_mode,
    chunk_ipynb,
    chunk_ipynb_with_images,
    ipynb_to_markdown,
    ipynb_to_markdown_with_images,
    stream_chunk_ipynb,
)
from .chunkers.json import (
    _validate_mode as _validate_json_mode,
    chunk_json,
    json_to_markdown,
    stream_chunk_json,
)
from .chunkers.md import _validate_md_options, chunk_md, md_to_markdown, stream_chunk_md
from .chunkers.msg import (
    _validate_mode as _validate_msg_mode,
    chunk_msg,
    chunk_msg_with_images,
    msg_to_markdown,
    msg_to_markdown_with_images,
    stream_chunk_msg,
)
from .chunkers.odf import (
    _validate_mode as _validate_odf_mode,
    chunk_odf,
    chunk_odf_with_images,
    odf_to_markdown,
    odf_to_markdown_with_images,
    stream_chunk_odf,
)
from .chunkers.pdf import (
    _validate_pdf_options,
    chunk_pdf,
    chunk_pdf_with_images,
    pdf_to_markdown,
    pdf_to_markdown_with_images,
    stream_chunk_pdf,
)
from .chunkers.ppt import (
    _validate_ppt_options,
    chunk_ppt,
    chunk_ppt_with_images,
    ppt_to_markdown,
    ppt_to_markdown_with_images,
    stream_chunk_ppt,
)
from .chunkers.pptx import (
    _validate_pptx_options,
    chunk_pptx,
    chunk_pptx_with_images,
    pptx_to_markdown,
    pptx_to_markdown_with_images,
    stream_chunk_pptx,
)
from .chunkers.rtf import (
    _validate_mode as _validate_rtf_mode,
    chunk_rtf,
    rtf_to_markdown,
    stream_chunk_rtf,
)
from .chunkers.tsv import chunk_tsv, tsv_to_markdown
from .chunkers.txt import _validate_txt_options, chunk_txt, stream_chunk_txt, txt_to_markdown
from .chunkers.xlsx import (
    _validate_xlsx_options,
    chunk_xlsx,
    chunk_xlsx_with_images,
    stream_chunk_xlsx,
    xlsx_to_markdown,
    xlsx_to_markdown_with_images,
)

# ── Extension families ───────────────────────────────────────────────────────

# OOXML Word/PowerPoint variant families routed through the docx / pptx chunkers.
# `.ppt` (legacy binary) is intentionally excluded — it uses the separate ppt chunker.
_WORD_OOXML_EXTS = (".docx", ".docm", ".dotx", ".dotm")
_PPTX_OOXML_EXTS = (".pptx", ".potx", ".potm", ".ppsx", ".ppsm")

# All calamine-backed spreadsheet formats routed through the xlsx chunker, and
# all delimited-text formats routed through the csv chunker. Membership drives
# the source-agnostic entry points (get_chunks / stream_chunks / …).
_SPREADSHEET_EXTS = (".xlsx", ".xls", ".xlsm", ".xlsb", ".ods", ".xltx", ".xltm")
_DELIMITED_EXTS = (".csv", ".tsv")

_EMAIL_EXTS = (".eml", ".mbox")
_ODF_EXTS = (".odt", ".odp")
_JSON_EXTS = (".json", ".jsonl", ".ndjson")
_HTML_EXTS = (".html", ".htm")


def _family(exts, value):
    """One `{ext: value}` mapping per member of an extension family."""
    return {ext: value for ext in exts}


# ── Routing tables ───────────────────────────────────────────────────────────

_DISPATCH = {
    ".doc": chunk_doc,
    **_family(_WORD_OOXML_EXTS, chunk_docx),
    ".csv": chunk_csv,
    ".tsv": chunk_tsv,
    ".msg": chunk_msg,
    **_family(_EMAIL_EXTS, chunk_eml),
    **_family(_ODF_EXTS, chunk_odf),
    **_family(_JSON_EXTS, chunk_json),
    ".rtf": chunk_rtf,
    ".epub": chunk_epub,
    ".ipynb": chunk_ipynb,
    **_family(_HTML_EXTS, chunk_html),
    ".md": chunk_md,
    ".pdf": chunk_pdf,
    ".ppt": chunk_ppt,
    **_family(_PPTX_OOXML_EXTS, chunk_pptx),
    ".txt": chunk_txt,
    **_family(_SPREADSHEET_EXTS, chunk_xlsx),
}

_MD_DISPATCH = {
    ".doc": doc_to_markdown,
    **_family(_WORD_OOXML_EXTS, docx_to_markdown),
    ".ppt": ppt_to_markdown,
    **_family(_PPTX_OOXML_EXTS, pptx_to_markdown),
    ".pdf": pdf_to_markdown,
    **_family(_HTML_EXTS, html_to_markdown),
    **_family(_SPREADSHEET_EXTS, xlsx_to_markdown),
    ".txt": txt_to_markdown,
    ".md": md_to_markdown,
    ".csv": csv_to_markdown,
    ".tsv": tsv_to_markdown,
    ".msg": msg_to_markdown,
    **_family(_EMAIL_EXTS, eml_to_markdown),
    **_family(_ODF_EXTS, odf_to_markdown),
    **_family(_JSON_EXTS, json_to_markdown),
    ".rtf": rtf_to_markdown,
    ".epub": epub_to_markdown,
    ".ipynb": ipynb_to_markdown,
}

# Formats with embedded-image extraction. `.xls` (OLE) and the text-only
# formats are intentionally absent: they have no image parts to extract.
_MD_IMAGE_DISPATCH = {
    ".doc": doc_to_markdown_with_images,
    **_family(_WORD_OOXML_EXTS, docx_to_markdown_with_images),
    ".ppt": ppt_to_markdown_with_images,
    **_family(_PPTX_OOXML_EXTS, pptx_to_markdown_with_images),
    **_family((".xlsx", ".xlsm", ".xltx", ".xltm", ".xlsb", ".ods"),
              xlsx_to_markdown_with_images),
    **_family(_HTML_EXTS, html_to_markdown_with_images),
    ".pdf": pdf_to_markdown_with_images,
    ".epub": epub_to_markdown_with_images,
    ".ipynb": ipynb_to_markdown_with_images,
    **_family(_EMAIL_EXTS, eml_to_markdown_with_images),
    ".msg": msg_to_markdown_with_images,
    **_family(_ODF_EXTS, odf_to_markdown_with_images),
}

_CHUNKS_IMAGE_DISPATCH = {
    ".doc": chunk_doc_with_images,
    **_family(_WORD_OOXML_EXTS, chunk_docx_with_images),
    ".ppt": chunk_ppt_with_images,
    **_family(_PPTX_OOXML_EXTS, chunk_pptx_with_images),
    **_family((".xlsx", ".xlsm", ".xltx", ".xltm", ".xlsb", ".ods"),
              chunk_xlsx_with_images),
    **_family(_HTML_EXTS, chunk_html_with_images),
    ".pdf": chunk_pdf_with_images,
    ".epub": chunk_epub_with_images,
    ".ipynb": chunk_ipynb_with_images,
    **_family(_EMAIL_EXTS, chunk_eml_with_images),
    ".msg": chunk_msg_with_images,
    **_family(_ODF_EXTS, chunk_odf_with_images),
}

# Streaming entry point per extension for the formats that take the standard
# (mode, window_size, overlap, sentences_per_chunk, paragraphs_per_page)
# signature. Spreadsheet and delimited formats have their own parameter
# mapping and are routed explicitly in `_open_stream`.
_STREAM_DISPATCH = {
    ".doc": stream_chunk_doc,
    **_family(_WORD_OOXML_EXTS, stream_chunk_docx),
    ".ppt": stream_chunk_ppt,
    **_family(_PPTX_OOXML_EXTS, stream_chunk_pptx),
    ".pdf": stream_chunk_pdf,
    ".msg": stream_chunk_msg,
    **_family(_EMAIL_EXTS, stream_chunk_eml),
    **_family(_ODF_EXTS, stream_chunk_odf),
    **_family(_JSON_EXTS, stream_chunk_json),
    ".rtf": stream_chunk_rtf,
    ".epub": stream_chunk_epub,
    ".ipynb": stream_chunk_ipynb,
    ".md": stream_chunk_md,
    ".txt": stream_chunk_txt,
    **_family(_HTML_EXTS, stream_chunk_html),
}

_SUPPORTED = ", ".join(sorted(_DISPATCH))

# ── Shared helpers ───────────────────────────────────────────────────────────


def _delimiter_for(ext, delimiter):
    """Default a .tsv file to a tab delimiter unless the caller overrode it."""
    if delimiter is None and ext == ".tsv":
        return "\t"
    return delimiter


def _xlsx_rows_per_chunk(sentences_per_chunk):
    return 1 if sentences_per_chunk == 3 else sentences_per_chunk


def _validate_xlsx_page_arg(mode, paragraphs_per_page):
    """Reject `paragraphs_per_page = 0` for the spreadsheet family.

    Spreadsheets paginate by `rows_per_chunk`, so `paragraphs_per_page` is
    dropped at this mapping site and never reaches the xlsx chunker — which
    meant `page_aware` with `paragraphs_per_page=0` was silently accepted here
    while every other format rejects it (and three docs pages promise it is
    rejected). Mode-scoped exactly like the engine's `validate_mode_args`: the
    value only has to be usable by the mode that reads it.
    """
    if mode == "page_aware" and paragraphs_per_page <= 0:
        raise ValueError("paragraphs_per_page must be greater than 0")


def _csv_rows_per_chunk(sentences_per_chunk):
    return max(1, sentences_per_chunk)


def _csv_options(mode, sentences_per_chunk, paragraphs_per_page):
    """Map the generic chunking parameters onto the CSV chunker's own."""
    csv_mode = "row" if mode == "default" else mode
    rows_per_chunk = (
        paragraphs_per_page if csv_mode == "page_aware"
        else _csv_rows_per_chunk(sentences_per_chunk)
    )
    return csv_mode, rows_per_chunk


def _resolve_chunker(filename):
    ext = os.path.splitext(filename)[1].lower()
    chunker = _DISPATCH.get(ext)
    if chunker is None:
        raise ValueError(
            f"Unsupported file type '{ext}'. Supported: {_SUPPORTED}"
        )
    return chunker, ext


def _resolve_markdown_dispatch_ext(ext):
    if ext not in _MD_DISPATCH:
        raise ValueError(
            f"get_markdown does not support '{ext}'. "
            f"Supported: {sorted(_MD_DISPATCH)}"
        )
    return ext


# ── Routing (batch + streaming) ──────────────────────────────────────────────


def _chunk_file(ext, file_path, *, mode, window_size, overlap,
                sentences_per_chunk, paragraphs_per_page, delimiter, encoding):
    """Chunk one on-disk file, applying the family-specific parameter mapping."""
    if ext in _SPREADSHEET_EXTS:
        _validate_xlsx_page_arg(mode, paragraphs_per_page)
        chunks, _ = chunk_xlsx(
            file_path,
            mode="row" if mode == "default" else mode,
            rows_per_chunk=_xlsx_rows_per_chunk(sentences_per_chunk),
            window_size=window_size,
            overlap=overlap,
        )
        return chunks
    if ext in _DELIMITED_EXTS:
        csv_mode, rows_per_chunk = _csv_options(mode, sentences_per_chunk, paragraphs_per_page)
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
    chunks, _ = _DISPATCH[ext](
        file_path,
        mode=mode,
        window_size=window_size,
        overlap=overlap,
        sentences_per_chunk=sentences_per_chunk,
        paragraphs_per_page=paragraphs_per_page,
    )
    return chunks


def _open_stream(ext, file_path, *, mode, window_size, overlap,
                 sentences_per_chunk, paragraphs_per_page, delimiter, encoding):
    """Open a streaming iterator over one on-disk file (table-driven)."""
    if ext in _SPREADSHEET_EXTS:
        _validate_xlsx_page_arg(mode, paragraphs_per_page)
        return stream_chunk_xlsx(
            file_path,
            mode="row" if mode == "default" else mode,
            rows_per_chunk=_xlsx_rows_per_chunk(sentences_per_chunk),
            window_size=window_size,
            overlap=overlap,
        )
    if ext in _DELIMITED_EXTS:
        csv_mode, rows_per_chunk = _csv_options(mode, sentences_per_chunk, paragraphs_per_page)
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
    stream_fn = _STREAM_DISPATCH.get(ext)
    if stream_fn is None:
        raise NotImplementedError(f"Streaming not yet supported for {ext} files")
    return stream_fn(
        file_path,
        mode=mode,
        window_size=window_size,
        overlap=overlap,
        sentences_per_chunk=sentences_per_chunk,
        paragraphs_per_page=paragraphs_per_page,
    )


# ── Bytes-path validation ────────────────────────────────────────────────────


def _mode_only(check):
    """Adapt a mode-only wrapper validator to the standard option signature."""
    def validate(mode, _window_size, _overlap, _sentences_per_chunk, _paragraphs_per_page):
        check(mode)
    return validate


# Per-extension option validators — the exact validation the path-based
# wrappers run, so the bytes route rejects bad arguments with identical
# exception types and messages (the engine's own wording differs).
_OPTION_VALIDATORS = {
    ".doc": _validate_doc_options,
    **_family(_WORD_OOXML_EXTS, _validate_docx_options),
    ".ppt": _validate_ppt_options,
    **_family(_PPTX_OOXML_EXTS, _validate_pptx_options),
    ".pdf": _validate_pdf_options,
    ".md": _validate_md_options,
    ".txt": _validate_txt_options,
    **_family(_HTML_EXTS, _validate_html_options),
    ".msg": _mode_only(_validate_msg_mode),
    **_family(_EMAIL_EXTS, _mode_only(_validate_eml_mode)),
    **_family(_ODF_EXTS, _mode_only(_validate_odf_mode)),
    **_family(_JSON_EXTS, _mode_only(_validate_json_mode)),
    ".rtf": _mode_only(_validate_rtf_mode),
    ".epub": _mode_only(_validate_epub_mode),
    ".ipynb": _mode_only(_validate_ipynb_mode),
}


def _csv_bytes_args(ext, mode, window_size, overlap, sentences_per_chunk,
                    paragraphs_per_page, delimiter, encoding):
    """Validate and map arguments for the delimited-text bytes entry point.

    Runs the same option validation `chunk_csv` runs, then returns the
    `(csv_mode, rows_per_chunk, delimiter_byte)` triple the Rust bytes
    function takes.
    """
    csv_mode, rows_per_chunk = _csv_options(mode, sentences_per_chunk, paragraphs_per_page)
    _validate_csv_options(csv_mode, rows_per_chunk, window_size, overlap, encoding)
    return csv_mode, rows_per_chunk, _parse_delimiter(_delimiter_for(ext, delimiter))


def _validate_bytes_options(ext, mode, window_size, overlap,
                            sentences_per_chunk, paragraphs_per_page):
    """Run the same Python-side validation the path-based wrappers run.

    Delimited formats validate separately (they also take delimiter/encoding);
    spreadsheet formats validate through the xlsx wrapper's option checks with
    the same parameter mapping the batch route applies.
    """
    if ext in _SPREADSHEET_EXTS:
        _validate_xlsx_page_arg(mode, paragraphs_per_page)
        _validate_xlsx_options(
            "row" if mode == "default" else mode,
            _xlsx_rows_per_chunk(sentences_per_chunk),
            window_size, overlap, "key_value", 2000,
        )
        return
    validator = _OPTION_VALIDATORS.get(ext)
    if validator is not None:
        validator(mode, window_size, overlap, sentences_per_chunk, paragraphs_per_page)
