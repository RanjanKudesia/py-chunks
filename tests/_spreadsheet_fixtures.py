"""Shared helpers for the calamine-backed spreadsheet format test suites.

Tests are driven off the real downloaded fixtures under ``<repo>/../test_files``.
If that directory is absent (e.g. a packaging-only checkout), the format tests
skip rather than fail.
"""

from pathlib import Path

import pytest

from py_chunks import get_chunks, get_chunks_from_bytes, get_markdown, stream_chunks
from py_chunks.chunkers.xlsx import chunk_xlsx

_TEST_FILES = Path(__file__).resolve().parents[2] / "test_files"

STANDARD_KEYS = {"content", "content_type", "metadata"}

# Every mode the xlsx chunker exposes.
XLSX_MODES = ["row", "table", "sheet", "sliding_window", "page_aware", "semantic"]


def fixtures(subdir: str, pattern: str) -> list[Path]:
    folder = _TEST_FILES / subdir
    if not folder.is_dir():
        return []
    return sorted(folder.glob(pattern))


def require(files: list[Path]) -> None:
    if not files:
        pytest.skip("real spreadsheet fixtures not present in ../test_files")


def assert_schema(chunks: list[dict]) -> None:
    for c in chunks:
        assert STANDARD_KEYS.issubset(c.keys())
        assert isinstance(c["content"], str)
        assert isinstance(c["metadata"], dict)


def parse_or_clean_error(path: Path):
    """Return chunks on success, or None if the file raised a *catchable*
    Exception. Re-raises BaseException (e.g. a leaked Rust panic) so a process
    abort or uncatchable error fails the test loudly.
    """
    try:
        return get_chunks(str(path))
    except Exception:  # noqa: BLE001 — the contract is "clean, catchable error"
        return None
