"""DOCX chunker wrapper over the Rust extension."""

import time
from pathlib import Path

from py_chunks import _rust


_DOCX_MODES = {
    "default",
    "structural",
    "section",
    "semantic",
    "sliding_window",
    "sentence",
    "page_aware",
}


def _validate_docx_args(
    path: Path,
    file_path: str,
    mode: str,
    window_size: int,
    overlap: int,
    sentences_per_chunk: int,
    paragraphs_per_page: int,
) -> str:
    if not path.is_file():
        raise FileNotFoundError(f"DOCX file not found: {file_path}")
    if path.suffix.lower() != ".docx":
        raise ValueError(f"Expected a .docx file, got: {file_path}")
    if mode not in _DOCX_MODES:
        raise ValueError(
            "mode must be 'default', 'structural', 'section', 'semantic', 'sliding_window', 'sentence', or 'page_aware'"
        )

    normalized_mode = "structural" if mode == "default" else mode

    if normalized_mode == "sliding_window" and overlap >= window_size:
        raise ValueError("overlap must be less than window_size")
    if normalized_mode == "sentence" and sentences_per_chunk <= 0:
        raise ValueError("sentences_per_chunk must be greater than 0")
    if normalized_mode == "page_aware" and paragraphs_per_page <= 0:
        raise ValueError("paragraphs_per_page must be greater than 0")

    return normalized_mode


def _run_docx_mode(
    path: Path,
    mode: str,
    window_size: int,
    overlap: int,
    sentences_per_chunk: int,
    paragraphs_per_page: int,
):
    path_str = str(path)
    if mode == "section":
        return _rust.chunk_docx_section(path_str)
    if mode == "semantic":
        return _rust.chunk_docx_semantic(path_str)
    if mode == "sliding_window":
        return _rust.chunk_docx_sliding_window(path_str, window_size, overlap)
    if mode == "sentence":
        return _rust.chunk_docx_sentence(path_str, sentences_per_chunk)
    if mode == "page_aware":
        return _rust.chunk_docx_page_aware(path_str, paragraphs_per_page)
    return _rust.chunk_docx(path_str)


def chunk_docx(
    file_path: str,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
) -> tuple[list[dict], dict]:
    """Chunk a DOCX file using the Rust extension module.

    Returns:
        (chunks, timing) where:
          - chunks: list of dicts with keys content, content_type, metadata
          - timing: dict with keys
              rust_ms   — parse + build time measured inside Rust
              python_ms — full trip time measured in Python (includes file I/O + Rust call)
    """
    path = Path(file_path)
    mode = _validate_docx_args(
        path,
        file_path,
        mode,
        window_size,
        overlap,
        sentences_per_chunk,
        paragraphs_per_page,
    )

    py_start = time.perf_counter()
    result = _run_docx_mode(
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


def stream_chunk_docx(
    file_path: str,
    mode: str = "default",
) -> object:
    """Stream chunks from a DOCX file as an iterator.

    Returns an iterator that yields chunks one at a time.
    Supports mode="default" and mode="structural" (equivalent behavior).

    Args:
        file_path: Path to the DOCX file.
        mode: Chunking mode. "default" and "structural" are supported for streaming.

    Returns:
        Iterator that yields chunk dicts with keys: content, content_type, metadata.

    Raises:
        FileNotFoundError: If the file does not exist.
        ValueError: If the file extension is not .docx.
        NotImplementedError: If mode is not "default" or "structural".
    """
    path = Path(file_path)
    if not path.is_file():
        raise FileNotFoundError(f"DOCX file not found: {file_path}")
    if path.suffix.lower() != ".docx":
        raise ValueError(f"Expected a .docx file, got: {file_path}")

    if mode not in {"default", "structural"}:
        raise NotImplementedError(f"Streaming for {mode} mode coming soon")

    return _rust.chunk_docx_structural_stream(str(path))
