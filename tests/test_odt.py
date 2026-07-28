"""Contract + quality tests for `.odt` (OpenDocument Text), driven off real
fixtures from The Document Foundation's ODF Toolkit and Apache Tika."""

from pathlib import Path

import pytest

from py_chunks import get_chunks, get_chunks_from_bytes, get_markdown, stream_chunks
from py_chunks.chunkers.odf import chunk_odf

_TEST_FILES = Path(__file__).resolve().parents[2] / "test_files"
ODT_DIR = _TEST_FILES / "odt"

ODF_MODES = ["default", "section", "semantic", "sentence", "sliding_window", "page_aware"]
STANDARD_KEYS = {"content", "content_type", "metadata"}
ENCRYPTED = {
    "odftoolkit_PasswordProtected.odt",
    "odftoolkit_encrypted-with-pwd_hello.odt",
    "tika_testODTEncrypted.odt",
}
NOT_A_ZIP = "tika_testODTnotaZipFile.odt"


def odt_files() -> list[Path]:
    if not ODT_DIR.is_dir():
        return []
    return sorted(ODT_DIR.glob("*.odt"))


def good_files() -> list[Path]:
    return [f for f in odt_files() if f.name not in ENCRYPTED and f.name != NOT_A_ZIP]


def require(files) -> None:
    if not files:
        pytest.skip("no .odt fixtures present")


def find(name: str) -> Path:
    match = [f for f in odt_files() if f.name == name]
    require(match)
    return match[0]


def test_no_crash_contract():
    """Every .odt returns chunks or a *catchable* error — never a panic."""
    require(odt_files())
    parsed = 0
    for f in odt_files():
        try:
            get_chunks(str(f))
            parsed += 1
        except (RuntimeError, ValueError):
            pass
    assert parsed >= 8


@pytest.mark.parametrize("mode", ODF_MODES)
def test_all_modes(mode):
    f = find("odftoolkit_Lebenslauf_German.odt")
    chunks, timing = chunk_odf(str(f), mode=mode)
    assert isinstance(chunks, list)
    for c in chunks:
        assert STANDARD_KEYS.issubset(c.keys())
    assert "rust_ms" in timing


def test_batch_stream_bytes_agree():
    for f in good_files():
        b = get_chunks(str(f))
        s = list(stream_chunks(str(f)))
        y = get_chunks_from_bytes(f.read_bytes(), f.name)
        assert len(b) == len(s) == len(y), f"{f.name}: paths diverge"


def test_headings_and_accents():
    """text:h → markdown headings; German ä/ö/ü/ß preserved."""
    f = find("odftoolkit_Lebenslauf_German.odt")
    md = get_markdown(str(f))
    assert "# Lebenslauf" in md
    assert "Staatsangehörigkeit" in md
    assert "Musterstraße" in md


def test_footnotes_retained():
    f = find("odftoolkit_footnote.odt")
    md = get_markdown(str(f))
    assert "## Notes" in md


def test_hyperlink_rendered():
    f = find("odftoolkit_hyperlink.odt")
    md = get_markdown(str(f))
    assert "](http" in md, "hyperlink not rendered as markdown link"


def test_metadata_shape():
    f = find("odftoolkit_Lebenslauf_German.odt")
    meta = get_chunks(str(f))[0]["metadata"]["document_metadata"]
    assert meta["source_type"] == "odt"
    assert "title" in meta and "creator" in meta


def test_encrypted_raises_clean_error():
    match = [f for f in odt_files() if f.name in ENCRYPTED]
    require(match)
    for f in match:
        with pytest.raises(Exception):  # noqa: B017 — must be catchable, not a panic
            get_chunks(str(f))


def test_not_a_zip_raises_clean_error():
    f = find(NOT_A_ZIP)
    with pytest.raises(Exception):  # noqa: B017
        get_chunks(str(f))


def test_wrong_extension_raises(tmp_path):
    p = tmp_path / "file.txt"
    p.write_text("x", encoding="utf-8")
    with pytest.raises(ValueError):
        chunk_odf(str(p))
