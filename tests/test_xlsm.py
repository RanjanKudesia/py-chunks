"""Contract tests for .xlsm (macro-enabled workbook).

.xlsm uses calamine's Xlsx reader — identical path to .xlsx. The extra concern is
that VBA macro payloads never leak into chunk content.
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

XLSM = fixtures("xlsm", "*.xlsm")
GOOD = "poi_testNames.xlsm"
VBA = "poi_SimpleMacro.xlsm"


def _good() -> Path:
    match = [f for f in XLSM if f.name == GOOD]
    require(match)
    return match[0]


def test_no_crash_contract():
    require(XLSM)
    parsed = sum(parse_or_clean_error(f) is not None for f in XLSM)
    assert parsed >= 8, f"expected >=8 parseable XLSM fixtures, got {parsed}"


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
    chunks = get_chunks_from_bytes(path.read_bytes(), "macro.xlsm")
    assert_schema(chunks)
    assert chunks


def test_markdown():
    md = get_markdown(str(_good()))
    assert isinstance(md, str) and md.strip()


def test_macro_source_absent_from_content():
    match = [f for f in XLSM if f.name == VBA]
    require(match)
    chunks = parse_or_clean_error(match[0])
    if chunks is None:
        pytest.skip(f"{VBA} not readable by calamine on this build")
    joined = " ".join(c["content"] for c in chunks).lower()
    # VBA keywords that would indicate macro source leaking into cell content.
    for token in ("sub ", "end sub", "function ", "dim "):
        assert token not in joined, f"VBA token {token!r} leaked into content"
