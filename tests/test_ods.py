"""Contract tests for .ods (OpenDocument Spreadsheet) via the calamine reader."""

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

ODS = fixtures("ods", "*.ods")
GOOD = "phpspreadsheet_AutoFilter.ods"
ENCRYPTED = "calamine_pass_protected.ods"


def _good() -> Path:
    match = [f for f in ODS if f.name == GOOD]
    require(match)
    return match[0]


def test_no_crash_contract():
    """Every .ods file must return chunks or raise a *catchable* error — never
    a process-aborting panic. At least a few must parse successfully."""
    require(ODS)
    parsed = sum(parse_or_clean_error(f) is not None for f in ODS)
    assert parsed >= 3, f"expected >=3 parseable ODS fixtures, got {parsed}"


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
    chunks = get_chunks_from_bytes(path.read_bytes(), "sheet.ods")
    assert_schema(chunks)
    assert chunks


def test_markdown():
    md = get_markdown(str(_good()))
    assert isinstance(md, str) and md.strip()


def test_missing_mimetype_ods_is_repaired_and_parsed():
    """Regression: the ODF `mimetype` package entry is OPTIONAL, but calamine
    hard-rejects archives that omit it. Such valid `.ods` files must be repaired
    (mimetype synthesised) and parsed, consistently across batch/stream/bytes."""
    candidates = [
        f for f in ODS if f.name in {"filesamples_sample1.ods", "filesamples_sample2.ods"}
    ]
    require(candidates)
    for path in candidates:
        batch = get_chunks(str(path))
        assert_schema(batch)
        assert batch, f"{path.name}: mimetype-less ODS produced no chunks"
        streamed = list(stream_chunks(str(path)))
        from_bytes = get_chunks_from_bytes(path.read_bytes(), path.name)
        assert len(batch) == len(streamed) == len(from_bytes), (
            f"{path.name}: batch/stream/bytes chunk counts diverge"
        )
        # Real spreadsheet content, not an error placeholder.
        assert get_markdown(str(path)).strip()


def test_encrypted_raises_clean_error():
    match = [f for f in ODS if f.name == ENCRYPTED]
    require(match)
    with pytest.raises(Exception):  # noqa: B017 — must be catchable, not a panic
        get_chunks(str(match[0]))


def test_date_cells_render():
    """Regression: typed ODS date cells previously serialised to empty."""
    match = [f for f in ODS if f.name == "calamine_date.ods"]
    require(match)
    chunks = get_chunks(str(match[0]))
    joined = " ".join(c["content"] for c in chunks)
    assert "2021" in joined, "expected a rendered date, date column dropped?"


def test_image_extraction_via_odf_walker():
    """.ods images resolve through the ODF Pictures/ + content.xml walker, with
    per-sheet attribution."""
    match = [f for f in ODS if f.name == "calamine_picture.ods"]
    require(match)
    result = get_chunks(str(match[0]), list_images=True)
    assert len(result.images) == 2
    image_chunks = [c for c in result.chunks if c["content_type"] == "image"]
    assert len(image_chunks) == 2
    sheets = {c["metadata"]["sheet_index"] for c in image_chunks}
    assert sheets == {0, 1}, "images should be attributed to their own sheets"


def test_markdown_with_images():
    match = [f for f in ODS if f.name == "calamine_picture.ods"]
    require(match)
    md = get_markdown(str(match[0]), list_images=True)
    assert len(md.images) == 2
    assert md.markdown.count("![](") == 2


def test_ods_named_ranges_are_surfaced():
    """Audit F1: ODF `table:named-range` must populate named_tables metadata,
    consistent with how xlsx surfaces named tables."""
    match = [f for f in ODS if f.name == "phpspreadsheet_DefinedNames.ods"]
    require(match)
    chunks, _ = chunk_xlsx(str(match[0]), mode="sheet")
    names = set()
    for c in chunks:
        names.update(c["metadata"].get("named_tables") or [])
    assert {"FIRST", "SECOND"}.issubset(names), f"named ranges dropped: {names}"


def test_merge_only_sheet_not_dropped():
    """Audit F2: a sheet whose only content is consumed as a header (single
    merged title cell) must not silently chunk to zero."""
    match = [f for f in ODS if f.name == "phpspreadsheet_MergeRangeTest.ods"]
    require(match)
    for mode in ("row", "sheet"):
        chunks, _ = chunk_xlsx(str(match[0]), mode=mode)
        assert chunks, f"content lost (0 chunks) in mode={mode}"
        assert any("Merge Range" in c["content"] for c in chunks)
