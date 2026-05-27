"""DOC (Word 97-2003) chunker wrapper over the Rust extension."""

import time
from pathlib import Path

from py_chunks import _rust

_DOC_MODES = {
    "default",
    "structural",
    "section",
    "semantic",
    "sliding_window",
    "sentence",
    "page_aware",
}


def chunk_doc(
    file_path: str,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
) -> tuple[list[dict], dict]:
    """Chunk a .doc (Word 97-2003) file. Returns (chunks, timing)."""
    path = Path(file_path)
    if not path.is_file():
        raise FileNotFoundError(f"DOC file not found: {file_path}")
    if path.suffix.lower() != ".doc":
        raise ValueError(f"Expected a .doc file, got: {file_path}")
    if mode not in _DOC_MODES:
        raise ValueError(f"mode must be one of {sorted(_DOC_MODES)}")

    normalized = "structural" if mode == "default" else mode

    if normalized == "sliding_window":
        if window_size <= 0:
            raise ValueError("window_size must be > 0")
        if overlap >= window_size:
            raise ValueError("overlap must be less than window_size")
    if normalized == "sentence" and sentences_per_chunk <= 0:
        raise ValueError("sentences_per_chunk must be > 0")
    if normalized == "page_aware" and paragraphs_per_page <= 0:
        raise ValueError("paragraphs_per_page must be > 0")

    py_start = time.perf_counter()
    path_str = str(path)

    if normalized == "section":
        result = _rust.chunk_doc_section(path_str)
    elif normalized == "semantic":
        result = _rust.chunk_doc_semantic(path_str)
    elif normalized == "sliding_window":
        result = _rust.chunk_doc_sliding_window(path_str, window_size, overlap)
    elif normalized == "sentence":
        result = _rust.chunk_doc_sentence(path_str, sentences_per_chunk)
    elif normalized == "page_aware":
        result = _rust.chunk_doc_page_aware(path_str, paragraphs_per_page)
    else:
        result = _rust.chunk_doc(path_str)

    python_ms = round((time.perf_counter() - py_start) * 1000, 3)
    return result["chunks"], {"rust_ms": result["rust_ms"], "python_ms": python_ms}


def stream_chunk_doc(
    file_path: str,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
):
    """Stream chunks from a .doc file as an iterator."""
    path = Path(file_path)
    if not path.is_file():
        raise FileNotFoundError(f"DOC file not found: {file_path}")
    if path.suffix.lower() != ".doc":
        raise ValueError(f"Expected a .doc file, got: {file_path}")

    normalized = "structural" if mode == "default" else mode

    if normalized == "semantic":
        return _rust.chunk_doc_semantic_stream(str(path))
    if normalized == "section":
        return _rust.chunk_doc_section_stream(str(path))
    if normalized == "sliding_window":
        return _rust.chunk_doc_sliding_window_stream(str(path), window_size, overlap)
    if normalized == "sentence":
        return _rust.chunk_doc_sentence_stream(str(path), sentences_per_chunk)
    if normalized == "page_aware":
        return _rust.chunk_doc_page_aware_stream(str(path), paragraphs_per_page)
    return _rust.chunk_doc_structural_stream(str(path))


def doc_to_markdown(file_path: str) -> str:
    """Convert a .doc (Word 97-2003) file to a Markdown string."""
    path = Path(file_path)
    if not path.is_file():
        raise FileNotFoundError(f"DOC file not found: {file_path}")
    if path.suffix.lower() != ".doc":
        raise ValueError(f"Expected a .doc file, got: {file_path}")
    return _rust.doc_to_markdown(str(path))
