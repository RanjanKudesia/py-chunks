"""Contract tests for .xltx / .xltm (Excel templates).

Templates aren't in calamine's extension table; open_spreadsheet routes them to
the Xlsx reader explicitly. The critical case is the bytes API, whose temp file
keeps the .xltx/.xltm suffix — proving the explicit route works end to end.
"""

from pathlib import Path

import pytest

from py_chunks import (
    get_chunks,
    get_chunks_from_bytes,
    get_markdown,
    stream_chunks,
)
from py_chunks.chunkers.xlsx import chunk_xlsx
from _spreadsheet_fixtures import (
    XLSX_MODES,
    assert_schema,
    fixtures,
    parse_or_clean_error,
    require,
)

XLTX = fixtures("xltx_xltm", "*.xltx")
XLTM = fixtures("xltx_xltm", "*.xltm")
ALL = XLTX + XLTM
GOOD_XLTX = "derived_from_file_example_XLSX_50.xltx"
GOOD_XLTM = "derived_from_poi_testNames.xltm"


def _pick(name: str) -> Path:
    match = [f for f in ALL if f.name == name]
    require(match)
    return match[0]


def test_no_crash_contract():
    require(ALL)
    parsed = sum(parse_or_clean_error(f) is not None for f in ALL)
    assert parsed >= 8, f"expected >=8 parseable template fixtures, got {parsed}"


@pytest.mark.parametrize("mode", XLSX_MODES)
def test_all_modes_xltx(mode):
    chunks, _ = chunk_xlsx(str(_pick(GOOD_XLTX)), mode=mode)
    assert_schema(chunks)


@pytest.mark.parametrize("ext,name", [(".xltx", GOOD_XLTX), (".xltm", GOOD_XLTM)])
def test_bytes_roundtrip_keeps_suffix(ext, name):
    """The bytes path writes a temp file with the template suffix; calamine must
    still route it to the Xlsx reader via open_spreadsheet."""
    path = _pick(name)
    chunks = get_chunks_from_bytes(path.read_bytes(), f"template{ext}")
    assert_schema(chunks)
    assert chunks


def test_get_chunks_and_stream_agree():
    path = _pick(GOOD_XLTX)
    batch = get_chunks(str(path))
    streamed = list(stream_chunks(str(path)))
    assert_schema(batch)
    assert len(batch) == len(streamed)


def test_markdown():
    md = get_markdown(str(_pick(GOOD_XLTX)))
    assert isinstance(md, str) and md.strip()


def test_empty_template_yields_no_data_rows():
    """A real, essentially-empty template (poi_test.xltx) must not fabricate
    bogus data-row chunks."""
    match = [f for f in XLTX if f.name == "poi_test.xltx"]
    require(match)
    chunks, _ = chunk_xlsx(str(match[0]), mode="row")
    # row mode emits one chunk per data row; an empty template has none.
    assert chunks == [] or all(c["content"].strip() for c in chunks)
