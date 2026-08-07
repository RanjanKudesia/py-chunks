"""JSON-family chunker — `.json` (single document), `.jsonl` / `.ndjson`
(line-delimited records). Records are rendered to markdown by the Rust extension
and chunked through the Markdown pipeline. Mirrors the `.eml`/`.odf` chunkers."""

import time
from pathlib import Path

from py_chunks import _rust

_JSON_MODES = {"default", "structural", "section", "semantic",
               "sliding_window", "sentence", "page_aware"}
_JSON_SUFFIXES = {".json", ".jsonl", ".ndjson"}


def _validate_mode(mode: str) -> None:
    """Path-independent half of the validation — shared with the bytes route."""
    if mode not in _JSON_MODES:
        raise ValueError(f"mode must be one of {sorted(_JSON_MODES)} for JSON, got: '{mode}'")


def _validate(file_path: str, path: Path, mode: str) -> None:
    if not path.is_file():
        raise FileNotFoundError(f"JSON file not found: {file_path}")
    if path.suffix.lower() not in _JSON_SUFFIXES:
        raise ValueError(f"Expected a .json, .jsonl or .ndjson file, got: {file_path}")
    _validate_mode(mode)


def chunk_json(
    file_path: str,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
) -> tuple[list[dict], dict]:
    """Chunk a `.json` document or `.jsonl`/`.ndjson` record stream."""
    path = Path(file_path)
    _validate(file_path, path, mode)
    p = str(path)
    py_start = time.perf_counter()
    if mode == "section":
        result = _rust.chunk_json_section(p)
    elif mode == "semantic":
        result = _rust.chunk_json_semantic(p)
    elif mode == "sentence":
        result = _rust.chunk_json_sentence(p, sentences_per_chunk)
    elif mode == "page_aware":
        result = _rust.chunk_json_page_aware(p, paragraphs_per_page)
    elif mode == "sliding_window":
        result = _rust.chunk_json_sliding_window(p, window_size, overlap)
    else:
        result = _rust.chunk_json(p)
    python_ms = max(round((time.perf_counter() - py_start) * 1000, 3), 0.001)
    return result["chunks"], {"rust_ms": result["rust_ms"], "python_ms": python_ms}


def stream_chunk_json(
    file_path: str,
    mode: str = "default",
    window_size: int = 3,
    overlap: int = 1,
    sentences_per_chunk: int = 3,
    paragraphs_per_page: int = 15,
):
    """Stream JSON record chunks as an iterator of chunk dicts."""
    path = Path(file_path)
    _validate(file_path, path, mode)
    return _rust.stream_json_chunks(
        str(path), mode, window_size, overlap, sentences_per_chunk, paragraphs_per_page
    )


def json_to_markdown(file_path: str) -> str:
    """Convert a `.json`/`.jsonl`/`.ndjson` file to a Markdown string."""
    path = Path(file_path)
    if not path.is_file():
        raise FileNotFoundError(f"JSON file not found: {file_path}")
    if path.suffix.lower() not in _JSON_SUFFIXES:
        raise ValueError(f"Expected a .json, .jsonl or .ndjson file, got: {file_path}")
    return _rust.json_to_markdown(str(path))
