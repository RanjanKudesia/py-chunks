"""MIME email chunker — `.eml` (single RFC822 message) and `.mbox` (mailbox of
messages). Parsed via the Rust extension (mail-parser), assembled into markdown,
and chunked through the Markdown pipeline. Mirrors the `.msg` chunker."""

import time
from pathlib import Path

from py_chunks import _rust

_EML_MODES = {"default", "structural", "section", "semantic",
              "sliding_window", "sentence", "page_aware"}
_EML_SUFFIXES = {".eml", ".mbox"}


def _validate(file_path: str, path: Path, mode: str) -> None:
    if not path.is_file():
        raise FileNotFoundError(f"Email file not found: {file_path}")
    if path.suffix.lower() not in _EML_SUFFIXES:
        raise ValueError(f"Expected a .eml or .mbox file, got: {file_path}")
    if mode not in _EML_MODES:
        raise ValueError(f"mode must be one of {sorted(_EML_MODES)} for email, got: '{mode}'")


def chunk_eml(
    file_path: str,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
) -> tuple[list[dict], dict]:
    """Chunk a `.eml` message or `.mbox` mailbox. Modes mirror the document chunkers."""
    path = Path(file_path)
    _validate(file_path, path, mode)
    p = str(path)
    py_start = time.perf_counter()
    if mode == "section":
        result = _rust.chunk_eml_section(p)
    elif mode == "semantic":
        result = _rust.chunk_eml_semantic(p)
    elif mode == "sentence":
        result = _rust.chunk_eml_sentence(p, sentences_per_chunk)
    elif mode == "page_aware":
        result = _rust.chunk_eml_page_aware(p, paragraphs_per_page)
    elif mode == "sliding_window":
        result = _rust.chunk_eml_sliding_window(p, window_size, overlap)
    else:
        result = _rust.chunk_eml(p)
    python_ms = max(round((time.perf_counter() - py_start) * 1000, 3), 0.001)
    return result["chunks"], {"rust_ms": result["rust_ms"], "python_ms": python_ms}


def stream_chunk_eml(
    file_path: str,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
):
    """Stream email chunks as an iterator of chunk dicts."""
    path = Path(file_path)
    _validate(file_path, path, mode)
    return _rust.stream_eml_chunks(
        str(path), mode, window_size, overlap, sentences_per_chunk, paragraphs_per_page
    )


def _validate_path_only(file_path: str, path: Path) -> None:
    if not path.is_file():
        raise FileNotFoundError(f"Email file not found: {file_path}")
    if path.suffix.lower() not in _EML_SUFFIXES:
        raise ValueError(f"Expected a .eml or .mbox file, got: {file_path}")


def eml_to_markdown(file_path: str) -> str:
    """Convert a `.eml`/`.mbox` file to a Markdown string."""
    path = Path(file_path)
    _validate_path_only(file_path, path)
    return _rust.eml_to_markdown(str(path))


def chunk_eml_with_images(
    file_path: str,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
) -> tuple[list[dict], dict[str, bytes]]:
    """Chunk an email and extract its inline/attached images as dedicated chunks."""
    path = Path(file_path)
    normalized = "default" if mode == "default" else mode
    _validate(file_path, path, normalized)
    chunk_list, image_list = _rust.chunk_eml_with_images(
        str(path), normalized, 1, window_size, overlap,
        sentences_per_chunk, paragraphs_per_page, 2000,
    )
    return chunk_list, dict(image_list)


def eml_to_markdown_with_images(file_path: str) -> tuple[str, dict[str, bytes]]:
    """Convert an email to Markdown and extract its inline/attached images."""
    path = Path(file_path)
    _validate_path_only(file_path, path)
    md, image_list = _rust.eml_to_markdown_with_images(str(path))
    return md, dict(image_list)
