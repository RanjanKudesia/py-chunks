"""PDF chunker wrapper over the Rust extension."""

import time
from pathlib import Path

from py_chunks import _rust

_SUPPORTED_MODES = {
    "default",
    "structural",
    "section",
    "semantic",
    "sliding_window",
    "sentence",
    "page_aware",
}


def _validate_pdf_args(
    path: Path,
    file_path: str,
    mode: str,
    window_size: int,
    overlap: int,
    sentences_per_chunk: int,
    paragraphs_per_page: int,
) -> None:
    if not path.is_file():
        raise FileNotFoundError(f"PDF file not found: {file_path}")
    if path.suffix.lower() != ".pdf":
        raise ValueError(f"Expected a .pdf file, got: {file_path}")
    if mode not in _SUPPORTED_MODES:
        raise ValueError(
            "mode must be 'default', 'structural', 'section', 'semantic', "
            "'sliding_window', 'sentence', or 'page_aware' for PDF"
        )
    if mode == "sliding_window" and window_size <= 0:
        raise ValueError("window_size must be greater than 0")
    if mode == "sliding_window" and overlap >= window_size:
        raise ValueError("overlap must be less than window_size")
    if mode == "sentence" and sentences_per_chunk <= 0:
        raise ValueError("sentences_per_chunk must be greater than 0")
    if mode == "page_aware" and paragraphs_per_page <= 0:
        raise ValueError("paragraphs_per_page must be greater than 0")


def _run_pdf_mode(
    path: Path,
    mode: str,
    window_size: int,
    overlap: int,
    sentences_per_chunk: int,
    paragraphs_per_page: int,
):
    path_str = str(path)
    if mode == "default":
        return _rust.chunk_pdf_fast(path_str)
    if mode == "section":
        return _rust.chunk_pdf_section(path_str)
    if mode == "semantic":
        return _rust.chunk_pdf_semantic(path_str)
    if mode == "sentence":
        return _rust.chunk_pdf_sentence(path_str, sentences_per_chunk)
    if mode == "page_aware":
        return _rust.chunk_pdf_page_aware(path_str, paragraphs_per_page)
    if mode == "sliding_window":
        return _rust.chunk_pdf_sliding_window(path_str, window_size, overlap)
    return _rust.chunk_pdf_structural(path_str)


def chunk_pdf(
    file_path: str,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
) -> tuple[list[dict], dict]:
    """Chunk a PDF file using the Rust extension module.

    Args:
        file_path: Path to the .pdf file.
        mode: Chunking mode. One of:
              ``"default"``     — heading sizes ranked within each page, via
              ``chunk_pdf_fast``. Holds one page at a time.
              ``"structural"``  — heading sizes ranked across the whole
              document, via ``chunk_pdf_structural``.
              ``"section"``     — heading-scoped grouping.
              ``"semantic"``    — heuristic topic-continuity grouping.
              ``"sliding_window"`` — overlapping paragraph windows.
              ``"sentence"``    — N sentences per chunk.
              ``"page_aware"``  — page-boundary grouping.
              ``"default"`` and ``"structural"`` are not equivalent for
              PDF: they extract the same text and differ only in which
              lines are called headings, and at what level.

    Returns:
        (chunks, timing) where:
          - chunks: list of dicts with keys content, content_type, metadata
          - timing: dict with keys
              rust_ms   - extract + chunking pipeline measured inside Rust
              python_ms - full trip time measured in Python
    """
    path = Path(file_path)
    _validate_pdf_args(
        path,
        file_path,
        mode,
        window_size,
        overlap,
        sentences_per_chunk,
        paragraphs_per_page,
    )

    py_start = time.perf_counter()
    result = _run_pdf_mode(
        path,
        mode,
        window_size,
        overlap,
        sentences_per_chunk,
        paragraphs_per_page,
    )
    python_ms = round((time.perf_counter() - py_start) * 1000, 3)

    timing = {
        "rust_ms": result["rust_ms"],
        "python_ms": python_ms,
    }
    return result["chunks"], timing


def stream_chunk_pdf(
    file_path: str,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
):
    """Stream PDF chunks from Rust as an iterator of chunk dicts.

    This preserves existing chunk output shape while yielding one chunk at a time.
    """
    path = Path(file_path)
    _validate_pdf_args(
        path,
        file_path,
        mode,
        window_size,
        overlap,
        sentences_per_chunk,
        paragraphs_per_page,
    )

    return _rust.stream_pdf_chunks(
        str(path),
        mode,
        window_size,
        overlap,
        sentences_per_chunk,
        paragraphs_per_page,
    )


def pdf_to_markdown(file_path: str) -> str:
    """Convert a PDF file to a Markdown string.

    Args:
        file_path: Path to the .pdf file.

    Returns:
        Markdown string with headings, paragraphs, bullet lists, tables,
        and ``---`` page-break separators.
    """
    path = Path(file_path)
    if not path.is_file():
        raise FileNotFoundError(f"PDF file not found: {file_path}")
    if path.suffix.lower() != ".pdf":
        raise ValueError(f"Expected a .pdf file, got: {file_path}")
    return _rust.pdf_to_markdown(str(path))


def chunk_pdf_with_images(
    file_path: str,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
) -> tuple[list[dict], dict[str, bytes]]:
    """Chunk a PDF file and extract embedded images as dedicated chunks.

    Image chunks (``content_type == "image"``) are prepended to the text
    chunks, one per page-occurrence of an embedded raster. Each carries a
    ``page_number`` and ``image_name`` in its metadata.

    Returns:
        (chunks, images) where images maps hash-named ``.png`` filenames to
        raw PNG bytes (deduplicated by content).
    """
    path = Path(file_path)
    _validate_pdf_args(
        path,
        file_path,
        mode,
        window_size,
        overlap,
        sentences_per_chunk,
        paragraphs_per_page,
    )
    chunk_list, image_list = _rust.chunk_pdf_with_images(
        str(path),
        mode,
        window_size,
        overlap,
        sentences_per_chunk,
        paragraphs_per_page,
    )
    return chunk_list, dict(image_list)


def pdf_to_markdown_with_images(file_path: str) -> tuple[str, dict[str, bytes]]:
    """Convert a PDF file to Markdown and extract embedded images.

    Returns:
        (markdown, images) where markdown contains ``![name](name)`` references
        at the top of each page's content, and images maps hash-named ``.png``
        filenames to raw PNG bytes (deduplicated by content).
    """
    path = Path(file_path)
    if not path.is_file():
        raise FileNotFoundError(f"PDF file not found: {file_path}")
    if path.suffix.lower() != ".pdf":
        raise ValueError(f"Expected a .pdf file, got: {file_path}")
    md, image_list = _rust.pdf_to_markdown_with_images(str(path))
    return md, dict(image_list)
