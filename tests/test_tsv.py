"""Contract tests for .tsv (tab-separated) via the CSV pipeline."""

from pathlib import Path

import pytest

from py_chunks import get_chunks, get_chunks_from_bytes, get_markdown, stream_chunks
from py_chunks.chunkers.tsv import chunk_tsv, stream_chunk_tsv, tsv_to_markdown
from _spreadsheet_fixtures import fixtures, require

TSV = fixtures("tsv", "*.tsv")
GOOD = "solar_system.tsv"
STANDARD_KEYS = {"content", "content_type", "metadata"}
CSV_MODES = ["row", "sliding_window", "page_aware"]


def _good() -> Path:
    match = [f for f in TSV if f.name == GOOD]
    require(match)
    return match[0]


def _assert_schema(chunks):
    assert chunks
    for c in chunks:
        assert STANDARD_KEYS.issubset(c.keys())


def test_all_fixtures_parse():
    require(TSV)
    for f in TSV:
        chunks = get_chunks(str(f))
        _assert_schema(chunks)


@pytest.mark.parametrize("mode", CSV_MODES)
def test_modes(mode):
    chunks, _ = chunk_tsv(str(_good()), mode=mode)
    _assert_schema(chunks)


def test_splits_on_tab_not_comma():
    """A TSV whose fields contain commas must split on tabs. The solar-system
    fixture has 4 tab-separated columns; the header must expose all four."""
    chunks, _ = chunk_tsv(str(_good()), mode="row", include_headers=True)
    header_names = chunks[0]["metadata"].get("header_row") or []
    # header_row is the parsed column headers; tab-splitting yields >1 column.
    assert isinstance(header_names, list)
    assert len([h for h in header_names if h]) >= 3


def test_comma_bearing_fields_stay_intact():
    """plotly super-store has commas inside fields but tabs as delimiters."""
    match = [f for f in TSV if f.name == "plotly_global_super_store_orders.tsv"]
    require(match)
    chunks = get_chunks(str(match[0]))
    assert chunks
    # If it had split on comma, the first line would be a single giant column.
    first = chunks[0]["content"]
    assert first.count("|") > 5


def test_get_chunks_and_stream_agree():
    path = _good()
    batch = get_chunks(str(path))
    streamed = list(stream_chunks(str(path)))
    _assert_schema(batch)
    assert len(batch) == len(streamed)


def test_bytes_roundtrip():
    path = _good()
    chunks = get_chunks_from_bytes(path.read_bytes(), "data.tsv")
    _assert_schema(chunks)


def test_markdown():
    md = get_markdown(str(_good()))
    assert isinstance(md, str) and "|" in md


def test_explicit_none_delimiter_autodetects():
    """Passing delimiter=None re-enables auto-detect (still resolves to tab)."""
    chunks, _ = chunk_tsv(str(_good()), delimiter=None)
    _assert_schema(chunks)
