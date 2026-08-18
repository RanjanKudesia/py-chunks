"""Plain text chunking tests across all supported modes and source APIs."""
# pylint: disable=redefined-outer-name

from io import BytesIO
from pathlib import Path

import pytest

from py_chunks import (
    get_chunks,
    get_chunks_from_bytes,
    get_chunks_from_fileobj,
    get_chunks_from_upload,
    stream_chunks_from_bytes,
    stream_chunks_from_fileobj,
)
from py_chunks.chunkers.txt import chunk_txt, stream_chunk_txt


# ── Fixture discovery ─────────────────────────────────────────────────────────

TXT_DIR = Path(__file__).resolve().parents[2] / "test_files" / "txt"

# Naming a fixed list of three files is why TECH_DEBT #30–33 went uncaught: the
# encoding, line-ending and degenerate-input fixtures could sit in the corpus
# without a single test ever opening them. Walk the directory instead, so a
# fixture counts the moment it is added (#34).
ALL_TXT_FILES = sorted(TXT_DIR.glob("*.txt")) if TXT_DIR.exists() else []

# Files that are *meant* to produce no chunks — an empty or whitespace-only
# document parses fine and simply has nothing to chunk, so it returns `[]`
# (TECH_DEBT T6; these used to raise). They are excluded from the sweeps that
# assert non-empty output and asserted on directly in TestTxtDegenerateInputs.
EMPTY_FIXTURES = {"edge_empty.txt", "edge_whitespace_only.txt"}
CHUNKABLE_TXT_FILES = [p for p in ALL_TXT_FILES if p.name not in EMPTY_FIXTURES]

# The three hand-authored documents the older tests were written against. Kept
# as a named subset so the assertions that depend on their specific content
# still have something to point at.
_PROSE_NAMES = ["prose_article.txt", "structured_report.txt", "test.txt"]
PROSE_TXT_FILES = [p for p in ALL_TXT_FILES if p.name in _PROSE_NAMES]

if len(PROSE_TXT_FILES) != 3 or len(CHUNKABLE_TXT_FILES) < 10:
    pytest.skip(
        f"Expected the plain-text fixture corpus in {TXT_DIR} — the three prose "
        "documents plus at least ten chunkable fixtures (TECH_DEBT #34)",
        allow_module_level=True,
    )

ALL_TXT_FILES = CHUNKABLE_TXT_FILES
TXT_IDS = [p.name for p in ALL_TXT_FILES]
STANDARD_KEYS = {"content", "content_type", "metadata"}
VALID_CONTENT_TYPES = {
    "paragraph",
    "heading",
    "code_block",
    "list_item",
    "table",
    "image",
    "plain_paragraph",
    "bullet_list",
    "mixed_content",
    "long_single_paragraph",
    "short_disconnected_paragraph",
    "semantic",
    "section",
    "sliding_window",
    "sentence",
    "page_aware",
}


# ── Fixtures / helpers ───────────────────────────────────────────────────────

@pytest.fixture
def txt_files() -> list[Path]:
    return ALL_TXT_FILES


@pytest.fixture
def txt_file(txt_files) -> Path:
    for p in txt_files:
        if p.name == "prose_article.txt":
            return p
    return txt_files[0]


class _DummyUpload:
    def __init__(self, data: bytes, filename: str):
        self.filename = filename
        self.file = BytesIO(data)


def _get(path: Path, mode: str = "default", **kwargs) -> list[dict]:
    return get_chunks(str(path), mode=mode, **kwargs)


def _assert_nonempty(chunks: list[dict], label: str) -> None:
    assert isinstance(chunks, list), f"{label}: expected list"
    assert len(chunks) > 0, f"{label}: expected non-empty chunk list"


def _assert_schema(chunks: list[dict], label: str) -> None:
    for c in chunks:
        assert STANDARD_KEYS.issubset(c.keys()), (
            f"{label}: missing keys {STANDARD_KEYS - c.keys()}"
        )
        assert isinstance(c["content"], str) and c["content"].strip(), (
            f"{label}: content must be non-empty str"
        )
        assert isinstance(c["content_type"], str) and c["content_type"], (
            f"{label}: content_type must be non-empty str"
        )
        assert isinstance(c["metadata"], dict), f"{label}: metadata must be dict"


def _assert_valid_content_types(chunks: list[dict], label: str) -> None:
    for c in chunks:
        assert c["content_type"] in VALID_CONTENT_TYPES, (
            f"{label}: unexpected content_type {c['content_type']!r}"
        )


# ════════════════════════════════════════════════════════════════════════════
# 1. STRUCTURAL
# ════════════════════════════════════════════════════════════════════════════

@pytest.mark.parametrize("txt_path", ALL_TXT_FILES, ids=TXT_IDS)
class TestTxtStructural:

    def test_returns_nonempty_list(self, txt_path):
        chunks = _get(txt_path, mode="structural")
        _assert_nonempty(chunks, txt_path.name)

    def test_schema_per_chunk(self, txt_path):
        chunks = _get(txt_path, mode="structural")
        _assert_schema(chunks, txt_path.name)

    def test_content_types_valid(self, txt_path):
        chunks = _get(txt_path, mode="structural")
        _assert_valid_content_types(chunks, txt_path.name)

    def test_streaming_parity(self, txt_path):
        batch = _get(txt_path, mode="structural")
        streamed = list(stream_chunk_txt(str(txt_path), mode="structural"))
        assert len(streamed) == len(batch), (
            f"{txt_path.name}: batch={len(batch)} stream={len(streamed)}"
        )


# ════════════════════════════════════════════════════════════════════════════
# 2. SEMANTIC
# ════════════════════════════════════════════════════════════════════════════

@pytest.mark.parametrize("txt_path", ALL_TXT_FILES, ids=TXT_IDS)
class TestTxtSemantic:

    def test_returns_nonempty_list(self, txt_path):
        chunks = _get(txt_path, mode="semantic")
        _assert_nonempty(chunks, txt_path.name)

    def test_schema_per_chunk(self, txt_path):
        chunks = _get(txt_path, mode="semantic")
        _assert_schema(chunks, txt_path.name)

    def test_content_types_valid(self, txt_path):
        chunks = _get(txt_path, mode="semantic")
        _assert_valid_content_types(chunks, txt_path.name)

    def test_streaming_parity(self, txt_path):
        # Keep semantic parity resilient in case batch/stream merge passes diverge.
        batch = _get(txt_path, mode="semantic")
        streamed = list(stream_chunk_txt(str(txt_path), mode="semantic"))
        _assert_nonempty(batch, f"{txt_path.name} batch")
        _assert_nonempty(streamed, f"{txt_path.name} stream")
        _assert_schema(streamed, f"{txt_path.name} stream")


# ════════════════════════════════════════════════════════════════════════════
# 3. SECTION
# ════════════════════════════════════════════════════════════════════════════

@pytest.mark.parametrize("txt_path", ALL_TXT_FILES, ids=TXT_IDS)
class TestTxtSection:

    def test_returns_nonempty_list(self, txt_path):
        chunks = _get(txt_path, mode="section")
        _assert_nonempty(chunks, txt_path.name)

    def test_schema_per_chunk(self, txt_path):
        chunks = _get(txt_path, mode="section")
        _assert_schema(chunks, txt_path.name)

    def test_content_types_valid(self, txt_path):
        chunks = _get(txt_path, mode="section")
        _assert_valid_content_types(chunks, txt_path.name)

    def test_streaming_parity(self, txt_path):
        batch = _get(txt_path, mode="section")
        streamed = list(stream_chunk_txt(str(txt_path), mode="section"))
        assert len(streamed) == len(batch), (
            f"{txt_path.name}: batch={len(batch)} stream={len(streamed)}"
        )

    def test_section_metadata_has_section_heading(self, txt_path):
        chunks = _get(txt_path, mode="section")
        for c in chunks:
            assert "section_heading" in c["metadata"], (
                f"{txt_path.name}: missing section_heading"
            )


# ════════════════════════════════════════════════════════════════════════════
# 4. SLIDING_WINDOW
# ════════════════════════════════════════════════════════════════════════════

@pytest.mark.parametrize("txt_path", ALL_TXT_FILES, ids=TXT_IDS)
class TestTxtSlidingWindow:

    def test_returns_nonempty_list(self, txt_path):
        chunks = _get(txt_path, mode="sliding_window", window_size=3, overlap=1)
        _assert_nonempty(chunks, txt_path.name)

    def test_schema_per_chunk(self, txt_path):
        chunks = _get(txt_path, mode="sliding_window", window_size=3, overlap=1)
        _assert_schema(chunks, txt_path.name)

    def test_content_types_valid(self, txt_path):
        chunks = _get(txt_path, mode="sliding_window", window_size=3, overlap=1)
        _assert_valid_content_types(chunks, txt_path.name)

    def test_streaming_parity(self, txt_path):
        batch = _get(txt_path, mode="sliding_window", window_size=3, overlap=1)
        streamed = list(
            stream_chunk_txt(
                str(txt_path),
                mode="sliding_window",
                window_size=3,
                overlap=1,
            )
        )
        assert len(streamed) == len(batch), (
            f"{txt_path.name}: batch={len(batch)} stream={len(streamed)}"
        )

    def test_sliding_window_metadata_fields(self, txt_path):
        chunks = _get(txt_path, mode="sliding_window", window_size=3, overlap=1)
        for c in chunks:
            meta = c["metadata"]
            assert "chunk_index" in meta
            assert "window_size" in meta
            assert "overlap" in meta


# ════════════════════════════════════════════════════════════════════════════
# 5. SENTENCE
# ════════════════════════════════════════════════════════════════════════════

@pytest.mark.parametrize("txt_path", ALL_TXT_FILES, ids=TXT_IDS)
class TestTxtSentence:

    def test_returns_nonempty_list(self, txt_path):
        chunks = _get(txt_path, mode="sentence", sentences_per_chunk=3)
        _assert_nonempty(chunks, txt_path.name)

    def test_schema_per_chunk(self, txt_path):
        chunks = _get(txt_path, mode="sentence", sentences_per_chunk=3)
        _assert_schema(chunks, txt_path.name)

    def test_content_types_valid(self, txt_path):
        chunks = _get(txt_path, mode="sentence", sentences_per_chunk=3)
        _assert_valid_content_types(chunks, txt_path.name)

    def test_streaming_parity(self, txt_path):
        batch = _get(txt_path, mode="sentence", sentences_per_chunk=3)
        streamed = list(
            stream_chunk_txt(
                str(txt_path),
                mode="sentence",
                sentences_per_chunk=3,
            )
        )
        assert len(streamed) == len(batch), (
            f"{txt_path.name}: batch={len(batch)} stream={len(streamed)}"
        )

    def test_sentence_metadata_has_chunk_index(self, txt_path):
        chunks = _get(txt_path, mode="sentence", sentences_per_chunk=3)
        for c in chunks:
            assert "chunk_index" in c["metadata"], (
                f"{txt_path.name}: missing chunk_index"
            )


# ════════════════════════════════════════════════════════════════════════════
# 6. PAGE_AWARE
# ════════════════════════════════════════════════════════════════════════════

@pytest.mark.parametrize("txt_path", ALL_TXT_FILES, ids=TXT_IDS)
class TestTxtPageAware:

    def test_returns_nonempty_list(self, txt_path):
        chunks = _get(txt_path, mode="page_aware", paragraphs_per_page=15)
        _assert_nonempty(chunks, txt_path.name)

    def test_schema_per_chunk(self, txt_path):
        chunks = _get(txt_path, mode="page_aware", paragraphs_per_page=15)
        _assert_schema(chunks, txt_path.name)

    def test_content_types_valid(self, txt_path):
        chunks = _get(txt_path, mode="page_aware", paragraphs_per_page=15)
        _assert_valid_content_types(chunks, txt_path.name)

    def test_streaming_parity(self, txt_path):
        batch = _get(txt_path, mode="page_aware", paragraphs_per_page=15)
        streamed = list(
            stream_chunk_txt(
                str(txt_path),
                mode="page_aware",
                paragraphs_per_page=15,
            )
        )
        assert len(streamed) == len(batch), (
            f"{txt_path.name}: batch={len(batch)} stream={len(streamed)}"
        )

    def test_page_aware_metadata_fields(self, txt_path):
        chunks = _get(txt_path, mode="page_aware", paragraphs_per_page=15)
        for c in chunks:
            meta = c["metadata"]
            assert "page_break_type" in meta, f"{txt_path.name}: missing page_break_type"
            assert "paragraph_count" in meta, f"{txt_path.name}: missing paragraph_count"


# ════════════════════════════════════════════════════════════════════════════
# 7. SOURCE API EQUIVALENCE
# ════════════════════════════════════════════════════════════════════════════

class TestTxtSourceEquivalence:

    def test_bytes_equals_path(self, txt_file):
        by_path = get_chunks(str(txt_file), mode="structural")
        by_bytes = get_chunks_from_bytes(
            txt_file.read_bytes(),
            filename="x.txt",
            mode="structural",
        )
        assert [c["content"] for c in by_bytes] == [c["content"] for c in by_path]

    def test_fileobj_equals_path(self, txt_file):
        by_path = get_chunks(str(txt_file), mode="structural")
        with open(txt_file, "rb") as f:
            by_fileobj = get_chunks_from_fileobj(f, filename="x.txt", mode="structural")
        assert by_fileobj == by_path

    def test_upload_equals_path(self, txt_file):
        by_path = get_chunks(str(txt_file), mode="structural")
        upload = _DummyUpload(txt_file.read_bytes(), "x.txt")
        by_upload = get_chunks_from_upload(upload, mode="structural")
        assert by_upload == by_path


class TestTxtStreamingSourceEquivalence:

    def test_stream_bytes_equals_path(self, txt_file):
        by_path = list(stream_chunk_txt(str(txt_file), mode="structural"))
        data = Path(txt_file).read_bytes()
        by_bytes = list(
            stream_chunks_from_bytes(data, filename="x.txt", mode="structural")
        )
        assert [c["content"] for c in by_bytes] == [c["content"] for c in by_path]

    def test_stream_fileobj_equals_path(self, txt_file):
        by_path = list(stream_chunk_txt(str(txt_file), mode="structural"))
        with open(txt_file, "rb") as f:
            by_fileobj = list(
                stream_chunks_from_fileobj(f, filename="x.txt", mode="structural")
            )
        assert [c["content"] for c in by_fileobj] == [c["content"] for c in by_path]


# ════════════════════════════════════════════════════════════════════════════
# 8. VALIDATION
# ════════════════════════════════════════════════════════════════════════════

class TestTxtValidation:

    def test_missing_file_raises(self):
        with pytest.raises(FileNotFoundError):
            get_chunks("/nonexistent/path/to/file.txt", mode="structural")

    def test_invalid_mode_raises(self, txt_file):
        with pytest.raises(ValueError):
            get_chunks(str(txt_file), mode="bad_mode")

    def test_wrong_extension_raises(self, txt_file, tmp_path):
        wrong_ext = tmp_path / "bad.md"
        wrong_ext.write_bytes(txt_file.read_bytes())
        with pytest.raises(ValueError):
            chunk_txt(str(wrong_ext), mode="structural")


# ════════════════════════════════════════════════════════════════════════════
# 9. THE EDGE-CASE CORPUS (TECH_DEBT #34)
#
# The corpus used to be three hand-authored prose documents, so nothing
# exercised an empty file, a lone line, a non-UTF-8 encoding, a Windows line
# ending or a grapheme that must not be split. That is why #30–#33 shipped.
# These tests assert on the fixtures those defects would have failed.
# ════════════════════════════════════════════════════════════════════════════

MAX_CHUNK_CHARS = 1200


def _fixture(name: str) -> Path:
    p = TXT_DIR / name
    if not p.exists():
        pytest.skip(f"fixture missing: {p}")
    return p


class TestTxtDegenerateInputs:
    """An empty document returns `[]`; only a broken one raises.

    This class used to assert the opposite ("must say so, not return an empty
    list"). That contract was inconsistent: docx/ppt/xlsx already returned `[]`
    for exactly this condition while txt/html/md raised, so whether "no content"
    was an error depended on the file extension (TECH_DEBT T6). The contract is
    now one rule across every format:

      * parsed fine, nothing to chunk  -> `[]`
      * structurally invalid           -> typed exception carrying a remedy
        (e.g. a PDF with no text layer says to pass `list_images`)

    Callers therefore branch on `if not chunks:` for emptiness and on the
    exception for failure — the two are no longer conflated.
    """

    @pytest.mark.parametrize("name", sorted(EMPTY_FIXTURES))
    def test_empty_input_returns_empty_list(self, name):
        chunks = get_chunks(str(_fixture(name)), mode="structural")
        if isinstance(chunks, tuple):
            chunks = chunks[0]
        assert chunks == []

    @pytest.mark.parametrize(
        "name", ["edge_single_line_no_newline.txt", "edge_single_word.txt"]
    )
    def test_a_single_line_still_chunks(self, name):
        chunks = get_chunks(str(_fixture(name)), mode="structural")
        assert len(chunks) == 1
        assert chunks[0]["content"].strip()


class TestTxtEncodings:
    """Every fixture must decode as what it is — no replacement chars, no NULs.

    #30/#31/#32 were all of this shape, and none of them had a fixture.
    """

    @pytest.mark.parametrize("txt_path", ALL_TXT_FILES, ids=TXT_IDS)
    def test_no_replacement_characters_anywhere(self, txt_path):
        chunks = get_chunks(str(txt_path), mode="structural")
        for c in chunks:
            assert "�" not in c["content"], (
                f"{txt_path.name}: U+FFFD in output — the encoding was guessed wrong"
            )
            assert "\x00" not in c["content"], f"{txt_path.name}: raw NUL in output"
            assert "﻿" not in c["content"], f"{txt_path.name}: BOM leaked through"

    def test_utf16be_without_a_bom_is_sniffed(self):
        chunks = get_chunks(str(_fixture("edge_utf16be_nobom.txt")), mode="structural")
        text = "\n".join(c["content"] for c in chunks)
        assert "UTF-16BE WITHOUT A BOM" in text
        assert "byte order mark" in text

    def test_latin1_accents_survive(self):
        chunks = get_chunks(str(_fixture("edge_latin1.txt")), mode="structural")
        text = "\n".join(c["content"] for c in chunks)
        for word in ("Naïve", "café", "résumé", "Zürich", "Straße", "façade"):
            assert word in text, f"{word!r} did not survive the 8-bit fallback"


class TestTxtMultibyteAndGraphemes:
    """Multi-byte text must never be cut through the middle of a character."""

    @pytest.mark.parametrize("mode", ["structural", "section", "semantic", "sentence"])
    def test_multilingual_output_is_valid_and_whole(self, mode):
        chunks = get_chunks(str(_fixture("edge_multilingual.txt")), mode=mode)
        text = "".join(c["content"] for c in chunks)
        # A mid-character split would surface as a replacement character once the
        # bytes were reassembled into str.
        assert "�" not in text
        for sample in ("日本語の段落です", "中文段落", "פסקה בעברית", "فقرة باللغة"):
            assert sample in text, f"{sample!r} was broken up"

    def test_emoji_zwj_sequences_are_not_split(self):
        chunks = get_chunks(str(_fixture("edge_multilingual.txt")), mode="structural")
        text = "".join(c["content"] for c in chunks)
        for grapheme in ("\U0001F468‍\U0001F469‍\U0001F467‍\U0001F466",
                         "\U0001F3F3️‍\U0001F308",
                         "\U0001F44D\U0001F3FD"):
            assert grapheme in text, "a multi-code-point grapheme was split"


class TestTxtRealWorldShapes:

    def test_an_application_log_keeps_its_stack_trace_together(self):
        chunks = get_chunks(str(_fixture("realworld_application_log.txt")),
                            mode="structural")
        text = "\n".join(c["content"] for c in chunks)
        assert "java.lang.IllegalStateException" in text
        assert "Caused by: java.net.SocketTimeoutException" in text
        assert "... 14 more" in text

    def test_an_ascii_table_is_classified_as_a_table(self):
        chunks = get_chunks(str(_fixture("realworld_ascii_tables.txt")),
                            mode="structural")
        assert any(c["content_type"] == "table" for c in chunks), (
            f"types: {[c['content_type'] for c in chunks]}"
        )

    def test_real_prose_chunks_into_many_pieces(self):
        """A whole Sherlock Holmes story is one document, not one chunk."""
        chunks = get_chunks(str(_fixture("gutenberg_sherlock_scandal.txt")),
                            mode="structural")
        assert len(chunks) > 20
        text = "\n".join(c["content"] for c in chunks)
        assert "A SCANDAL IN BOHEMIA" in text
        assert "Irene Adler" in text


class TestTxtAllCapsHeuristic:
    """TECH_DEBT #33 — measured, not assumed.

    Against the old three-document corpus the ALL-CAPS rule fired 55 times and
    was right every time, which is why narrowing it was refused. With real books
    in the corpus it is provably wrong on some lines, so this test records both
    halves: the headings it must keep calling headings, and the lines a future
    narrowing has to stop calling headings.
    """

    @staticmethod
    def _headings(name: str) -> set[str]:
        chunks = get_chunks(str(_fixture(name)), mode="structural")
        return {c["content"].strip() for c in chunks if c["content_type"] == "heading"}

    def test_genuine_all_caps_headings_are_still_headings(self):
        found = self._headings("adversarial_allcaps.txt")
        for heading in ("OPERATIONS RUNBOOK", "INTRODUCTION", "ESCALATION PATH",
                        "APPENDIX A: GLOSSARY"):
            assert heading in found, f"{heading!r} must stay a heading"

    def test_real_prose_headings_are_still_headings(self):
        assert "A SCANDAL IN BOHEMIA" in self._headings(
            "gutenberg_sherlock_scandal.txt"
        )

    def test_machine_markers_are_not_headings(self):
        """TECH_DEBT #33, fixed once #34's corpus made it reproducible.

        These were a strict xfail until the rule was narrowed; the xfail turning
        into an XPASS is what reported the fix.
        """
        found = self._headings("gutenberg_moby_dick_frontmatter.txt")
        for marker in (
            "*** START OF THE PROJECT GUTENBERG EBOOK MOBY DICK; OR, THE WHALE ***",
            "*** END OF THE PROJECT GUTENBERG EBOOK MOBY DICK; OR, THE WHALE ***",
            "1.F.",
            "MOBY-DICK;",
        ):
            assert marker not in found, f"{marker!r} is not a section heading"

    def test_bare_numerals_above_a_title_are_not_headings(self):
        """Sherlock prints `I.` on its own line above `A SCANDAL IN BOHEMIA`."""
        found = self._headings("gutenberg_sherlock_scandal.txt")
        assert "I." not in found
        assert "A SCANDAL IN BOHEMIA" in found, "the real title must survive"

    def test_the_narrowing_loses_no_genuine_heading(self):
        """The 28 real headings the corpus contains all still classify.

        The admonition blocklist #33 originally proposed would have failed this:
        in adversarial_allcaps.txt, WARNING/NOTE/CAUTION genuinely are headings.
        """
        found = self._headings("adversarial_allcaps.txt")
        for heading in ("OPERATIONS RUNBOOK", "INTRODUCTION", "WARNING", "NOTE",
                        "CAUTION", "ESCALATION PATH", "APPENDIX A: GLOSSARY",
                        "END OF RUNBOOK"):
            assert heading in found, f"{heading!r} must stay a heading"
