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

    if normalized_mode == "sliding_window" and window_size <= 0:
        raise ValueError("window_size must be > 0")
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
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
) -> object:
    """Stream chunks from a DOCX file as an iterator.

    Note on streaming semantics: this is *lazy chunk emission*, not
    event-driven XML streaming. The full ``word/document.xml`` is parsed into
    an in-memory ``Vec<DocumentElement>`` before the first ``__next__`` call,
    and chunks are then produced one at a time as the iterator advances.
    Peak memory is therefore approximately ``file_size + element_vec_size``;
    it does not scale down for very large documents. Use it to overlap chunk
    consumption with downstream processing, not to bound peak memory.

    Args:
        file_path: Path to the DOCX file.
        mode: Chunking mode. One of:
              ``"default"`` / ``"structural"`` — one chunk per block element.
              ``"semantic"``      — topic-continuity grouping.
              ``"section"``       — group content under headings.
              ``"sliding_window"``— overlapping block windows.
              ``"sentence"``      — N sentences per chunk.
              ``"page_aware"``    — paragraph-count groups.

    Returns:
        Iterator that yields chunk dicts with keys: content, content_type, metadata.

    Raises:
        FileNotFoundError: If the file does not exist.
        ValueError: If the file extension is not .docx.
        NotImplementedError: If mode is not supported.
    """
    path = Path(file_path)
    if not path.is_file():
        raise FileNotFoundError(f"DOCX file not found: {file_path}")
    if path.suffix.lower() != ".docx":
        raise ValueError(f"Expected a .docx file, got: {file_path}")

    if mode not in {"default", "structural", "semantic", "section", "sliding_window", "sentence", "page_aware"}:
        raise NotImplementedError(f"Streaming for {mode} mode coming soon")

    if mode == "semantic":
        return _rust.chunk_docx_semantic_stream(str(path))

    if mode == "section":
        return _rust.chunk_docx_section_stream(str(path))

    if mode == "sliding_window":
        if window_size <= 0:
            raise ValueError("window_size must be > 0")
        return _rust.chunk_docx_sliding_window_stream(str(path), window_size, overlap)

    if mode == "sentence":
        return _rust.chunk_docx_sentence_stream(str(path), sentences_per_chunk)

    if mode == "page_aware":
        return _rust.chunk_docx_page_aware_stream(str(path), paragraphs_per_page)

    return _rust.chunk_docx_structural_stream(str(path))
