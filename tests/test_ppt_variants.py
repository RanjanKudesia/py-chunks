"""Contract tests for PowerPoint OOXML variants — .potx/.potm/.ppsx/.ppsm (via pptx)."""

from pathlib import Path

import pytest

from py_chunks import get_chunks, get_chunks_from_bytes, get_markdown, stream_chunks
from py_chunks.chunkers.pptx import chunk_pptx
from _spreadsheet_fixtures import fixtures, require

PPTX_MODES = ["default", "structural", "section", "semantic",
              "sliding_window", "sentence", "page_aware"]
STANDARD_KEYS = {"content", "content_type", "metadata"}

POTX = fixtures("potx", "*.potx")
POTM = fixtures("potm", "*.potm")
PPSX = fixtures("ppsx", "*.ppsx")
PPSM = fixtures("ppsm", "*.ppsm")
ALL = POTX + POTM + PPSX + PPSM


def _assert_schema(chunks):
    for c in chunks:
        assert STANDARD_KEYS.issubset(c.keys())


def _parse_or_clean_error(path: Path):
    try:
        return get_chunks(str(path))
    except Exception:  # noqa: BLE001
        return None


def _pick(files, name):
    m = [f for f in files if f.name == name]
    require(m)
    return m[0]


def test_no_crash_contract():
    require(ALL)
    parsed = sum(_parse_or_clean_error(f) is not None for f in ALL)
    assert parsed >= 30, f"expected >=30 parseable PPT-variant fixtures, got {parsed}"


@pytest.mark.parametrize("files,name", [
    (POTX, "oxml_00_2006-2007MoCalEd_TP10164564.potx"),
    (PPSX, "poi_01_testPPT.ppsx"),
    (POTM, "derived_01_from_large_deck.potm"),
    (PPSM, "poi_02_testPPT.ppsm"),
])
@pytest.mark.parametrize("mode", PPTX_MODES)
def test_all_modes(files, name, mode):
    chunks, _ = chunk_pptx(str(_pick(files, name)), mode=mode)
    _assert_schema(chunks)


@pytest.mark.parametrize("ext,files,name", [
    (".potx", POTX, "oxml_00_2006-2007MoCalEd_TP10164564.potx"),
    (".ppsx", PPSX, "poi_01_testPPT.ppsx"),
    (".potm", POTM, "derived_01_from_large_deck.potm"),
    (".ppsm", PPSM, "poi_02_testPPT.ppsm"),
])
def test_batch_stream_bytes_agree(ext, files, name):
    path = _pick(files, name)
    batch = get_chunks(str(path))
    streamed = list(stream_chunks(str(path)))
    from_bytes = get_chunks_from_bytes(path.read_bytes(), f"x{ext}")
    _assert_schema(batch)
    assert len(batch) == len(streamed) == len(from_bytes)


def test_empty_deck_returns_empty_not_error():
    """A text-less template must return [] (softened from the old 'No text
    content found' raise), not raise — with images still available."""
    textless = [f for f in POTX if f.name == "poi_02_bug59273.potx"]
    require(textless)
    chunks = get_chunks(str(textless[0]))  # must not raise
    assert chunks == []
    # images (if any) still flow via list_images
    res = get_chunks(str(textless[0]), list_images=True)
    assert isinstance(res.chunks, list)


def test_markdown():
    md = get_markdown(str(_pick(POTX, "oxml_00_2006-2007MoCalEd_TP10164564.potx")))
    assert isinstance(md, str) and md.strip()


def test_macro_source_absent_from_content():
    macro_files = [f for f in (POTM + PPSM) if "macro" in f.name.lower()]
    require(macro_files)
    for f in macro_files:
        chunks = _parse_or_clean_error(f)
        if not chunks:
            continue
        joined = " ".join(c["content"] for c in chunks).lower()
        for token in ("end sub", "attribute vb_"):
            assert token not in joined, f"VBA token {token!r} leaked from {f.name}"


def test_ppsx_matches_pptx_behavior():
    """.ppsx is structurally identical to .pptx — a 3-slide deck yields chunks."""
    m = [f for f in PPSX if f.name == "poi_01_testPPT.ppsx"]
    require(m)
    chunks = get_chunks(str(m[0]))
    _assert_schema(chunks)
    assert chunks


def test_legacy_ppt_still_routes_separately():
    """Regression: legacy binary .ppt must not be captured by the pptx set."""
    ppt = fixtures("ppt", "*.ppt")
    require(ppt)
    chunks = get_chunks(str(ppt[0]))  # routes to chunk_ppt, must still work
    assert isinstance(chunks, list)
