"""EPUB chunker — OCF/OPF navigation + HTML chunking in spine (reading) order,
via the Rust extension."""

import time
from pathlib import Path

from py_chunks import _rust

_EPUB_MODES = {"default", "structural", "section", "semantic",
               "sliding_window", "sentence", "page_aware"}


def _validate_mode(mode: str) -> None:
    """Path-independent half of the validation — shared with the bytes route."""
    if mode not in _EPUB_MODES:
        raise ValueError(f"mode must be one of {sorted(_EPUB_MODES)} for EPUB, got: '{mode}'")


def _validate(file_path: str, path: Path, mode: str) -> None:
    if not path.is_file():
        raise FileNotFoundError(f"EPUB file not found: {file_path}")
    if path.suffix.lower() != ".epub":
        raise ValueError(f"Expected a .epub file, got: {file_path}")
    _validate_mode(mode)


def chunk_epub(
    file_path: str,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
) -> tuple[list[dict], dict]:
    """Chunk an EPUB book (reading order). Modes mirror the document chunkers."""
    path = Path(file_path)
    _validate(file_path, path, mode)
    p = str(path)
    py_start = time.perf_counter()
    if mode == "section":
        result = _rust.chunk_epub_section(p)
    elif mode == "semantic":
        result = _rust.chunk_epub_semantic(p)
    elif mode == "sentence":
        result = _rust.chunk_epub_sentence(p, sentences_per_chunk)
    elif mode == "page_aware":
        result = _rust.chunk_epub_page_aware(p, paragraphs_per_page)
    elif mode == "sliding_window":
        result = _rust.chunk_epub_sliding_window(p, window_size, overlap)
    else:
        result = _rust.chunk_epub(p)
    python_ms = max(round((time.perf_counter() - py_start) * 1000, 3), 0.001)
    return result["chunks"], {"rust_ms": result["rust_ms"], "python_ms": python_ms}


def stream_chunk_epub(
    file_path: str,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
):
    """Stream EPUB chunks as an iterator of chunk dicts (reading order)."""
    path = Path(file_path)
    _validate(file_path, path, mode)
    return _rust.stream_epub_chunks(
        str(path), mode, window_size, overlap, sentences_per_chunk, paragraphs_per_page
    )


def epub_to_markdown(file_path: str) -> str:
    """Convert an EPUB book to a Markdown string (reading order)."""
    path = Path(file_path)
    if not path.is_file():
        raise FileNotFoundError(f"EPUB file not found: {file_path}")
    if path.suffix.lower() != ".epub":
        raise ValueError(f"Expected a .epub file, got: {file_path}")
    return _rust.epub_to_markdown(str(path))


def chunk_epub_with_images(
    file_path: str,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
) -> tuple[list[dict], dict[str, bytes]]:
    """Chunk an EPUB and extract its embedded images as dedicated chunks."""
    path = Path(file_path)
    normalized = "default" if mode == "default" else mode
    _validate(file_path, path, normalized)
    chunk_list, image_list = _rust.chunk_epub_with_images(
        str(path), normalized, 1, window_size, overlap,
        sentences_per_chunk, paragraphs_per_page, 2000,
    )
    return chunk_list, dict(image_list)


def epub_to_markdown_with_images(file_path: str) -> tuple[str, dict[str, bytes]]:
    """Convert an EPUB to Markdown and extract its embedded images."""
    path = Path(file_path)
    if not path.is_file():
        raise FileNotFoundError(f"EPUB file not found: {file_path}")
    if path.suffix.lower() != ".epub":
        raise ValueError(f"Expected a .epub file, got: {file_path}")
    md, image_list = _rust.epub_to_markdown_with_images(str(path))
    return md, dict(image_list)
