"""TSV chunker — thin wrapper over the CSV Rust pipeline with a tab default.

A .tsv file is CSV with a tab delimiter. The Rust CSV pipeline is already
delimiter-agnostic (its `detect_delimiter` scores tab as a candidate), so TSV
support is purely a matter of defaulting the delimiter to ``"\\t"`` and accepting
the ``.tsv`` suffix. Passing ``delimiter=None`` explicitly re-enables auto-detect.
"""

from .csv import chunk_csv, stream_chunk_csv, csv_to_markdown

_TAB = "\t"


def chunk_tsv(
    file_path: str,
    mode: str = "row",
    rows_per_chunk: int = 10,
    window_size: int = 5,
    overlap: int = 1,
    include_headers: bool = True,
    delimiter: str | None = _TAB,
    encoding: str = "utf-8",
    skip_empty_rows: bool = True,
) -> tuple[list[dict], dict]:
    """Chunk a TSV file. Delimiter defaults to tab; pass ``None`` for auto-detect."""
    return chunk_csv(
        file_path,
        mode=mode,
        rows_per_chunk=rows_per_chunk,
        window_size=window_size,
        overlap=overlap,
        include_headers=include_headers,
        delimiter=delimiter,
        encoding=encoding,
        skip_empty_rows=skip_empty_rows,
    )


def stream_chunk_tsv(
    file_path: str,
    mode: str = "row",
    rows_per_chunk: int = 10,
    window_size: int = 5,
    overlap: int = 1,
    include_headers: bool = True,
    delimiter: str | None = _TAB,
    encoding: str = "utf-8",
    skip_empty_rows: bool = True,
):
    """Stream TSV chunks as an iterator. Delimiter defaults to tab."""
    return stream_chunk_csv(
        file_path,
        mode=mode,
        rows_per_chunk=rows_per_chunk,
        window_size=window_size,
        overlap=overlap,
        include_headers=include_headers,
        delimiter=delimiter,
        encoding=encoding,
        skip_empty_rows=skip_empty_rows,
    )


def tsv_to_markdown(
    file_path: str,
    delimiter: str | None = _TAB,
    encoding: str = "utf-8",
) -> str:
    """Convert a TSV file to a Markdown pipe table. Delimiter defaults to tab."""
    return csv_to_markdown(file_path, delimiter=delimiter, encoding=encoding)
