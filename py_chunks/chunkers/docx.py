"""DOCX chunker wrapper over the Rust extension."""

import time
from pathlib import Path

from py_chunks import _rust


def chunk_docx(
    file_path: str,
    mode: str = "structural",
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
    if not path.is_file():
        raise FileNotFoundError(f"DOCX file not found: {file_path}")
    if path.suffix.lower() != ".docx":
        raise ValueError(f"Expected a .docx file, got: {file_path}")
    if mode not in {
        "structural",
        "section",
        "semantic",
        "sliding_window",
        "sentence",
        "page_aware",
    }:
        raise ValueError(
            "mode must be 'structural', 'section', 'semantic', 'sliding_window', 'sentence', or 'page_aware'"
        )
    if mode == "sliding_window" and overlap >= window_size:
        raise ValueError("overlap must be less than window_size")
    if mode == "sentence" and sentences_per_chunk <= 0:
        raise ValueError("sentences_per_chunk must be greater than 0")
    if mode == "page_aware" and paragraphs_per_page <= 0:
        raise ValueError("paragraphs_per_page must be greater than 0")

    py_start = time.perf_counter()
    if mode == "section":
        result = _rust.chunk_docx_section(str(path))
    elif mode == "semantic":
        result = _rust.chunk_docx_semantic(str(path))
    elif mode == "sliding_window":
        result = _rust.chunk_docx_sliding_window(
            str(path), window_size, overlap)
    elif mode == "sentence":
        result = _rust.chunk_docx_sentence(str(path), sentences_per_chunk)
    elif mode == "page_aware":
        result = _rust.chunk_docx_page_aware(str(path), paragraphs_per_page)
    else:
        result = _rust.chunk_docx(str(path))
    python_ms = round((time.perf_counter() - py_start) * 1000, 3)

    timing = {
        "rust_ms": result["rust_ms"],
        "python_ms": python_ms,
    }
    return result["chunks"], timing


def stream_chunk_docx(
    file_path: str,
    mode: str = "structural",
) -> object:
    """Stream chunks from a DOCX file as an iterator.

    Returns an iterator that yields chunks one at a time.
    Currently only supports mode="structural".

    Args:
        file_path: Path to the DOCX file.
        mode: Chunking mode. Currently only "structural" is supported for streaming.

    Returns:
        Iterator that yields chunk dicts with keys: content, content_type, metadata.

    Raises:
        FileNotFoundError: If the file does not exist.
        ValueError: If the file extension is not .docx.
        NotImplementedError: If mode is not "structural".
    """
    path = Path(file_path)
    if not path.is_file():
        raise FileNotFoundError(f"DOCX file not found: {file_path}")
    if path.suffix.lower() != ".docx":
        raise ValueError(f"Expected a .docx file, got: {file_path}")

    if mode != "structural":
        raise NotImplementedError(f"Streaming for {mode} mode coming soon")

    return _rust.chunk_docx_structural_stream(str(path))
    return _rust.chunk_docx_true_stream(str(path))
