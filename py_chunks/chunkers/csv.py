"""CSV chunker wrapper over the Rust extension."""

from __future__ import annotations

import time
from pathlib import Path

from py_chunks import _rust

_CSV_MODES = {"row", "default", "sliding_window", "page_aware"}
_CSV_SUFFIXES = (".csv", ".tsv")


def _parse_delimiter(delimiter: str | None) -> int | None:
    if delimiter is None:
        return None
    if delimiter == ",":
        return ord(",")
    if delimiter == "\t":
        return ord("\t")
    if delimiter == ";":
        return ord(";")
    if delimiter == "|":
        return ord("|")
    raise ValueError("delimiter must be one of None, ',', '\\t', ';', or '|' for CSV")


def _validate_csv_options(
    mode: str,
    rows_per_chunk: int,
    window_size: int,
    overlap: int,
    encoding: str,
) -> None:
    """Path-independent half of the CSV validation — also used by the
    source-agnostic bytes entry points, so the messages stay identical."""
    if mode not in _CSV_MODES:
        raise ValueError(f"mode must be one of {sorted(_CSV_MODES)} for CSV, got: '{mode}'")
    if rows_per_chunk < 1:
        raise ValueError("rows_per_chunk must be greater than 0")
    if window_size < 1:
        raise ValueError("window_size must be greater than 0")
    if overlap >= window_size:
        raise ValueError("overlap must be less than window_size")
    if encoding.lower() not in {"utf-8", "utf-8-bom", "latin-1", "windows-1252"}:
        raise ValueError(
            "encoding must be one of 'utf-8', 'utf-8-bom', 'latin-1', or 'windows-1252'"
        )


def _validate_csv_args(
    file_path: str,
    path: Path,
    mode: str,
    rows_per_chunk: int,
    window_size: int,
    overlap: int,
    encoding: str,
) -> None:
    if not path.is_file():
        raise FileNotFoundError(f"CSV file not found: {file_path}")
    if path.suffix.lower() not in _CSV_SUFFIXES:
        raise ValueError(
            f"Expected one of {', '.join(_CSV_SUFFIXES)}, got: {file_path}")
    _validate_csv_options(mode, rows_per_chunk, window_size, overlap, encoding)


def chunk_csv(
    file_path: str,
    mode: str = "row",
    rows_per_chunk: int = 10,
    window_size: int = 5,
    overlap: int = 1,
    include_headers: bool = True,
    delimiter: str | None = None,
    encoding: str = "utf-8",
    skip_empty_rows: bool = True,
) -> tuple[list[dict], dict]:
    path = Path(file_path)
    _validate_csv_args(file_path, path, mode, rows_per_chunk, window_size, overlap, encoding)

    py_start = time.perf_counter()
    result = _rust.chunk_csv(
        str(path),
        mode=mode,
        rows_per_chunk=rows_per_chunk,
        window_size=window_size,
        overlap=overlap,
        include_headers=include_headers,
        delimiter=_parse_delimiter(delimiter),
        encoding=encoding,
        skip_empty_rows=skip_empty_rows,
    )
    python_ms = max(round((time.perf_counter() - py_start) * 1000, 3), 0.001)
    return result["chunks"], {"rust_ms": result["rust_ms"], "python_ms": python_ms}


def stream_chunk_csv(
    file_path: str,
    mode: str = "row",
    rows_per_chunk: int = 10,
    window_size: int = 5,
    overlap: int = 1,
    include_headers: bool = True,
    delimiter: str | None = None,
    encoding: str = "utf-8",
    skip_empty_rows: bool = True,
):
    path = Path(file_path)
    _validate_csv_args(file_path, path, mode, rows_per_chunk, window_size, overlap, encoding)
    return _rust.stream_csv_chunks(
        str(path),
        mode,
        rows_per_chunk,
        window_size,
        overlap,
        include_headers,
        _parse_delimiter(delimiter),
        encoding,
        skip_empty_rows,
    )


def csv_to_markdown(
    file_path: str,
    delimiter: str | None = None,
    encoding: str = "utf-8",
) -> str:
    """Convert a CSV file to a Markdown pipe table.

    Args:
        file_path: Path to the .csv file.
        delimiter: Column separator. ``None`` (default) = auto-detect from the
                   first data line. Accepts ``','``, ``'\\t'``, ``';'``, ``'|'``.
        encoding:  File encoding. One of ``"utf-8"`` (default), ``"utf-8-bom"``,
                   ``"latin-1"``, ``"windows-1252"``.

    Returns:
        Markdown pipe table string with the first CSV row as the header row.
    """
    path = Path(file_path)
    if not path.is_file():
        raise FileNotFoundError(f"CSV file not found: {file_path}")
    if path.suffix.lower() not in _CSV_SUFFIXES:
        raise ValueError(
            f"Expected one of {', '.join(_CSV_SUFFIXES)}, got: {file_path}")
    if delimiter is None and path.suffix.lower() == ".tsv":
        delimiter = "\t"
    return _rust.csv_to_markdown(
        str(path),
        delimiter=_parse_delimiter(delimiter),
        encoding=encoding,
    )
