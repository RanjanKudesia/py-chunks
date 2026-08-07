"""Outlook .msg chunker — MAPI extraction (envelope + body + attachments) via
the Rust extension, chunked through the Markdown pipeline."""

import time
from pathlib import Path

from py_chunks import _rust

_MSG_MODES = {"default", "structural", "section", "semantic",
              "sliding_window", "sentence", "page_aware"}


def _validate_mode(mode: str) -> None:
    """Path-independent half of the validation — shared with the bytes route."""
    if mode not in _MSG_MODES:
        raise ValueError(f"mode must be one of {sorted(_MSG_MODES)} for MSG, got: '{mode}'")


def _validate(file_path: str, path: Path, mode: str) -> None:
    if not path.is_file():
        raise FileNotFoundError(f"MSG file not found: {file_path}")
    if path.suffix.lower() != ".msg":
        raise ValueError(f"Expected a .msg file, got: {file_path}")
    _validate_mode(mode)


def chunk_msg(
    file_path: str,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
) -> tuple[list[dict], dict]:
    """Chunk an Outlook .msg message. Modes mirror the document chunkers."""
    path = Path(file_path)
    _validate(file_path, path, mode)
    p = str(path)
    py_start = time.perf_counter()
    if mode == "section":
        result = _rust.chunk_msg_section(p)
    elif mode == "semantic":
        result = _rust.chunk_msg_semantic(p)
    elif mode == "sentence":
        result = _rust.chunk_msg_sentence(p, sentences_per_chunk)
    elif mode == "page_aware":
        result = _rust.chunk_msg_page_aware(p, paragraphs_per_page)
    elif mode == "sliding_window":
        result = _rust.chunk_msg_sliding_window(p, window_size, overlap)
    else:
        result = _rust.chunk_msg(p)
    python_ms = max(round((time.perf_counter() - py_start) * 1000, 3), 0.001)
    return result["chunks"], {"rust_ms": result["rust_ms"], "python_ms": python_ms}


def stream_chunk_msg(
    file_path: str,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
):
    """Stream .msg chunks as an iterator of chunk dicts."""
    path = Path(file_path)
    _validate(file_path, path, mode)
    return _rust.stream_msg_chunks(
        str(path), mode, window_size, overlap, sentences_per_chunk, paragraphs_per_page
    )


def msg_to_markdown(file_path: str) -> str:
    """Convert an Outlook .msg to a Markdown string (envelope + body + attachments)."""
    path = Path(file_path)
    if not path.is_file():
        raise FileNotFoundError(f"MSG file not found: {file_path}")
    if path.suffix.lower() != ".msg":
        raise ValueError(f"Expected a .msg file, got: {file_path}")
    return _rust.msg_to_markdown(str(path))


def chunk_msg_with_images(
    file_path: str,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
) -> tuple[list[dict], dict[str, bytes]]:
    """Chunk a .msg and extract its image attachments as dedicated chunks.

    Mirrors ``chunk_eml_with_images``. ``list_images=True`` used to be a silent
    no-op for ``.msg`` even when the message carried real image attachments.
    """
    path = Path(file_path)
    normalized = "default" if mode == "default" else mode
    _validate(file_path, path, normalized)
    chunk_list, image_list = _rust.chunk_msg_with_images(
        str(path), normalized, 1, window_size, overlap,
        sentences_per_chunk, paragraphs_per_page, 2000,
    )
    return chunk_list, dict(image_list)


def msg_to_markdown_with_images(file_path: str) -> tuple[str, dict[str, bytes]]:
    """Convert a .msg to Markdown and extract its image attachments."""
    path = Path(file_path)
    if not path.is_file():
        raise FileNotFoundError(f"MSG file not found: {file_path}")
    if path.suffix.lower() != ".msg":
        raise ValueError(f"Expected a .msg file, got: {file_path}")
    md, image_list = _rust.msg_to_markdown_with_images(str(path))
    return md, dict(image_list)
