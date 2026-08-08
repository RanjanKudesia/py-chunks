"""PPT (PowerPoint 97-2003) chunker wrapper over the Rust extension."""

from __future__ import annotations

import time
from pathlib import Path

from py_chunks import _rust

_PPT_MODES = {
    "default",
    "structural",
    "section",
    "semantic",
    "sliding_window",
    "sentence",
    "page_aware",
}

# One wording per rejection, shared by every entry point — the same pattern
# `chunkers/pptx.py` uses. The strings are the *engine's* own, so a caller sees
# one sentence per mistake no matter which layer rejected it (this file used to
# say "must be > 0" where the engine says "must be greater than 0").
_E_WINDOW = "window_size must be greater than 0"
_E_OVERLAP = "overlap must be less than window_size"
_E_SENTENCES = "sentences_per_chunk must be greater than 0"
_E_PARAGRAPHS = "paragraphs_per_page must be greater than 0"


def _validate_ppt_options(
    mode: str,
    window_size: int,
    overlap: int,
    sentences_per_chunk: int,
    paragraphs_per_page: int,
) -> None:
    """Raise for argument values the requested mode cannot use.

    Path-independent, so the bytes route shares it — and so do the batch,
    streaming and with-images entry points, which all reject the same argument
    with the same wording.
    """
    if mode not in _PPT_MODES:
        raise ValueError(f"mode must be one of {sorted(_PPT_MODES)}")
    normalized = "structural" if mode == "default" else mode
    if normalized == "sliding_window":
        if window_size <= 0:
            raise ValueError(_E_WINDOW)
        if overlap >= window_size:
            raise ValueError(_E_OVERLAP)
    if normalized == "sentence" and sentences_per_chunk <= 0:
        raise ValueError(_E_SENTENCES)
    if normalized == "page_aware" and paragraphs_per_page <= 0:
        raise ValueError(_E_PARAGRAPHS)


def chunk_ppt(
    file_path: str,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
) -> tuple[list[dict], dict]:
    """Chunk a .ppt (PowerPoint 97-2003) file. Returns (chunks, timing)."""
    path = Path(file_path)
    if not path.is_file():
        raise FileNotFoundError(f"PPT file not found: {file_path}")
    if path.suffix.lower() != ".ppt":
        raise ValueError(f"Expected a .ppt file, got: {file_path}")
    _validate_ppt_options(mode, window_size, overlap, sentences_per_chunk, paragraphs_per_page)

    normalized = "structural" if mode == "default" else mode

    py_start = time.perf_counter()
    path_str = str(path)

    if normalized == "section":
        result = _rust.chunk_ppt_section(path_str)
    elif normalized == "semantic":
        result = _rust.chunk_ppt_semantic(path_str)
    elif normalized == "sliding_window":
        result = _rust.chunk_ppt_sliding_window(path_str, window_size, overlap)
    elif normalized == "sentence":
        result = _rust.chunk_ppt_sentence(path_str, sentences_per_chunk)
    elif normalized == "page_aware":
        result = _rust.chunk_ppt_page_aware(path_str, paragraphs_per_page)
    else:
        result = _rust.chunk_ppt(path_str)

    python_ms = round((time.perf_counter() - py_start) * 1000, 3)
    return result["chunks"], {"rust_ms": result["rust_ms"], "python_ms": python_ms}


def stream_chunk_ppt(
    file_path: str,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
):
    """Stream chunks from a .ppt file as an iterator.

    Raises:
        FileNotFoundError: If the file does not exist.
        ValueError: If the extension is not .ppt, or the mode/arguments are
            invalid.
    """
    path = Path(file_path)
    if not path.is_file():
        raise FileNotFoundError(f"PPT file not found: {file_path}")
    if path.suffix.lower() != ".ppt":
        raise ValueError(f"Expected a .ppt file, got: {file_path}")
    # Streaming skipped this entirely, so a typo'd mode silently streamed
    # *default-mode* chunks instead of raising.
    _validate_ppt_options(mode, window_size, overlap, sentences_per_chunk, paragraphs_per_page)

    normalized = "structural" if mode == "default" else mode

    if normalized == "semantic":
        return _rust.chunk_ppt_semantic_stream(str(path))
    if normalized == "section":
        return _rust.chunk_ppt_section_stream(str(path))
    if normalized == "sliding_window":
        return _rust.chunk_ppt_sliding_window_stream(str(path), window_size, overlap)
    if normalized == "sentence":
        return _rust.chunk_ppt_sentence_stream(str(path), sentences_per_chunk)
    if normalized == "page_aware":
        return _rust.chunk_ppt_page_aware_stream(str(path), paragraphs_per_page)
    return _rust.chunk_ppt_structural_stream(str(path))


def ppt_to_markdown(file_path: str) -> str:
    """Convert a .ppt (PowerPoint 97-2003) file to a Markdown string."""
    path = Path(file_path)
    if not path.is_file():
        raise FileNotFoundError(f"PPT file not found: {file_path}")
    if path.suffix.lower() != ".ppt":
        raise ValueError(f"Expected a .ppt file, got: {file_path}")
    return _rust.ppt_to_markdown(str(path))


def ppt_to_markdown_with_images(file_path: str) -> tuple[str, dict[str, bytes]]:
    """Convert a .ppt file to Markdown and extract embedded images."""
    path = Path(file_path)
    if not path.is_file():
        raise FileNotFoundError(f"PPT file not found: {file_path}")
    if path.suffix.lower() != ".ppt":
        raise ValueError(f"Expected a .ppt file, got: {file_path}")
    md, image_list = _rust.ppt_to_markdown_with_images(str(path))
    return md, dict(image_list)


def chunk_ppt_with_images(
    file_path: str,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
) -> tuple[list[dict], dict[str, bytes]]:
    """Chunk a .ppt file with images extracted as dedicated chunks.

    Returns (chunks, images) where images is a dict[str, bytes] mapping
    hash-named image filenames to their raw bytes.
    """
    path = Path(file_path)
    if not path.is_file():
        raise FileNotFoundError(f"PPT file not found: {file_path}")
    if path.suffix.lower() != ".ppt":
        raise ValueError(f"Expected a .ppt file, got: {file_path}")
    _validate_ppt_options(mode, window_size, overlap, sentences_per_chunk, paragraphs_per_page)

    normalized = "structural" if mode == "default" else mode
    path_str = str(path)

    if normalized == "structural":
        chunk_list, image_list = _rust.chunk_ppt_structural_with_images(path_str)
    elif normalized == "section":
        chunk_list, image_list = _rust.chunk_ppt_section_with_images(path_str)
    elif normalized == "semantic":
        chunk_list, image_list = _rust.chunk_ppt_semantic_with_images(path_str)
    elif normalized == "sliding_window":
        chunk_list, image_list = _rust.chunk_ppt_sliding_window_with_images(
            path_str, window_size, overlap
        )
    elif normalized == "sentence":
        chunk_list, image_list = _rust.chunk_ppt_sentence_with_images(
            path_str, sentences_per_chunk
        )
    elif normalized == "page_aware":
        chunk_list, image_list = _rust.chunk_ppt_page_aware_with_images(
            path_str, paragraphs_per_page
        )
    else:
        raise ValueError(f"Unknown mode: {mode!r}")

    return chunk_list, dict(image_list)
