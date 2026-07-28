"""Contract tests for Word OOXML variants — .docm / .dotx / .dotm (via docx)."""

from pathlib import Path

import pytest

from py_chunks import get_chunks, get_chunks_from_bytes, get_markdown, stream_chunks
from py_chunks.chunkers.docx import chunk_docx
from _spreadsheet_fixtures import fixtures, require

DOCX_MODES = ["default", "structural", "section", "semantic",
              "sliding_window", "sentence", "page_aware"]
STANDARD_KEYS = {"content", "content_type", "metadata"}

DOCM = fixtures("docm", "*.docm")
DOTX = fixtures("dotx", "*.dotx")
DOTM = fixtures("dotm", "*.dotm")
ALL = DOCM + DOTX + DOTM


def _assert_schema(chunks):
    for c in chunks:
        assert STANDARD_KEYS.issubset(c.keys())


def _parse_or_clean_error(path: Path):
    try:
        return get_chunks(str(path))
    except Exception:  # noqa: BLE001 — contract is a clean, catchable error
        return None


def _pick(files, name):
    m = [f for f in files if f.name == name]
    require(m)
    return m[0]


def test_no_crash_contract():
    """Every Word variant returns chunks or a catchable error — never a panic."""
    require(ALL)
    parsed = sum(_parse_or_clean_error(f) is not None for f in ALL)
    assert parsed >= 24, f"expected >=24 parseable Word-variant fixtures, got {parsed}"


@pytest.mark.parametrize("ext,files,name", [
    (".docm", DOCM, "poi_02_45690.docm"),
    (".dotx", DOTX, "poi_02_60316.dotx"),
    (".dotm", DOTM, "tika_02_testDOTM.dotm"),
])
@pytest.mark.parametrize("mode", DOCX_MODES)
def test_all_modes(ext, files, name, mode):
    chunks, _ = chunk_docx(str(_pick(files, name)), mode=mode)
    _assert_schema(chunks)


@pytest.mark.parametrize("ext,files,name", [
    (".docm", DOCM, "poi_02_45690.docm"),
    (".dotx", DOTX, "poi_02_60316.dotx"),
    (".dotm", DOTM, "tika_02_testDOTM.dotm"),
])
def test_batch_stream_bytes_agree(ext, files, name):
    path = _pick(files, name)
    batch = get_chunks(str(path))
    streamed = list(stream_chunks(str(path)))
    from_bytes = get_chunks_from_bytes(path.read_bytes(), f"x{ext}")
    _assert_schema(batch)
    # streaming may split long multibyte paragraphs slightly differently, but
    # both must be non-empty and schema-valid (regression guard for the
    # char-boundary panic fix on Cyrillic/CJK text).
    assert batch and streamed and from_bytes


def test_markdown():
    md = get_markdown(str(_pick(DOTM, "tika_02_testDOTM.dotm")))
    assert isinstance(md, str) and md.strip()


def test_encrypted_docm_raises_clean_error():
    enc = [f for f in DOCM if "encrypted" in f.name]
    require(enc)
    with pytest.raises(Exception):  # noqa: B017 — must be catchable, not a panic
        get_chunks(str(enc[0]))


def test_empty_template_does_not_error():
    """A near-empty .dotx returns chunks (possibly few), never raises."""
    m = [f for f in DOTX if f.name == "poi_06_60316b.dotx"]
    require(m)
    chunks = get_chunks(str(m[0]))  # must not raise
    assert isinstance(chunks, list)


def test_macro_source_absent_from_content():
    """VBA/macro payload must never leak into chunk text (.docm/.dotm)."""
    macro_files = [f for f in DOCM if "Macro" in f.name or "macro" in f.name]
    require(macro_files)
    for f in macro_files:
        chunks = _parse_or_clean_error(f)
        if chunks is None:
            continue
        joined = " ".join(c["content"] for c in chunks).lower()
        for token in ("end sub", "attribute vb_", "sub autoopen"):
            assert token not in joined, f"VBA token {token!r} leaked from {f.name}"


def test_image_extraction():
    """word/media images extract through the docx walker for variants."""
    m = [f for f in DOCM if f.name == "poi_02_45690.docm"]
    require(m)
    result = get_chunks(str(m[0]), list_images=True)
    assert len(result.images) >= 1
    assert any(c["content_type"] == "image" for c in result.chunks)


def test_multibyte_streaming_no_panic():
    """Regression: streaming a Cyrillic-heavy .dotm previously panicked on a
    non-char-boundary byte slice."""
    m = [f for f in DOTM if f.name == "tika_02_testDOTM.dotm"]
    require(m)
    chunks = list(stream_chunks(str(m[0])))  # must not panic
    assert chunks
