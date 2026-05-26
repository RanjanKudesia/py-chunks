"""XLSX chunker wrapper over the Rust extension."""

import time
from pathlib import Path

from py_chunks import _rust

_XLSX_MODES = {"row", "table", "sheet", "sliding_window", "page_aware", "semantic"}
_XLSX_SERIALIZERS = {"key_value"}


def _validate_xlsx_args(
    file_path: str,
    path: Path,
    mode: str,
    rows_per_chunk: int,
    window_size: int,
    overlap: int,
    serialize_as: str,
    max_chunk_chars: int,
) -> None:
    if not path.is_file():
        raise FileNotFoundError(f"Spreadsheet file not found: {file_path}")
    if path.suffix.lower() not in (".xlsx", ".xls"):
        raise ValueError(f"Expected a .xlsx or .xls file, got: {file_path}")
    if mode not in _XLSX_MODES:
        raise ValueError(
            f"mode must be one of {sorted(_XLSX_MODES)} for XLSX, got: '{mode}'"
        )
    if rows_per_chunk < 1:
        raise ValueError("rows_per_chunk must be greater than 0")
    if mode == "sliding_window":
        if window_size < 1:
            raise ValueError("window_size must be >= 1")
        if overlap >= window_size:
            raise ValueError("overlap must be less than window_size")
    if max_chunk_chars < 1:
        raise ValueError("max_chunk_chars must be greater than 0")
    if serialize_as not in _XLSX_SERIALIZERS:
        raise ValueError(
            "serialize_as must be one of "
            f"{sorted(_XLSX_SERIALIZERS)} for XLSX, got: '{serialize_as}'"
        )


def chunk_xlsx(
    file_path: str,
    mode: str = "row",
    rows_per_chunk: int = 1,
    window_size: int = 3,
    overlap: int = 1,
    include_headers: bool = True,
    sheet_names: list[str] | None = None,
    skip_empty_rows: bool = True,
    serialize_as: str = "key_value",
    max_chunk_chars: int = 2000,
) -> tuple[list[dict], dict]:
    """Chunk an XLSX file into row or table region chunks."""
    path = Path(file_path)
    _validate_xlsx_args(
        file_path,
        path,
        mode,
        rows_per_chunk,
        window_size,
        overlap,
        serialize_as,
        max_chunk_chars,
    )

    py_start = time.perf_counter()
    if mode == "table":
        result = _rust.chunk_xlsx_table(
            str(path),
            include_headers,
            sheet_names or [],
            skip_empty_rows,
            max_chunk_chars,
        )
    elif mode == "sheet":
        result = _rust.chunk_xlsx_sheet(
            str(path),
            include_headers,
            sheet_names or [],
            skip_empty_rows,
            max_chunk_chars,
        )
    elif mode == "sliding_window":
        result = _rust.chunk_xlsx_sliding_window(
            str(path),
            window_size,
            overlap,
            include_headers,
            sheet_names or [],
            skip_empty_rows,
        )
    elif mode == "page_aware":
        result = _rust.chunk_xlsx_page_aware(
            str(path),
            include_headers,
            sheet_names or [],
            skip_empty_rows,
            max_chunk_chars,
        )
    elif mode == "semantic":
        result = _rust.chunk_xlsx_semantic(
            str(path),
            rows_per_chunk,
            include_headers,
            sheet_names or [],
            skip_empty_rows,
        )
    else:
        result = _rust.chunk_xlsx_row(
            str(path),
            rows_per_chunk,
            include_headers,
            sheet_names or [],
            skip_empty_rows,
        )
    python_ms = max(round((time.perf_counter() - py_start) * 1000, 3), 0.001)
    return result["chunks"], {"rust_ms": result["rust_ms"], "python_ms": python_ms}


def stream_chunk_xlsx(
    file_path: str,
    mode: str = "row",
    rows_per_chunk: int = 1,
    window_size: int = 3,
    overlap: int = 1,
    include_headers: bool = True,
    sheet_names: list[str] | None = None,
    skip_empty_rows: bool = True,
    max_chunk_chars: int = 2000,
):
    """Stream XLSX chunks as an iterator (batch-drain)."""
    path = Path(file_path)
    _validate_xlsx_args(
        file_path,
        path,
        mode,
        rows_per_chunk,
        window_size,
        overlap,
        "key_value",
        max_chunk_chars,
    )
    return _rust.stream_xlsx_chunks(
        str(path),
        mode,
        rows_per_chunk,
        window_size,
        overlap,
        include_headers,
        sheet_names or [],
        skip_empty_rows,
        max_chunk_chars,
    )
