"""Public Python API for py_chunks.

This module exposes both format-specific chunkers and source-agnostic helpers
that accept paths, bytes, file-like objects, upload objects, and pre-signed
URLs. Format routing lives in ``_dispatch``; the source-agnostic entry points
live in ``_sources``.
"""

from __future__ import annotations

import os
from importlib.metadata import PackageNotFoundError, version as _distribution_version
from pathlib import Path

from .chunkers.csv import chunk_csv, stream_chunk_csv
from .chunkers.doc import chunk_doc, stream_chunk_doc
from .chunkers.docx import chunk_docx, stream_chunk_docx
from .chunkers.eml import chunk_eml, stream_chunk_eml
from .chunkers.epub import chunk_epub, stream_chunk_epub
from .chunkers.html import chunk_html, stream_chunk_html
from .chunkers.ipynb import chunk_ipynb, stream_chunk_ipynb
from .chunkers.json import chunk_json, stream_chunk_json
from .chunkers.md import chunk_md, stream_chunk_md
from .chunkers.msg import chunk_msg, stream_chunk_msg
from .chunkers.odf import chunk_odf, stream_chunk_odf
from .chunkers.pdf import chunk_pdf, stream_chunk_pdf
from .chunkers.ppt import chunk_ppt, stream_chunk_ppt
from .chunkers.pptx import chunk_pptx, stream_chunk_pptx
from .chunkers.rtf import chunk_rtf, stream_chunk_rtf
from .chunkers.tsv import chunk_tsv, stream_chunk_tsv
from .chunkers.txt import chunk_txt, stream_chunk_txt
from .chunkers.xlsx import chunk_xlsx, stream_chunk_xlsx

# Routing tables re-exported for introspection (tests rely on _MD_DISPATCH).
from .fit import fit_tokens
from ._dispatch import _DISPATCH, _MD_DISPATCH  # pylint: disable=unused-import
from ._sources import (
    ChunksResult,
    MarkdownResult,
    get_chunks,
    get_chunks_from_bytes,
    get_chunks_from_fileobj,
    get_chunks_from_path,
    get_chunks_from_s3_presigned_url,
    get_chunks_from_upload,
    get_markdown,
    stream_chunks,
    stream_chunks_from_bytes,
    stream_chunks_from_fileobj,
    stream_chunks_from_path,
    stream_chunks_from_s3_presigned_url,
    stream_chunks_from_upload,
)

#: Installed distribution version of PyPI ``py-chunks``. Read from the
#: distribution metadata rather than hard-coded, so it can never drift from
#: what pip actually installed. Falls back when the package is imported from a
#: source tree that was never installed (no ``.dist-info`` to read).
try:
    __version__ = _distribution_version("py-chunks")
except PackageNotFoundError:  # pragma: no cover - source tree without install
    __version__ = "0.0.0+unknown"

_pkg_dir = Path(__file__).parent

os.environ.setdefault("PY_CHUNKS_PACKAGE_DIR", str(_pkg_dir))

# PDF parsing is handled by the `liteparse` crate, which vendors its own PDFium
# binding — no external PDFium binary resolution is required here anymore.

__all__ = [
    # Package metadata
    "__version__",
    # Token fitting (parity-exempt — see py_chunks/fit.py)
    "fit_tokens",
    # Result types
    "ChunksResult",
    "MarkdownResult",
    # Source-agnostic entry points
    "get_chunks",
    "get_chunks_from_bytes",
    "get_chunks_from_fileobj",
    "get_chunks_from_path",
    "get_chunks_from_s3_presigned_url",
    "get_chunks_from_upload",
    "get_markdown",
    "stream_chunks",
    "stream_chunks_from_bytes",
    "stream_chunks_from_fileobj",
    "stream_chunks_from_path",
    "stream_chunks_from_s3_presigned_url",
    "stream_chunks_from_upload",
    # Per-format chunkers (one batch + one streaming entry per module)
    "chunk_csv",
    "stream_chunk_csv",
    "chunk_doc",
    "stream_chunk_doc",
    "chunk_docx",
    "stream_chunk_docx",
    "chunk_eml",
    "stream_chunk_eml",
    "chunk_epub",
    "stream_chunk_epub",
    "chunk_html",
    "stream_chunk_html",
    "chunk_ipynb",
    "stream_chunk_ipynb",
    "chunk_json",
    "stream_chunk_json",
    "chunk_md",
    "stream_chunk_md",
    "chunk_msg",
    "stream_chunk_msg",
    "chunk_odf",
    "stream_chunk_odf",
    "chunk_pdf",
    "stream_chunk_pdf",
    "chunk_ppt",
    "stream_chunk_ppt",
    "chunk_pptx",
    "stream_chunk_pptx",
    "chunk_rtf",
    "stream_chunk_rtf",
    "chunk_tsv",
    "stream_chunk_tsv",
    "chunk_txt",
    "stream_chunk_txt",
    "chunk_xlsx",
    "stream_chunk_xlsx",
]
