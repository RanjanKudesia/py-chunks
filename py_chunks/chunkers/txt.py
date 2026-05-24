"""TXT chunker wrapper over the Rust extension."""

import time
from pathlib import Path

from py_chunks import _rust

_TXT_MODES = {
    "default", "structural", "semantic", "section",
    "sliding_window", "sentence", "page_aware",
}


def chunk_txt(
    file_path: str,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
) -> tuple[list[dict], dict]:
    """Chunk a plain-text file using the Rust extension module.

    Args:
        file_path: Path to the .txt file.
        mode: Chunking mode. One of:
              ``"default"`` / ``"structural"`` — one chunk per detected block.
              ``"semantic"``      — topic-continuity grouping (10 signals).
              ``"section"``       — all blocks under a heading in one chunk.
              ``"sliding_window"``— overlapping block windows.
              ``"sentence"``      — N sentences per chunk.
              ``"page_aware"``    — heading-boundary or paragraph-count groups.
        window_size: Blocks per window (``sliding_window`` only, default 3).
        overlap: Overlapping blocks (``sliding_window`` only, default 1).
        sentences_per_chunk: Sentences per chunk (``sentence`` only, default 3).
        paragraphs_per_page: Block quota before flush (``page_aware``, default 15).

    Returns:
        (chunks, timing) where timing has ``rust_ms`` and ``python_ms`` keys.
    """
    path = Path(file_path)
    if not path.is_file():
        raise FileNotFoundError(f"TXT file not found: {file_path}")
    if path.suffix.lower() != ".txt":
        raise ValueError(f"Expected a .txt file, got: {file_path}")
    if mode not in _TXT_MODES:
        raise ValueError(
            f"mode must be one of {sorted(_TXT_MODES)} for TXT, got: '{mode}'"
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

    py_start = time.perf_counter()
    path_str = str(path)

    if mode == "semantic":
        result = _rust.chunk_txt_semantic(path_str)
    elif mode == "section":
        result = _rust.chunk_txt_section(path_str)
    elif mode == "sliding_window":
        result = _rust.chunk_txt_sliding_window(path_str, window_size, overlap)
    elif mode == "sentence":
        result = _rust.chunk_txt_sentence(path_str, sentences_per_chunk)
    elif mode == "page_aware":
        result = _rust.chunk_txt_page_aware(path_str, paragraphs_per_page)
    else:
        result = _rust.chunk_txt(path_str)

    python_ms = round((time.perf_counter() - py_start) * 1000, 3)
    return result["chunks"], {"rust_ms": result["rust_ms"], "python_ms": python_ms}


def stream_chunk_txt(
    file_path: str,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
):
    """Stream chunks from a plain-text file as an iterator.

    All 6 modes are supported.  ``structural`` and ``semantic`` use a true
    block-by-block state machine.  The remaining modes compute all chunks
    upfront and drain them one per iteration step.

    Returns:
        Iterator yielding chunk dicts: content, content_type, metadata.
    """
    path = Path(file_path)
    if not path.is_file():
        raise FileNotFoundError(f"TXT file not found: {file_path}")
    if path.suffix.lower() != ".txt":
        raise ValueError(f"Expected a .txt file, got: {file_path}")
    if mode not in _TXT_MODES:
        raise ValueError(
            f"mode must be one of {sorted(_TXT_MODES)} for TXT streaming, got: '{mode}'"
        )
    return _rust.stream_txt_chunks(
        str(path), mode, window_size, overlap, sentences_per_chunk, paragraphs_per_page,
    )
