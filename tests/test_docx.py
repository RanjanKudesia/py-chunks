"""
Comprehensive tests for DOCX chunking — all modes, all test files.

Every test class is parametrized over every .docx file in test_files/docx/
so schema, metadata, and content assertions are verified against 8 real documents.

File inventory (test_files/docx/):
  all_round.docx        — headings + tables + mixed content
  form_template.docx    — section headings (personal/employment info)
  large_document.docx   — 139 chunks, 3 heading levels
  lists.docx            — list-heavy, no structural headings
  multi_column.docx     — multi-column layout
  quarterly_report.docx — prose report with financial tables
  tables.docx           — table-heavy document
  tracked_changes.docx  — document with tracked revisions

Streaming note: DOCX only supports streaming for default/structural.
All other modes raise NotImplementedError on stream.
"""

from io import BytesIO
from pathlib import Path

import pytest

from py_chunks import (
    get_chunks,
    get_chunks_from_bytes,
    get_chunks_from_fileobj,
    get_chunks_from_upload,
    stream_chunks,
    stream_chunks_from_bytes,
    stream_chunks_from_fileobj,
)
from py_chunks.chunkers.docx import chunk_docx, stream_chunk_docx

# ── Discover all test DOCX files ──────────────────────────────────────────────

_CANDIDATE_DIRS = [
    Path(__file__).resolve().parents[2] / "test_files",
    Path(__file__).resolve().parents[2] / "test_input_files",
]

_DOCX_DIR = next(
    (d / "docx" for d in _CANDIDATE_DIRS if (d / "docx").exists()),
    None,
)

# Fixtures the engine is CORRECT to reject. They are encrypted/malformed, so a
# clean raise is the desired behaviour — but every parametrized test below
# assumes its fixture parses successfully, so leaving them in the glob turns one
# well-behaved refusal into dozens of spurious failures. They get a dedicated
# expect-raises test instead (see test_adversarial_fixtures_raise_cleanly).
ADVERSARIAL_DOCX = {
    # OLE2 compound file (password-protected), not a zip. py-chunks raises
    # "Could not find EOCD", which is right.
    "poi_bug53475-password-is-pass.docx",
}

_ALL_GLOBBED = sorted(_DOCX_DIR.glob("*.docx")) if _DOCX_DIR else []
ALL_DOCX_FILES = [f for f in _ALL_GLOBBED if f.name not in ADVERSARIAL_DOCX]
ADVERSARIAL_DOCX_FILES = [f for f in _ALL_GLOBBED if f.name in ADVERSARIAL_DOCX]

if not ALL_DOCX_FILES:
    pytest.skip(
        "No DOCX test files found in test_files/docx/",
        allow_module_level=True,
    )

DOCX_IDS = [f.name for f in ALL_DOCX_FILES]

# Word supports nine heading levels, not six. `<w:outlineLvl>` is 0-based and
# valid for 0-8, so a heading can legitimately report level 9 — poi_bug59058.docx
# uses outlineLvl 6 and must report 7. The old 1-6 bound was borrowed from HTML's
# h1-h6 and failed on a document the engine reads correctly.
MAX_HEADING_LEVEL = 9

STANDARD_KEYS = {"content", "content_type", "metadata"}


# ── Helpers ───────────────────────────────────────────────────────────────────

class _DummyUpload:
    def __init__(self, data: bytes, filename: str):
        self.filename = filename
        self.file = BytesIO(data)


def _get(docx_file: Path, mode: str = "structural", **kwargs) -> list[dict]:
    return get_chunks(str(docx_file), mode=mode, **kwargs)


def _assert_standard_schema(chunks: list[dict], label: str = "") -> None:
    tag = f" [{label}]" if label else ""
    assert len(chunks) > 0, f"Expected at least one chunk{tag}"
    for c in chunks:
        assert STANDARD_KEYS.issubset(c.keys()), (
            f"Missing keys{tag}: {STANDARD_KEYS - c.keys()}"
        )
        assert isinstance(c["content"], str) and c["content"].strip(), (
            f"content must be non-empty str{tag}"
        )
        assert isinstance(c["content_type"], str), f"content_type must be str{tag}"
        assert isinstance(c["metadata"], dict), f"metadata must be dict{tag}"
        assert "text" not in c, f"Old 'text' key present{tag}"
        assert "strategy" not in c, f"Old 'strategy' key present{tag}"


# ════════════════════════════════════════════════════════════════════════════
# 1. DEFAULT / STRUCTURAL
# ════════════════════════════════════════════════════════════════════════════

@pytest.mark.parametrize("docx_file", ALL_DOCX_FILES, ids=DOCX_IDS)
class TestDocxStructural:

    def test_standard_schema(self, docx_file):
        _assert_standard_schema(_get(docx_file, "structural"), docx_file.name)

    def test_default_alias_equals_structural(self, docx_file):
        default = _get(docx_file, "default")
        structural = _get(docx_file, "structural")
        assert len(default) == len(structural), (
            f"{docx_file.name}: default={len(default)} structural={len(structural)}"
        )

    def test_valid_content_types(self, docx_file):
        valid = {
            "heading", "mixed_content", "table", "plain_paragraph",
            "bullet_list", "code_block", "long_single_paragraph",
            "short_disconnected_paragraph", "image", "footnote_caption",
            "header_footer",
        }
        for c in _get(docx_file, "structural"):
            assert c["content_type"] in valid, (
                f"{docx_file.name}: unexpected content_type {c['content_type']!r}"
            )

    def test_metadata_fields_present(self, docx_file):
        for c in _get(docx_file, "structural"):
            meta = c["metadata"]
            for key in ("section_heading", "section_heading_level",
                        "page_number", "footnotes", "endnotes", "document_metadata"):
                assert key in meta, (
                    f"{docx_file.name}: missing metadata key '{key}'"
                )

    def test_page_number_is_positive_int(self, docx_file):
        for c in _get(docx_file, "structural"):
            pn = c["metadata"]["page_number"]
            assert isinstance(pn, int) and pn >= 1, (
                f"{docx_file.name}: page_number={pn!r} must be int >= 1"
            )

    def test_section_heading_level_valid(self, docx_file):
        for c in _get(docx_file, "structural"):
            lvl = c["metadata"]["section_heading_level"]
            assert lvl is None or (isinstance(lvl, int) and 1 <= lvl <= MAX_HEADING_LEVEL), (
                f"{docx_file.name}: section_heading_level={lvl!r}"
            )

    def test_section_heading_is_string_or_none(self, docx_file):
        for c in _get(docx_file, "structural"):
            sh = c["metadata"]["section_heading"]
            assert sh is None or isinstance(sh, str), (
                f"{docx_file.name}: section_heading={sh!r}"
            )

    def test_footnotes_and_endnotes_are_lists(self, docx_file):
        for c in _get(docx_file, "structural"):
            assert isinstance(c["metadata"]["footnotes"], list)
            assert isinstance(c["metadata"]["endnotes"], list)

    def test_document_metadata_image_count(self, docx_file):
        dm = _get(docx_file, "structural")[0]["metadata"]["document_metadata"]
        assert "image_count" in dm
        assert isinstance(dm["image_count"], int) and dm["image_count"] >= 0

    def test_streaming_parity(self, docx_file):
        batch = _get(docx_file, "structural")
        streamed = list(stream_chunks(str(docx_file), mode="structural"))
        assert len(batch) == len(streamed), (
            f"{docx_file.name}: batch={len(batch)} stream={len(streamed)}"
        )
        for c in streamed:
            assert STANDARD_KEYS.issubset(c.keys())

    def test_default_streaming_parity(self, docx_file):
        batch = _get(docx_file, "default")
        streamed = list(stream_chunks(str(docx_file), mode="default"))
        assert len(batch) == len(streamed), (
            f"{docx_file.name}: batch={len(batch)} stream={len(streamed)}"
        )


# ════════════════════════════════════════════════════════════════════════════
# 2. SEMANTIC
# ════════════════════════════════════════════════════════════════════════════

@pytest.mark.parametrize("docx_file", ALL_DOCX_FILES, ids=DOCX_IDS)
class TestDocxSemantic:

    def test_standard_schema(self, docx_file):
        _assert_standard_schema(_get(docx_file, "semantic"), docx_file.name)

    def test_content_type_is_semantic(self, docx_file):
        for c in _get(docx_file, "semantic"):
            assert c["content_type"] == "semantic", (
                f"{docx_file.name}: expected 'semantic', got {c['content_type']!r}"
            )

    def test_metadata_fields_present(self, docx_file):
        for c in _get(docx_file, "semantic"):
            for key in ("section_heading", "section_heading_level",
                        "paragraph_count", "merge_reason", "document_metadata"):
                assert key in c["metadata"], (
                    f"{docx_file.name}: missing '{key}'"
                )

    def test_paragraph_count_is_positive(self, docx_file):
        for c in _get(docx_file, "semantic"):
            pc = c["metadata"]["paragraph_count"]
            assert isinstance(pc, int) and pc >= 1, (
                f"{docx_file.name}: paragraph_count={pc!r}"
            )

    def test_merge_reason_is_non_empty_string(self, docx_file):
        for c in _get(docx_file, "semantic"):
            mr = c["metadata"]["merge_reason"]
            assert isinstance(mr, str) and mr, (
                f"{docx_file.name}: merge_reason={mr!r}"
            )

    def test_source_type_is_docx(self, docx_file):
        for c in _get(docx_file, "semantic"):
            assert c["metadata"]["document_metadata"].get("source_type") == "docx", (
                f"{docx_file.name}"
            )

    def test_section_heading_level_valid(self, docx_file):
        for c in _get(docx_file, "semantic"):
            lvl = c["metadata"]["section_heading_level"]
            assert lvl is None or (isinstance(lvl, int) and 1 <= lvl <= MAX_HEADING_LEVEL), (
                f"{docx_file.name}: section_heading_level={lvl!r}"
            )

    def test_streaming_parity(self, docx_file):
        batch = _get(docx_file, "semantic")
        streamed = list(stream_chunks(str(docx_file), mode="semantic"))
        assert len(batch) == len(streamed), (
            f"{docx_file.name}: batch={len(batch)} stream={len(streamed)}"
        )
        for c in streamed:
            assert STANDARD_KEYS.issubset(c.keys())


# ════════════════════════════════════════════════════════════════════════════
# 3. SECTION
# ════════════════════════════════════════════════════════════════════════════

@pytest.mark.parametrize("docx_file", ALL_DOCX_FILES, ids=DOCX_IDS)
class TestDocxSection:

    def test_standard_schema(self, docx_file):
        _assert_standard_schema(_get(docx_file, "section"), docx_file.name)

    def test_content_type_is_section(self, docx_file):
        for c in _get(docx_file, "section"):
            assert c["content_type"] == "section", (
                f"{docx_file.name}: got {c['content_type']!r}"
            )

    def test_metadata_fields_present(self, docx_file):
        for c in _get(docx_file, "section"):
            for key in ("section_heading", "section_heading_level",
                        "section_level", "heading_path", "document_metadata"):
                assert key in c["metadata"], (
                    f"{docx_file.name}: missing '{key}'"
                )

    def test_section_heading_is_non_empty_string(self, docx_file):
        for c in _get(docx_file, "section"):
            sh = c["metadata"]["section_heading"]
            assert isinstance(sh, str) and sh, (
                f"{docx_file.name}: section_heading={sh!r}"
            )

    def test_section_level_is_valid(self, docx_file):
        # 0 = preamble (content before the first heading), 1-6 = heading levels
        for c in _get(docx_file, "section"):
            lvl = c["metadata"]["section_level"]
            assert isinstance(lvl, int) and 0 <= lvl <= MAX_HEADING_LEVEL, (
                f"{docx_file.name}: section_level={lvl!r} must be 0-{MAX_HEADING_LEVEL}"
            )

    def test_section_heading_level_matches_section_level(self, docx_file):
        for c in _get(docx_file, "section"):
            meta = c["metadata"]
            assert meta["section_heading_level"] == meta["section_level"], (
                f"{docx_file.name}: section_heading_level={meta['section_heading_level']!r} "
                f"!= section_level={meta['section_level']!r}"
            )

    def test_heading_path_is_string(self, docx_file):
        for c in _get(docx_file, "section"):
            hp = c["metadata"]["heading_path"]
            assert isinstance(hp, str), (
                f"{docx_file.name}: heading_path type={type(hp)}"
            )

    def test_heading_path_contains_section_heading(self, docx_file):
        for c in _get(docx_file, "section"):
            meta = c["metadata"]
            assert meta["section_heading"] in meta["heading_path"], (
                f"{docx_file.name}: '{meta['section_heading']}' "
                f"not in '{meta['heading_path']}'"
            )

    def test_source_type_is_docx(self, docx_file):
        for c in _get(docx_file, "section"):
            assert c["metadata"]["document_metadata"].get("source_type") == "docx"

    def test_streaming_parity(self, docx_file):
        batch = _get(docx_file, "section")
        streamed = list(stream_chunks(str(docx_file), mode="section"))
        assert len(batch) == len(streamed), (
            f"{docx_file.name}: batch={len(batch)} stream={len(streamed)}"
        )
        for c in streamed:
            assert STANDARD_KEYS.issubset(c.keys())


# ════════════════════════════════════════════════════════════════════════════
# 4. SLIDING_WINDOW
# ════════════════════════════════════════════════════════════════════════════

@pytest.mark.parametrize("docx_file", ALL_DOCX_FILES, ids=DOCX_IDS)
class TestDocxSlidingWindow:

    def test_standard_schema(self, docx_file):
        _assert_standard_schema(
            _get(docx_file, "sliding_window", window_size=3, overlap=1),
            docx_file.name,
        )

    def test_content_type_is_sliding_window(self, docx_file):
        for c in _get(docx_file, "sliding_window", window_size=3, overlap=1):
            assert c["content_type"] == "sliding_window", (
                f"{docx_file.name}: got {c['content_type']!r}"
            )

    def test_metadata_fields_present(self, docx_file):
        for c in _get(docx_file, "sliding_window", window_size=3, overlap=1):
            for key in ("window_index", "window_size", "overlap",
                        "paragraph_indices", "paragraph_meta",
                        "heading_count", "list_item_count"):
                assert key in c["metadata"], (
                    f"{docx_file.name}: missing '{key}'"
                )

    def test_window_index_increments_from_zero(self, docx_file):
        chunks = _get(docx_file, "sliding_window", window_size=3, overlap=1)
        indices = [c["metadata"]["window_index"] for c in chunks]
        assert indices == list(range(len(chunks))), (
            f"{docx_file.name}: window_index={indices}"
        )

    def test_window_size_matches_requested(self, docx_file):
        for ws in [2, 3, 4]:
            chunks = _get(docx_file, "sliding_window", window_size=ws, overlap=0)
            for c in chunks:
                assert c["metadata"]["window_size"] == ws, (
                    f"{docx_file.name}: ws={ws} got {c['metadata']['window_size']}"
                )

    def test_overlap_matches_requested(self, docx_file):
        chunks = _get(docx_file, "sliding_window", window_size=4, overlap=2)
        for c in chunks:
            assert c["metadata"]["overlap"] == 2, (
                f"{docx_file.name}: overlap={c['metadata']['overlap']!r}"
            )

    def test_paragraph_indices_is_list_of_ints(self, docx_file):
        chunks = _get(docx_file, "sliding_window", window_size=3, overlap=1)
        for c in chunks:
            pi = c["metadata"]["paragraph_indices"]
            assert isinstance(pi, list) and all(isinstance(x, int) for x in pi), (
                f"{docx_file.name}: paragraph_indices={pi!r}"
            )

    def test_paragraph_meta_structure(self, docx_file):
        chunks = _get(docx_file, "sliding_window", window_size=3, overlap=1)
        for c in chunks:
            for entry in c["metadata"]["paragraph_meta"]:
                for key in ("index", "is_heading", "is_list", "is_table"):
                    assert key in entry, (
                        f"{docx_file.name}: paragraph_meta missing '{key}'"
                    )

    def test_heading_count_is_non_negative(self, docx_file):
        chunks = _get(docx_file, "sliding_window", window_size=3, overlap=1)
        for c in chunks:
            hc = c["metadata"]["heading_count"]
            assert isinstance(hc, int) and hc >= 0, (
                f"{docx_file.name}: heading_count={hc!r}"
            )

    def test_invalid_window_size_zero_raises(self, docx_file):
        with pytest.raises(ValueError):
            _get(docx_file, "sliding_window", window_size=0, overlap=0)

    def test_invalid_overlap_gte_window_size_raises(self, docx_file):
        with pytest.raises(ValueError):
            _get(docx_file, "sliding_window", window_size=3, overlap=3)

    def test_streaming_parity(self, docx_file):
        batch = _get(docx_file, "sliding_window", window_size=3, overlap=1)
        streamed = list(stream_chunks(str(docx_file), mode="sliding_window"))
        assert len(batch) == len(streamed), (
            f"{docx_file.name}: batch={len(batch)} stream={len(streamed)}"
        )
        for c in streamed:
            assert STANDARD_KEYS.issubset(c.keys())


# ════════════════════════════════════════════════════════════════════════════
# 5. SENTENCE
# ════════════════════════════════════════════════════════════════════════════

@pytest.mark.parametrize("docx_file", ALL_DOCX_FILES, ids=DOCX_IDS)
class TestDocxSentence:

    def test_standard_schema(self, docx_file):
        _assert_standard_schema(
            _get(docx_file, "sentence", sentences_per_chunk=2),
            docx_file.name,
        )

    def test_content_type_is_sentence(self, docx_file):
        for c in _get(docx_file, "sentence", sentences_per_chunk=2):
            assert c["content_type"] == "sentence", (
                f"{docx_file.name}: got {c['content_type']!r}"
            )

    def test_metadata_fields_present(self, docx_file):
        for c in _get(docx_file, "sentence", sentences_per_chunk=2):
            for key in ("actual_sentence_count", "chunk_index", "sentences_per_chunk",
                        "source_paragraph_index", "source_paragraph_is_heading",
                        "source_paragraph_is_list", "source_paragraph_is_table",
                        "source_paragraph_heading_level"):
                assert key in c["metadata"], (
                    f"{docx_file.name}: missing '{key}'"
                )

    def test_actual_sentence_count_respects_limit(self, docx_file):
        for spc in [1, 2, 3]:
            for c in _get(docx_file, "sentence", sentences_per_chunk=spc):
                asc = c["metadata"]["actual_sentence_count"]
                assert isinstance(asc, int) and 1 <= asc <= spc, (
                    f"{docx_file.name}: actual_sentence_count={asc} limit={spc}"
                )

    def test_sentences_per_chunk_echoed_in_metadata(self, docx_file):
        for spc in [1, 2, 3]:
            for c in _get(docx_file, "sentence", sentences_per_chunk=spc):
                assert c["metadata"]["sentences_per_chunk"] == spc, (
                    f"{docx_file.name}: spc={spc}"
                )

    def test_chunk_index_increments_from_zero(self, docx_file):
        chunks = _get(docx_file, "sentence", sentences_per_chunk=2)
        indices = [c["metadata"]["chunk_index"] for c in chunks]
        assert indices == list(range(len(chunks))), (
            f"{docx_file.name}: chunk_index={indices}"
        )

    def test_source_paragraph_index_non_negative(self, docx_file):
        for c in _get(docx_file, "sentence", sentences_per_chunk=2):
            idx = c["metadata"]["source_paragraph_index"]
            assert isinstance(idx, int) and idx >= 0, (
                f"{docx_file.name}: source_paragraph_index={idx!r}"
            )

    def test_source_paragraph_flags_are_bools(self, docx_file):
        for c in _get(docx_file, "sentence", sentences_per_chunk=2):
            meta = c["metadata"]
            for flag in ("source_paragraph_is_heading",
                         "source_paragraph_is_list",
                         "source_paragraph_is_table"):
                assert isinstance(meta[flag], bool), (
                    f"{docx_file.name}: {flag}={meta[flag]!r} must be bool"
                )

    def test_heading_level_valid_when_heading(self, docx_file):
        for c in _get(docx_file, "sentence", sentences_per_chunk=2):
            meta = c["metadata"]
            if meta["source_paragraph_is_heading"]:
                lvl = meta["source_paragraph_heading_level"]
                assert isinstance(lvl, int) and 1 <= lvl <= MAX_HEADING_LEVEL, (
                    f"{docx_file.name}: heading_level={lvl!r} for heading chunk"
                )

    def test_invalid_sentences_per_chunk_zero_raises(self, docx_file):
        with pytest.raises(ValueError):
            _get(docx_file, "sentence", sentences_per_chunk=0)

    def test_invalid_sentences_per_chunk_negative_raises(self, docx_file):
        with pytest.raises(ValueError):
            _get(docx_file, "sentence", sentences_per_chunk=-1)

    def test_streaming_parity(self, docx_file):
        batch = _get(docx_file, "sentence", sentences_per_chunk=3)
        streamed = list(stream_chunks(str(docx_file), mode="sentence"))
        assert len(batch) == len(streamed), (
            f"{docx_file.name}: batch={len(batch)} stream={len(streamed)}"
        )
        for c in streamed:
            assert STANDARD_KEYS.issubset(c.keys())


# ════════════════════════════════════════════════════════════════════════════
# 6. PAGE_AWARE
# ════════════════════════════════════════════════════════════════════════════

@pytest.mark.parametrize("docx_file", ALL_DOCX_FILES, ids=DOCX_IDS)
class TestDocxPageAware:

    def test_standard_schema(self, docx_file):
        _assert_standard_schema(
            _get(docx_file, "page_aware", paragraphs_per_page=5),
            docx_file.name,
        )

    def test_content_type_is_page_aware(self, docx_file):
        for c in _get(docx_file, "page_aware", paragraphs_per_page=5):
            assert c["content_type"] == "page_aware", (
                f"{docx_file.name}: got {c['content_type']!r}"
            )

    def test_metadata_fields_present(self, docx_file):
        for c in _get(docx_file, "page_aware", paragraphs_per_page=5):
            for key in ("page_number", "page_break_type", "paragraph_count",
                        "headings", "list_item_count", "table_count",
                        "section_heading_level"):
                assert key in c["metadata"], (
                    f"{docx_file.name}: missing '{key}'"
                )

    def test_page_break_type_valid(self, docx_file):
        valid = {"heading_boundary", "estimated", "explicit", "rendered", "section"}
        for c in _get(docx_file, "page_aware", paragraphs_per_page=5):
            pbt = c["metadata"]["page_break_type"]
            assert pbt in valid, (
                f"{docx_file.name}: page_break_type={pbt!r}"
            )

    def test_page_number_is_positive_int(self, docx_file):
        for c in _get(docx_file, "page_aware", paragraphs_per_page=5):
            pn = c["metadata"]["page_number"]
            assert isinstance(pn, int) and pn >= 1, (
                f"{docx_file.name}: page_number={pn!r}"
            )

    def test_paragraph_count_is_positive(self, docx_file):
        for c in _get(docx_file, "page_aware", paragraphs_per_page=5):
            pc = c["metadata"]["paragraph_count"]
            assert isinstance(pc, int) and pc >= 1, (
                f"{docx_file.name}: paragraph_count={pc!r}"
            )

    def test_headings_list_structure(self, docx_file):
        for c in _get(docx_file, "page_aware", paragraphs_per_page=5):
            for entry in c["metadata"]["headings"]:
                assert "text" in entry and "level" in entry, (
                    f"{docx_file.name}: headings entry missing keys: {entry}"
                )

    def test_list_and_table_counts_non_negative(self, docx_file):
        for c in _get(docx_file, "page_aware", paragraphs_per_page=5):
            meta = c["metadata"]
            assert isinstance(meta["list_item_count"], int) and meta["list_item_count"] >= 0
            assert isinstance(meta["table_count"], int) and meta["table_count"] >= 0

    def test_larger_page_size_produces_fewer_or_equal_chunks(self, docx_file):
        small = _get(docx_file, "page_aware", paragraphs_per_page=3)
        large = _get(docx_file, "page_aware", paragraphs_per_page=30)
        assert len(small) >= len(large), (
            f"{docx_file.name}: small={len(small)} large={len(large)}"
        )

    def test_invalid_paragraphs_per_page_zero_raises(self, docx_file):
        with pytest.raises(ValueError):
            _get(docx_file, "page_aware", paragraphs_per_page=0)

    def test_streaming_parity(self, docx_file):
        batch = _get(docx_file, "page_aware", paragraphs_per_page=15)
        streamed = list(stream_chunks(str(docx_file), mode="page_aware"))
        assert len(batch) == len(streamed), (
            f"{docx_file.name}: batch={len(batch)} stream={len(streamed)}"
        )
        for c in streamed:
            assert STANDARD_KEYS.issubset(c.keys())


# ════════════════════════════════════════════════════════════════════════════
# 7. SOURCE API EQUIVALENCE
# ════════════════════════════════════════════════════════════════════════════

@pytest.mark.parametrize("docx_file", ALL_DOCX_FILES, ids=DOCX_IDS)
class TestDocxSourceAPIs:

    def test_path_vs_bytes_equivalence(self, docx_file):
        by_path = get_chunks(str(docx_file))
        by_bytes = get_chunks_from_bytes(docx_file.read_bytes(), docx_file.name)
        assert len(by_path) == len(by_bytes), (
            f"{docx_file.name}: path={len(by_path)} bytes={len(by_bytes)}"
        )

    def test_path_vs_fileobj_equivalence(self, docx_file):
        by_path = get_chunks(str(docx_file))
        with open(docx_file, "rb") as f:
            by_fileobj = get_chunks_from_fileobj(f, filename=docx_file.name)
        assert len(by_path) == len(by_fileobj), (
            f"{docx_file.name}: path={len(by_path)} fileobj={len(by_fileobj)}"
        )

    def test_path_vs_upload_equivalence(self, docx_file):
        by_path = get_chunks(str(docx_file))
        upload = _DummyUpload(docx_file.read_bytes(), docx_file.name)
        by_upload = get_chunks_from_upload(upload)
        assert len(by_path) == len(by_upload), (
            f"{docx_file.name}: path={len(by_path)} upload={len(by_upload)}"
        )

    def test_wrong_extension_raises_value_error(self, docx_file, tmp_path):
        wrong_ext = tmp_path / f"{docx_file.stem}.pdf"
        wrong_ext.write_bytes(docx_file.read_bytes())
        with pytest.raises(ValueError):
            chunk_docx(str(wrong_ext))

    def test_invalid_mode_raises_value_error(self, docx_file):
        with pytest.raises(ValueError):
            get_chunks(str(docx_file), mode="not_a_real_mode")

    def test_deterministic_output(self, docx_file):
        first = get_chunks(str(docx_file), mode="structural")
        second = get_chunks(str(docx_file), mode="structural")
        assert len(first) == len(second), f"{docx_file.name}: non-deterministic count"
        assert [c["content_type"] for c in first] == [c["content_type"] for c in second], (
            f"{docx_file.name}: non-deterministic content_types"
        )

    def test_timing_dict_keys_and_types(self, docx_file):
        _, timing = chunk_docx(str(docx_file))
        assert "rust_ms" in timing and "python_ms" in timing
        assert isinstance(timing["rust_ms"], float) and timing["rust_ms"] >= 0
        assert isinstance(timing["python_ms"], float) and timing["python_ms"] >= 0


# ════════════════════════════════════════════════════════════════════════════
# 8. STREAMING PARAM VALIDATION
# ════════════════════════════════════════════════════════════════════════════

@pytest.mark.parametrize("docx_file", ALL_DOCX_FILES, ids=DOCX_IDS)
class TestDocxStreamingValidation:

    def test_sliding_window_window_size_zero_raises(self, docx_file):
        with pytest.raises(ValueError):
            stream_chunk_docx(
                str(docx_file),
                mode="sliding_window",
                window_size=0,
                overlap=0,
            )

    def test_sliding_window_overlap_gte_window_size_raises(self, docx_file):
        with pytest.raises(ValueError):
            stream_chunk_docx(
                str(docx_file),
                mode="sliding_window",
                window_size=3,
                overlap=3,
            )

    def test_sentence_sentences_per_chunk_zero_raises(self, docx_file):
        with pytest.raises(ValueError):
            stream_chunk_docx(
                str(docx_file),
                mode="sentence",
                sentences_per_chunk=0,
            )

    def test_invalid_mode_raises(self, docx_file):
        with pytest.raises((ValueError, NotImplementedError)):
            stream_chunk_docx(str(docx_file), mode="not_a_real_mode")


@pytest.mark.parametrize("docx_file", ALL_DOCX_FILES, ids=DOCX_IDS)
class TestDocxStreamingSourceEquivalence:

    def test_stream_bytes_equals_path(self, docx_file):
        by_path = list(stream_chunk_docx(str(docx_file), mode="structural"))
        data = Path(docx_file).read_bytes()
        by_bytes = list(
            stream_chunks_from_bytes(data, filename="x.docx", mode="structural")
        )
        assert [c["content"] for c in by_bytes] == [c["content"] for c in by_path]

    def test_stream_fileobj_equals_path(self, docx_file):
        by_path = list(stream_chunk_docx(str(docx_file), mode="structural"))
        with open(docx_file, "rb") as f:
            by_fileobj = list(
                stream_chunks_from_fileobj(f, filename="x.docx", mode="structural")
            )
        assert [c["content"] for c in by_fileobj] == [c["content"] for c in by_path]


# ════════════════════════════════════════════════════════════════════════════
# 9. MISSING FILE (runs once — not parametrized per file)
# ════════════════════════════════════════════════════════════════════════════

def test_missing_file_raises_file_not_found():
    with pytest.raises(FileNotFoundError):
        get_chunks("/nonexistent/path/to/file.docx")


@pytest.mark.skipif(
    not ADVERSARIAL_DOCX_FILES, reason="no adversarial DOCX fixtures present"
)
@pytest.mark.parametrize(
    "docx_file",
    ADVERSARIAL_DOCX_FILES,
    ids=[f.name for f in ADVERSARIAL_DOCX_FILES],
)
def test_adversarial_fixtures_raise_cleanly(docx_file):
    """Encrypted/malformed DOCX must fail with a catchable error, never a panic.

    These are excluded from ALL_DOCX_FILES because every other parametrized test
    assumes its fixture parses. Refusing them is correct behaviour, so it is
    asserted here explicitly rather than left untested.
    """
    with pytest.raises((RuntimeError, ValueError)):
        get_chunks(str(docx_file))


# ── A complete sentence is not a "disconnected fragment" (#14) ────────────────

RTL_CJK_DOCX = (_DOCX_DIR / "corpus_rtl_cjk.docx") if _DOCX_DIR else Path("missing")


@pytest.mark.skipif(not RTL_CJK_DOCX.exists(), reason="missing corpus_rtl_cjk.docx")
def test_dense_script_paragraphs_are_not_merged():
    """Each language paragraph must survive as its own chunk.

    The short-paragraph bucket aggregates stray fragments. A whole sentence in
    a dense script is short by any length measure — 20 characters of Chinese,
    60 bytes — so length alone merged five distinct paragraphs into one.
    """
    chunks = get_chunks(str(RTL_CJK_DOCX), mode="default")
    assert len(chunks) == 6, [c["content"][:30] for c in chunks]
    joined = [c["content"] for c in chunks]
    assert any("العربية" in c for c in joined)
    assert any("בעברית" in c for c in joined)
    assert any("日本語の段落です" in c for c in joined)
    assert any("这是一个中文段落" in c for c in joined)


@pytest.mark.skipif(not RTL_CJK_DOCX.exists(), reason="missing corpus_rtl_cjk.docx")
def test_a_fragment_without_terminal_punctuation_stays_short():
    """The trailing mixed line ends mid-thought and must still aggregate."""
    chunks = get_chunks(str(RTL_CJK_DOCX), mode="default")
    last = chunks[-1]
    assert last["content_type"] == "short_disconnected_paragraph", last
