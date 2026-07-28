"""RTF chunker — spec-correct hand-rolled text extraction via the Rust
extension, chunked through the Markdown pipeline."""

import time
from pathlib import Path

from py_chunks import _rust

_RTF_MODES = {"default", "structural", "section", "semantic",
              "sliding_window", "sentence", "page_aware"}


def _validate(file_path: str, path: Path, mode: str) -> None:
    if not path.is_file():
        raise FileNotFoundError(f"RTF file not found: {file_path}")
    if path.suffix.lower() != ".rtf":
        raise ValueError(f"Expected a .rtf file, got: {file_path}")
    if mode not in _RTF_MODES:
        raise ValueError(f"mode must be one of {sorted(_RTF_MODES)} for RTF, got: '{mode}'")


def chunk_rtf(
    file_path: str,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
) -> tuple[list[dict], dict]:
    """Chunk an RTF document. Modes mirror the document chunkers."""
    path = Path(file_path)
    _validate(file_path, path, mode)
    p = str(path)
    py_start = time.perf_counter()
    if mode == "section":
        result = _rust.chunk_rtf_section(p)
    elif mode == "semantic":
        result = _rust.chunk_rtf_semantic(p)
    elif mode == "sentence":
        result = _rust.chunk_rtf_sentence(p, sentences_per_chunk)
    elif mode == "page_aware":
        result = _rust.chunk_rtf_page_aware(p, paragraphs_per_page)
    elif mode == "sliding_window":
        result = _rust.chunk_rtf_sliding_window(p, window_size, overlap)
    else:
        result = _rust.chunk_rtf(p)
    python_ms = max(round((time.perf_counter() - py_start) * 1000, 3), 0.001)
    return result["chunks"], {"rust_ms": result["rust_ms"], "python_ms": python_ms}


def stream_chunk_rtf(
    file_path: str,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
):
    """Stream RTF chunks as an iterator of chunk dicts."""
    path = Path(file_path)
    _validate(file_path, path, mode)
    return _rust.stream_rtf_chunks(
        str(path), mode, window_size, overlap, sentences_per_chunk, paragraphs_per_page
    )


def rtf_to_markdown(file_path: str) -> str:
    """Convert an RTF document to a Markdown string."""
    path = Path(file_path)
    if not path.is_file():
        raise FileNotFoundError(f"RTF file not found: {file_path}")
    if path.suffix.lower() != ".rtf":
        raise ValueError(f"Expected a .rtf file, got: {file_path}")
    return _rust.rtf_to_markdown(str(path))
