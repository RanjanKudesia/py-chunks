"""Markdown chunker wrapper over the Rust extension."""

import time
from pathlib import Path

from py_chunks import _rust

_MD_MODES = {
    "default", "structural", "semantic", "section",
    "sliding_window", "sentence", "page_aware",
}
_MD_STREAMING_MODES = _MD_MODES  # all modes support streaming


def chunk_md(
    file_path: str,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
) -> tuple[list[dict], dict]:
    """Chunk a Markdown file using the Rust extension module.

    Args:
        file_path: Path to the .md file.
        mode: Chunking mode.  One of:
              ``"default"`` / ``"structural"`` — one chunk per block element.
              ``"semantic"``     — topic-continuity grouping with 10 signals.
              ``"section"``      — all content under a heading in one chunk.
              ``"sliding_window"`` — overlapping paragraph windows.
              ``"sentence"``     — N sentences per chunk.
              ``"page_aware"``   — heading-boundary or paragraph-count groups.
        window_size: Blocks per window (``sliding_window`` only, default 3).
        overlap: Overlapping blocks between windows (default 1).
        sentences_per_chunk: Sentences per chunk (``sentence`` only, default 3).
        paragraphs_per_page: Block count before a page flush (``page_aware``
                             only, default 15).

    Returns:
        (chunks, timing) where timing has ``rust_ms`` and ``python_ms`` keys.
    """
    path = Path(file_path)
    if not path.is_file():
        raise FileNotFoundError(f"MD file not found: {file_path}")
    if path.suffix.lower() != ".md":
        raise ValueError(f"Expected a .md file, got: {file_path}")
    if mode not in _MD_MODES:
        raise ValueError(
            f"mode must be one of {sorted(_MD_MODES)} for MD, got: '{mode}'"
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
        result = _rust.chunk_md_semantic(path_str)
    elif mode == "section":
        result = _rust.chunk_md_section(path_str)
    elif mode == "sliding_window":
        result = _rust.chunk_md_sliding_window(path_str, window_size, overlap)
    elif mode == "sentence":
        result = _rust.chunk_md_sentence(path_str, sentences_per_chunk)
    elif mode == "page_aware":
        result = _rust.chunk_md_page_aware(path_str, paragraphs_per_page)
    else:
        result = _rust.chunk_md(path_str)

    python_ms = round((time.perf_counter() - py_start) * 1000, 3)
    return result["chunks"], {"rust_ms": result["rust_ms"], "python_ms": python_ms}


def stream_chunk_md(
    file_path: str,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
):
    """Stream chunks from a Markdown file as an iterator.

    All 6 modes are supported for streaming. ``structural`` and ``semantic``
    use a true block-by-block state machine (O(blocks) memory, not
    O(chunks)).  The remaining modes (``section``, ``sliding_window``,
    ``sentence``, ``page_aware``) compute chunks upfront and drain them
    lazily — identical output to the batch equivalent, one chunk per
    iteration step.

    Args:
        file_path:           Path to the .md file.
        mode:                Any of the 6 MD chunking modes.
        window_size:         Blocks per window (``sliding_window`` only).
        overlap:             Overlapping blocks between windows.
        sentences_per_chunk: Sentences per chunk (``sentence`` only).
        paragraphs_per_page: Block quota before a flush (``page_aware``).

    Returns:
        Iterator that yields chunk dicts with keys: content, content_type,
        metadata.
    """
    path = Path(file_path)
    if not path.is_file():
        raise FileNotFoundError(f"MD file not found: {file_path}")
    if path.suffix.lower() != ".md":
        raise ValueError(f"Expected a .md file, got: {file_path}")
    if mode not in _MD_STREAMING_MODES:
        raise ValueError(
            f"mode must be one of {sorted(_MD_STREAMING_MODES)} for MD streaming, "
            f"got: '{mode}'"
        )

    return _rust.stream_md_chunks(
        str(path),
        mode,
        window_size,
        overlap,
        sentences_per_chunk,
        paragraphs_per_page,
    )


def md_to_markdown(file_path: str) -> str:
    """Return the contents of a Markdown file as a Markdown string.

    Markdown files are already in the target format, so the file is returned as-is.
    """
    path = Path(file_path)
    if not path.is_file():
        raise FileNotFoundError(f"Markdown file not found: {file_path}")
    if path.suffix.lower() != ".md":
        raise ValueError(f"Expected a .md file, got: {file_path}")
    return path.read_text(encoding="utf-8", errors="replace")
