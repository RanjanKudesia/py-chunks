"""Contract + quality tests for `.odp` (OpenDocument Presentation), driven off
real fixtures from The Document Foundation's ODF Toolkit and Apache Tika."""

from pathlib import Path

import pytest

from py_chunks import get_chunks, get_chunks_from_bytes, get_markdown, stream_chunks
from py_chunks.chunkers.odf import chunk_odf

_TEST_FILES = Path(__file__).resolve().parents[2] / "test_files"
ODP_DIR = _TEST_FILES / "odp"

STANDARD_KEYS = {"content", "content_type", "metadata"}


def odp_files() -> list[Path]:
    if not ODP_DIR.is_dir():
        return []
    return sorted(ODP_DIR.glob("*.odp"))


def require(files) -> None:
    if not files:
        pytest.skip("no .odp fixtures present")


def find(name: str) -> Path:
    match = [f for f in odp_files() if f.name == name]
    require(match)
    return match[0]


def test_no_crash_contract():
    """Every .odp (incl. malformed/NPE decks) returns chunks or a catchable
    error — never a panic."""
    require(odp_files())
    for f in odp_files():
        try:
            chunks = get_chunks(str(f))
            assert isinstance(chunks, list)
            for c in chunks:
                assert STANDARD_KEYS.issubset(c.keys())
        except (RuntimeError, ValueError):
            pass


def test_batch_stream_bytes_agree():
    require(odp_files())
    for f in odp_files():
        try:
            b = get_chunks(str(f))
        except (RuntimeError, ValueError):
            continue
        s = list(stream_chunks(str(f)))
        y = get_chunks_from_bytes(f.read_bytes(), f.name)
        assert len(b) == len(s) == len(y), f"{f.name}: paths diverge"


def test_slides_sectioned_and_counted():
    f = find("odftoolkit_Presentation1.odp")
    md = get_markdown(str(f))
    assert "## Slide 1" in md
    meta = get_chunks(str(f))[0]["metadata"]["document_metadata"]
    assert meta["source_type"] == "odp"
    assert meta["slide_count"] >= 2


def test_cjk_content_decoded():
    """Presentation1.odp carries genuine CJK content."""
    import re

    f = find("odftoolkit_Presentation1.odp")
    md = get_markdown(str(f))
    assert re.search(r"[一-鿿]", md), "CJK content not present/decoded"


def test_speaker_notes_extracted():
    f = find("odftoolkit_LibCon_Tokyo_with_Notes.odp")
    md = get_markdown(str(f))
    assert "**Notes:**" in md


def test_image_extraction():
    """A deck with embedded images exposes them via list_images."""
    f = find("odftoolkit_LibCon_Tokyo_with_Notes.odp")
    result = get_chunks(str(f), list_images=True)
    assert len(result.images) > 0
    for name, data in result.images.items():
        assert isinstance(data, bytes) and len(data) > 0


def test_wrong_extension_raises(tmp_path):
    p = tmp_path / "file.pptx"
    p.write_bytes(b"PK\x03\x04fake")
    with pytest.raises(ValueError):
        chunk_odf(str(p))
