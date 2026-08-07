"""XLSX chunker wrapper over the Rust extension."""

import time
from pathlib import Path

from py_chunks import _rust

_XLSX_MODES = {"row", "table", "sheet",
               "sliding_window", "page_aware", "semantic"}
_XLSX_SERIALIZERS = {"key_value"}

# Every calamine-readable spreadsheet extension routed through this chunker.
# Must stay in sync with SPREADSHEET_EXTS in src/extensions/xlsx/common.rs.
_XLSX_SUFFIXES = (".xlsx", ".xls", ".xlsm", ".xlsb", ".ods", ".xltx", ".xltm")

# Formats with working embedded-image extraction. The Xlsx OOXML family and .xlsb
# go through the OOXML drawing walker (.xlsb via a sheetN.bin.rels fallback); .ods
# goes through a dedicated ODF walker (Pictures/ + content.xml draw:image). Only
# .xls (OLE, no drawing parts calamine exposes) returns an empty image dict.
_XLSX_IMAGE_SUFFIXES = (".xlsx", ".xlsm", ".xltx", ".xltm", ".xlsb", ".ods")


def _validate_xlsx_options(
    mode: str,
    rows_per_chunk: int,
    window_size: int,
    overlap: int,
    serialize_as: str,
    max_chunk_chars: int,
) -> None:
    """Path-independent half of the validation — shared with the bytes route."""
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
    if path.suffix.lower() not in _XLSX_SUFFIXES:
        raise ValueError(
            f"Expected one of {', '.join(_XLSX_SUFFIXES)}, got: {file_path}")
    _validate_xlsx_options(
        mode, rows_per_chunk, window_size, overlap, serialize_as, max_chunk_chars
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


def xlsx_to_markdown(file_path: str) -> str:
    """Convert an XLSX/XLS file to a Markdown string (one pipe table per sheet)."""
    path = Path(file_path)
    if not path.is_file():
        raise FileNotFoundError(f"Spreadsheet file not found: {file_path}")
    if path.suffix.lower() not in _XLSX_SUFFIXES:
        raise ValueError(
            f"Expected one of {', '.join(_XLSX_SUFFIXES)}, got: {file_path}")
    return _rust.xlsx_to_markdown(str(path))


def chunk_xlsx_with_images(
    file_path: str,
    mode: str = "row",
    rows_per_chunk: int = 1,
    window_size: int = 3,
    overlap: int = 1,
    include_headers: bool = True,
    sheet_names: list[str] | None = None,
    skip_empty_rows: bool = True,
    max_chunk_chars: int = 2000,
    sentences_per_chunk: int | None = None,
    paragraphs_per_page: int | None = None,
) -> tuple[list[dict], dict[str, bytes]]:
    """Chunk a spreadsheet and extract embedded images as image chunks.

    Images are returned as a dict keyed by hash filename for the Xlsx OOXML family
    (.xlsx/.xlsm/.xltx/.xltm), .xlsb (via a sheetN.bin.rels fallback), and .ods
    (via the ODF Pictures/ walker). For .xls, image extraction is unsupported and
    returns an empty dict.
    """
    path = Path(file_path)
    normalized_mode = "row" if mode == "default" else mode

    if sentences_per_chunk is not None and rows_per_chunk == 1:
        rows_per_chunk = 1 if sentences_per_chunk == 3 else sentences_per_chunk
    # For API compatibility with get_chunks(..., mode="page_aware").
    _ = paragraphs_per_page

    _validate_xlsx_args(
        file_path,
        path,
        normalized_mode,
        rows_per_chunk,
        window_size,
        overlap,
        "key_value",
        max_chunk_chars,
    )
    chunk_list, image_list = _rust.chunk_xlsx_with_images(
        str(path),
        normalized_mode,
        rows_per_chunk,
        window_size,
        overlap,
        include_headers,
        sheet_names or [],
        skip_empty_rows,
        max_chunk_chars,
    )
    return chunk_list, dict(image_list)


def xlsx_to_markdown_with_images(file_path: str) -> tuple[str, dict[str, bytes]]:
    """Convert a spreadsheet to Markdown and extract embedded images.

    Markdown includes ![](hash.ext) references after each sheet section for the
    Xlsx OOXML family, .xlsb, and .ods. For .xls, conversion succeeds and the
    image dict is empty.
    """
    path = Path(file_path)
    if not path.is_file():
        raise FileNotFoundError(f"Spreadsheet file not found: {file_path}")
    if path.suffix.lower() not in _XLSX_SUFFIXES:
        raise ValueError(
            f"Expected one of {', '.join(_XLSX_SUFFIXES)}, got: {file_path}")
    md, image_list = _rust.xlsx_to_markdown_with_images(str(path))
    return md, dict(image_list)
