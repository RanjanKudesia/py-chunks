"""OpenDocument chunker — `.odt` (text) and `.odp` (presentation). Parsed via the
Rust extension (zip + content.xml walker), assembled into markdown, and chunked
through the Markdown pipeline. Mirrors the `.eml`/`.msg` chunkers."""

import time
from pathlib import Path

from py_chunks import _rust

_ODF_MODES = {"default", "structural", "section", "semantic",
              "sliding_window", "sentence", "page_aware"}
_ODF_SUFFIXES = {".odt", ".odp"}


def _validate(file_path: str, path: Path, mode: str) -> None:
    if not path.is_file():
        raise FileNotFoundError(f"ODF file not found: {file_path}")
    if path.suffix.lower() not in _ODF_SUFFIXES:
        raise ValueError(f"Expected a .odt or .odp file, got: {file_path}")
    if mode not in _ODF_MODES:
        raise ValueError(f"mode must be one of {sorted(_ODF_MODES)} for ODF, got: '{mode}'")


def chunk_odf(
    file_path: str,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
) -> tuple[list[dict], dict]:
    """Chunk a `.odt` document or `.odp` presentation. Modes mirror the document chunkers."""
    path = Path(file_path)
    _validate(file_path, path, mode)
    p = str(path)
    py_start = time.perf_counter()
    if mode == "section":
        result = _rust.chunk_odf_section(p)
    elif mode == "semantic":
        result = _rust.chunk_odf_semantic(p)
    elif mode == "sentence":
        result = _rust.chunk_odf_sentence(p, sentences_per_chunk)
    elif mode == "page_aware":
        result = _rust.chunk_odf_page_aware(p, paragraphs_per_page)
    elif mode == "sliding_window":
        result = _rust.chunk_odf_sliding_window(p, window_size, overlap)
    else:
        result = _rust.chunk_odf(p)
    python_ms = max(round((time.perf_counter() - py_start) * 1000, 3), 0.001)
    return result["chunks"], {"rust_ms": result["rust_ms"], "python_ms": python_ms}


def stream_chunk_odf(
    file_path: str,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
):
    """Stream ODF chunks as an iterator of chunk dicts."""
    path = Path(file_path)
    _validate(file_path, path, mode)
    return _rust.stream_odf_chunks(
        str(path), mode, window_size, overlap, sentences_per_chunk, paragraphs_per_page
    )


def _validate_path_only(file_path: str, path: Path) -> None:
    if not path.is_file():
        raise FileNotFoundError(f"ODF file not found: {file_path}")
    if path.suffix.lower() not in _ODF_SUFFIXES:
        raise ValueError(f"Expected a .odt or .odp file, got: {file_path}")


def odf_to_markdown(file_path: str) -> str:
    """Convert a `.odt`/`.odp` file to a Markdown string."""
    path = Path(file_path)
    _validate_path_only(file_path, path)
    return _rust.odf_to_markdown(str(path))


def chunk_odf_with_images(
    file_path: str,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
) -> tuple[list[dict], dict[str, bytes]]:
    """Chunk an ODF file and extract its embedded images as dedicated chunks."""
    path = Path(file_path)
    normalized = "default" if mode == "default" else mode
    _validate(file_path, path, normalized)
    chunk_list, image_list = _rust.chunk_odf_with_images(
        str(path), normalized, 1, window_size, overlap,
        sentences_per_chunk, paragraphs_per_page, 2000,
    )
    return chunk_list, dict(image_list)


def odf_to_markdown_with_images(file_path: str) -> tuple[str, dict[str, bytes]]:
    """Convert an ODF file to Markdown and extract its embedded images."""
    path = Path(file_path)
    _validate_path_only(file_path, path)
    md, image_list = _rust.odf_to_markdown_with_images(str(path))
    return md, dict(image_list)
