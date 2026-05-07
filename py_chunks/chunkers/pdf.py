"""PDF chunker wrapper over the Rust extension."""

import time
from pathlib import Path

from py_chunks import _rust


def chunk_pdf(file_path: str) -> tuple[list[dict], dict]:
    """Chunk a PDF file using the Rust extension module.

    Returns:
        (chunks, timing) where:
          - chunks: list of dicts with keys content, content_type, metadata
          - timing: dict with keys
              rust_ms   - extract + chunking pipeline measured inside Rust
              python_ms - full trip time measured in Python
    """
    path = Path(file_path)
    if not path.is_file():
        raise FileNotFoundError(f"PDF file not found: {file_path}")
    if path.suffix.lower() != ".pdf":
        raise ValueError(f"Expected a .pdf file, got: {file_path}")

    py_start = time.perf_counter()
    result = _rust.chunk_pdf(str(path))
    python_ms = round((time.perf_counter() - py_start) * 1000, 3)

    timing = {
        "rust_ms": result["rust_ms"],
        "python_ms": python_ms,
    }
    return result["chunks"], timing
