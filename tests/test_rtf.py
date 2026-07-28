"""Contract tests for .rtf — spec-correct hand-rolled extraction.

Each encoding/structure test targets a specific gap the `rtf-parser` crate had
(metadata leak, \\uN doubling, double-byte \\'xx, surrogate pairs, panics),
which is why we hand-rolled the extractor.
"""

from pathlib import Path

import pytest

from py_chunks import get_chunks, get_chunks_from_bytes, get_markdown, stream_chunks
from py_chunks.chunkers.rtf import chunk_rtf
from _spreadsheet_fixtures import fixtures, require

RTF = fixtures("rtf", "*.rtf")
MODES = ["default", "section", "semantic", "sentence", "sliding_window", "page_aware"]
STANDARD_KEYS = {"content", "content_type", "metadata"}


def _pick(name):
    m = [f for f in RTF if f.name == name]
    require(m)
    return m[0]


def test_no_crash_contract():
    """Every RTF returns chunks or empty — never a panic (incl. invalid unicode)."""
    require(RTF)
    for f in RTF:
        chunks = get_chunks(str(f))  # must not raise/panic
        assert isinstance(chunks, list)
        for c in chunks:
            assert STANDARD_KEYS.issubset(c.keys())


@pytest.mark.parametrize("mode", MODES)
def test_all_modes(mode):
    chunks, _ = chunk_rtf(str(_pick("tika_testRTFHyperlink.rtf")), mode=mode)
    assert chunks


def test_batch_stream_bytes_agree():
    for f in RTF:
        batch = get_chunks(str(f))
        streamed = list(stream_chunks(str(f)))
        from_bytes = get_chunks_from_bytes(f.read_bytes(), "x.rtf")
        assert len(batch) == len(streamed) == len(from_bytes)


# ── Encoding correctness (the double-byte / unicode gaps) ─────────────────────

@pytest.mark.parametrize("name,needle", [
    ("tika_testRTF-ms932.rtf", "こんにちは"),                # Shift-JIS
    ("tika_testRTFJapanese.rtf", "古書店で処刑記録見つかる"),  # Shift-JIS body \\'xx
    ("tika_testRTFWord2010CzechCharacters.rtf", "Mrtvého moře"),  # Czech
    ("tika_testRTFUnicodeGothic.rtf", "𐌲𐌿𐍄𐌹𐍃𐌺"),          # surrogate pairs
    ("tika_testRTFHexEscapeInsideWord.rtf", "ESPÍRITO"),      # \\'xx mid-word
])
def test_encodings_decoded_correctly(name, needle):
    md = get_markdown(str(_pick(name)))
    assert needle in md, f"encoding lost in {name}: {md[:80]!r}"


# ── Metadata must NOT leak into the body (the \\info gap) ──────────────────────

def test_info_metadata_not_leaked():
    """Author/operator/company names from \\info must not appear in chunk text."""
    md = get_markdown(str(_pick("tika_testRTFListMicrosoftWord.rtf")))
    assert "Axel" not in md, f"author leaked: {md[:60]!r}"
    assert md.strip().startswith("A short ordered list")


def test_no_uN_fallback_doubling():
    """`\\uN` must not emit both the unicode char and its ASCII fallback."""
    md = get_markdown(str(_pick("tika_testRTFListMicrosoftWord.rtf")))
    # 'D?fler'/'DÖfler' doubling would show a stray '?' next to letters
    assert "D?fler" not in md and "DörflerAxel" not in md


# ── Structure ─────────────────────────────────────────────────────────────────

def test_table_cells_separated():
    md = get_markdown(str(_pick("tika_testRTFTableCellSeparation.rtf")))
    assert "a | b" in md and "abcd" not in md  # cells not run together


def test_lists_preserved():
    md = get_markdown(str(_pick("tika_testRTFListMicrosoftWord.rtf")))
    assert "1." in md and "one" in md and "two" in md


def test_title_surfaced_when_simple():
    """A simple \\title becomes a heading; an all-'?' \\upr fallback is discarded."""
    md = get_markdown(str(_pick("tika_testRTF-ms932.rtf")))
    assert md.startswith("# タイトル")
    # Japanese file has a '?????' ANSI fallback title — must be dropped, not shown
    jp = get_markdown(str(_pick("tika_testRTFJapanese.rtf")))
    assert "?????" not in jp


def test_invalid_unicode_no_panic():
    md = get_markdown(str(_pick("tika_testRTFInvalidUnicode.rtf")))
    assert isinstance(md, str)  # got here without a panic


def test_body_only_file_returns_empty():
    """An RTF with only fonts/objects/metadata (no body) yields [] cleanly."""
    chunks = get_chunks(str(_pick("tika_testRTFEmbeddedLink.rtf")))
    assert chunks == []
