"""HTML chunker wrapper over the Rust extension."""

from __future__ import annotations

import time
from pathlib import Path

from py_chunks import _rust

_HTML_EXTS = {".html", ".htm"}
_HTML_MODES = {
    "default", "structural", "semantic", "section",
    "sliding_window", "sentence", "page_aware",
}


def _validate_html_options(
    mode: str,
    window_size: int,
    overlap: int,
    sentences_per_chunk: int,
    paragraphs_per_page: int,
) -> None:
    """Path-independent half of the validation — shared with the bytes route."""
    if mode not in _HTML_MODES:
        raise ValueError(
            f"mode must be one of {sorted(_HTML_MODES)} for HTML, got: '{mode}'"
        )
    if mode == "sliding_window":
        if window_size <= 0:
            raise ValueError("window_size must be greater than 0")
        if overlap >= window_size:
            raise ValueError("overlap must be less than window_size")
    if mode == "sentence" and sentences_per_chunk <= 0:
        raise ValueError("sentences_per_chunk must be greater than 0")
    if mode == "page_aware" and paragraphs_per_page <= 0:
        raise ValueError("paragraphs_per_page must be greater than 0")


def chunk_html(
    file_path: str,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
) -> tuple[list[dict], dict]:
    """Chunk an HTML file using the Rust extension module.

    Args:
        file_path: Path to the .html / .htm file.
        mode: Chunking mode. One of:
              ``"default"`` / ``"structural"`` — one chunk per block element.
              ``"semantic"``      — topic-continuity grouping (10 signals).
              ``"section"``       — group under h1-h6 headings with breadcrumb.
              ``"sliding_window"``— overlapping block windows.
              ``"sentence"``      — N sentences per chunk.
              ``"page_aware"``    — heading-boundary or block-count groups.
        window_size: Blocks per window (``sliding_window`` only, default 3).
        overlap: Overlapping blocks (``sliding_window`` only, default 1).
        sentences_per_chunk: Sentences per chunk (``sentence`` only, default 3).
        paragraphs_per_page: Block count before flush (``page_aware``, default 15).

    Returns:
        (chunks, timing) where timing has ``rust_ms`` and ``python_ms`` keys.
    """
    path = Path(file_path)
    if not path.is_file():
        raise FileNotFoundError(f"HTML file not found: {file_path}")
    if path.suffix.lower() not in _HTML_EXTS:
        raise ValueError(f"Expected a .html / .htm file, got: {file_path}")
    _validate_html_options(mode, window_size, overlap, sentences_per_chunk, paragraphs_per_page)

    py_start = time.perf_counter()
    path_str = str(path)

    if mode == "semantic":
        result = _rust.chunk_html_semantic(path_str)
    elif mode == "section":
        result = _rust.chunk_html_section(path_str)
    elif mode == "sliding_window":
        result = _rust.chunk_html_sliding_window(path_str, window_size, overlap)
    elif mode == "sentence":
        result = _rust.chunk_html_sentence(path_str, sentences_per_chunk)
    elif mode == "page_aware":
        result = _rust.chunk_html_page_aware(path_str, paragraphs_per_page)
    else:
        result = _rust.chunk_html(path_str)

    python_ms = round((time.perf_counter() - py_start) * 1000, 3)
    return result["chunks"], {"rust_ms": result["rust_ms"], "python_ms": python_ms}


def stream_chunk_html(
    file_path: str,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
):
    """Stream chunks from an HTML file as an iterator.

    ``structural`` and ``semantic`` use true block-by-block state machines.
    All other modes compute chunks upfront and drain one per iteration step.
    """
    path = Path(file_path)
    if not path.is_file():
        raise FileNotFoundError(f"HTML file not found: {file_path}")
    if path.suffix.lower() not in _HTML_EXTS:
        raise ValueError(f"Expected a .html / .htm file, got: {file_path}")
    if mode not in _HTML_MODES:
        raise ValueError(
            f"mode must be one of {sorted(_HTML_MODES)} for HTML streaming, got: '{mode}'"
        )
    return _rust.stream_html_chunks(
        str(path), mode, window_size, overlap, sentences_per_chunk, paragraphs_per_page,
    )


def html_to_markdown(file_path: str) -> str:
    """Convert an HTML file to a Markdown string."""
    path = Path(file_path)
    if not path.is_file():
        raise FileNotFoundError(f"HTML file not found: {file_path}")
    if path.suffix.lower() not in (".html", ".htm"):
        raise ValueError(f"Expected a .html or .htm file, got: {file_path}")
    return _rust.html_to_markdown(str(path))


def chunk_html_with_images(
    file_path: str,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
) -> tuple[list[dict], dict[str, bytes]]:
    """Chunk an HTML file and extract embedded images as dedicated chunks.

    Returns (chunks, images) where images maps hash-named filenames to raw bytes.
    Supports base64 data URI images and local file references. Remote URLs are skipped.
    """
    path = Path(file_path)
    if not path.is_file():
        raise FileNotFoundError(f"HTML file not found: {file_path}")
    if path.suffix.lower() not in _HTML_EXTS:
        raise ValueError(f"Expected a .html / .htm file, got: {file_path}")
    if mode not in _HTML_MODES:
        raise ValueError(
            f"mode must be one of {sorted(_HTML_MODES)} for HTML, got: '{mode}'"
        )
    chunk_list, image_list = _rust.chunk_html_with_images(
        str(path), mode, window_size, overlap, sentences_per_chunk, paragraphs_per_page
    )
    return chunk_list, dict(image_list)


def html_to_markdown_with_images(file_path: str) -> tuple[str, dict[str, bytes]]:
    """Convert an HTML file to Markdown and extract embedded images.

    Returns (markdown, images) where markdown contains ![alt](hash.ext) references
    and images maps hash-named filenames to raw bytes.
    """
    path = Path(file_path)
    if not path.is_file():
        raise FileNotFoundError(f"HTML file not found: {file_path}")
    if path.suffix.lower() not in (".html", ".htm"):
        raise ValueError(f"Expected a .html or .htm file, got: {file_path}")
    md, image_list = _rust.html_to_markdown_with_images(str(path))
    return md, dict(image_list)
