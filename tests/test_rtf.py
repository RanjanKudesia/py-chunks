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


# ── Character formatting survives as markdown emphasis ────────────────────────

def test_bold_and_italic_become_emphasis():
    md = get_markdown(str(_pick("tika_testRTFBoldItalic.rtf")))
    lines = md.splitlines()
    assert lines[0] == "**bold**"
    assert lines[1] == "**bold** ***italic***"
    assert lines[3] == "*italic*"


def test_emphasis_wraps_only_its_own_run():
    """Buffered bytes must flush before `\\b`/`\\i` changes under them."""
    md = get_markdown(str(_pick("tika_testRTFHyperlink.rtf")))
    assert "**Type a question for help**" in md
    assert "**Contact Us**" in md


def test_emphasis_markers_are_well_formed():
    for f in RTF:
        for line in get_markdown(str(f)).splitlines():
            assert line.count("*") % 2 == 0, f"unbalanced emphasis in {f.name}: {line}"
            assert "****" not in line, f"empty emphasis span in {f.name}: {line}"


# ── Body headings come from paragraph styles ──────────────────────────────────

def test_heading_styles_become_headings():
    """LibreOffice keeps `heading 1/2/3` when converting the POI original."""
    md = get_markdown(str(_pick("conv_libreoffice_heading123.rtf")))
    lines = md.splitlines()
    assert lines[0] == "# First paragraph"
    assert "## Second paragraph" in lines
    assert "### Third paragraph" in lines
    assert sum(1 for line in lines if line.startswith("#")) == 3


def test_headings_reach_chunk_metadata():
    chunks, _ = chunk_rtf(str(_pick("conv_libreoffice_heading123.rtf")), mode="semantic")
    headings = [c for c in chunks if c["content_type"] == "heading"]
    assert [c["content"] for c in headings] == [
        "First paragraph", "Second paragraph", "Third paragraph",
    ]
    assert headings[2]["metadata"]["heading_path"] == [
        "First paragraph", "Second paragraph", "Third paragraph",
    ]


def test_no_headings_without_a_stylesheet():
    """Apple's `textutil` drops the styles, so there is nothing to detect."""
    md = get_markdown(str(_pick("conv_apple_heading123.rtf")))
    assert md.startswith("First paragraph")
    assert "#" not in md


# ── Symbol-font list markers ──────────────────────────────────────────────────

def test_list_markers_are_markdown_not_glyphs():
    for name in ("tika_testRTFListLibreOffice.rtf", "tika_testRTFListMicrosoftWord.rtf"):
        md = get_markdown(str(_pick(name)))
        assert "- first" in md, f"{name}: {md!r}"
        assert "1. one" in md, f"{name}: {md!r}"


def test_no_replacement_or_private_use_characters():
    """OpenSymbol decoded U+FFFD; Wingdings wrote its bullet as U+F0FC."""
    for f in RTF:
        for ch in get_markdown(str(f)):
            assert ch != "�", f"replacement char in {f.name}"
            assert not 0xE000 <= ord(ch) <= 0xF8FF, f"private use char in {f.name}"


# ── document_metadata.author ──────────────────────────────────────────────────

@pytest.mark.parametrize("name,author", [
    ("tika_testRTFBoldItalic.rtf", "Michael McCandless"),
    ("tika_testRTFListLibreOffice.rtf", "Axel Dörfler"),   # \\'f6 in its code page
    ("conv_libreoffice_heading123.rtf", "Paolo Mottadelli"),  # a real \\upr pair
    ("tika_testRTF-ms932.rtf", "shinsuke"),
])
def test_author_recovered(name, author):
    chunks, _ = chunk_rtf(str(_pick(name)), mode="semantic")
    assert chunks[0]["metadata"]["document_metadata"]["author"] == author


def test_author_absent_when_no_info_group():
    chunks, _ = chunk_rtf(str(_pick("tika_testRTFHyperlink.rtf")), mode="semantic")
    assert chunks[0]["metadata"]["document_metadata"]["author"] is None


def test_body_only_file_returns_empty():
    """An RTF with only fonts/objects/metadata (no body) yields [] cleanly."""
    chunks = get_chunks(str(_pick("tika_testRTFEmbeddedLink.rtf")))
    assert chunks == []
