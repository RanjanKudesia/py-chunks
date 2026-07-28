"""Contract tests for .xlsb (Excel binary workbook) via the calamine reader.

Notably exercises the panic-safety wrapper: calamine 0.26 hard-panics on some
real .xlsb files (e.g. poi_Simple.xlsb) — those must surface as clean, catchable
errors, never a process abort.
"""

from pathlib import Path

import pytest

from py_chunks import get_chunks, get_chunks_from_bytes, get_markdown, stream_chunks
from py_chunks.chunkers.xlsx import chunk_xlsx
from _spreadsheet_fixtures import (
    XLSX_MODES,
    assert_schema,
    fixtures,
    parse_or_clean_error,
    require,
)

XLSB = fixtures("xlsb", "*.xlsb")
GOOD = "calamine_issues.xlsb"
ENCRYPTED = "calamine_pass_protected.xlsb"
# Files that trigger an internal calamine panic — must degrade to a clean error.
PANIC_PRONE = ["poi_Simple.xlsb", "calamine_issue_419.xlsb"]


def _good() -> Path:
    match = [f for f in XLSB if f.name == GOOD]
    require(match)
    return match[0]


def test_no_crash_contract():
    require(XLSB)
    parsed = sum(parse_or_clean_error(f) is not None for f in XLSB)
    assert parsed >= 5, f"expected >=5 parseable XLSB fixtures, got {parsed}"


@pytest.mark.parametrize("name", PANIC_PRONE)
def test_calamine_panic_becomes_catchable_error(name):
    match = [f for f in XLSB if f.name == name]
    require(match)
    # The key guarantee: a calamine panic is caught in Rust and re-raised as a
    # normal Exception, not a BaseException/abort. `except Exception` must catch it.
    with pytest.raises(Exception):  # noqa: B017
        get_chunks(str(match[0]))


@pytest.mark.parametrize("mode", XLSX_MODES)
def test_all_modes(mode):
    chunks, _ = chunk_xlsx(str(_good()), mode=mode)
    assert_schema(chunks)


def test_get_chunks_and_stream_agree():
    path = _good()
    batch = get_chunks(str(path))
    streamed = list(stream_chunks(str(path)))
    assert_schema(batch)
    assert len(batch) == len(streamed)


def test_bytes_roundtrip():
    path = _good()
    chunks = get_chunks_from_bytes(path.read_bytes(), "book.xlsb")
    assert_schema(chunks)
    assert chunks


def test_markdown():
    md = get_markdown(str(_good()))
    assert isinstance(md, str) and md.strip()


def test_encrypted_raises_clean_error():
    match = [f for f in XLSB if f.name == ENCRYPTED]
    require(match)
    with pytest.raises(Exception):  # noqa: B017
        get_chunks(str(match[0]))


def test_image_extraction_via_bin_rels_fallback():
    """.xlsb images resolve through the sheetN.bin.rels fallback, with per-sheet
    attribution matching the OOXML walker."""
    match = [f for f in XLSB if f.name == "calamine_picture.xlsb"]
    require(match)
    result = get_chunks(str(match[0]), list_images=True)
    assert len(result.images) == 2
    image_chunks = [c for c in result.chunks if c["content_type"] == "image"]
    assert len(image_chunks) == 2
    sheets = {c["metadata"]["sheet_index"] for c in image_chunks}
    assert sheets == {0, 1}, "images should be attributed to their own sheets"


def test_markdown_with_images():
    match = [f for f in XLSB if f.name == "calamine_picture.xlsb"]
    require(match)
    md = get_markdown(str(match[0]), list_images=True)
    assert len(md.images) == 2
    assert md.markdown.count("![](") == 2
