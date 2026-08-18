# pylint: disable=redefined-outer-name
from pathlib import Path

import pytest

import py_chunks as _py_chunks
from py_chunks import get_chunks, get_markdown, stream_chunks
from py_chunks.chunkers.ppt import ppt_to_markdown

chunk_ppt = getattr(_py_chunks, "chunk_ppt")
stream_chunk_ppt = getattr(_py_chunks, "stream_chunk_ppt")

TEST_FILES_DIR = Path(__file__).resolve().parents[2] / "test_files"
PPT_DIR = TEST_FILES_DIR / "ppt"
PPT_FILES = sorted(PPT_DIR.glob("*.ppt")) if PPT_DIR.exists() else []

if not PPT_FILES:
    pytest.skip("No .ppt fixtures in test_files/ppt/", allow_module_level=True)

MODES = [
    "default",
    "structural",
    "section",
    "semantic",
    "sentence",
    "sliding_window",
    "page_aware",
]

# Modes that emit exactly one chunk per slide.
PER_SLIDE_MODES = ["section", "semantic", "page_aware"]


def _slide_count(fp: Path) -> int:
    # page_aware emits one chunk per slide for these decks.
    chunks, _ = chunk_ppt(str(fp), mode="page_aware")
    return len(chunks)


@pytest.mark.parametrize("fp", PPT_FILES, ids=lambda p: p.name)
@pytest.mark.parametrize("mode", MODES)
def test_all_modes_produce_chunks(fp, mode):
    chunks, timing = chunk_ppt(str(fp), mode=mode)
    assert isinstance(chunks, list)
    assert len(chunks) > 0
    assert "rust_ms" in timing and "python_ms" in timing
    for c in chunks:
        assert set(c) >= {"content", "content_type", "metadata"}
        assert c["content"].strip()


@pytest.mark.parametrize("fp", PPT_FILES, ids=lambda p: p.name)
def test_metadata_is_slide_aware(fp):
    """A deck describes itself in presentation vocabulary (TECH_DEBT #18).

    This test previously asserted that `.ppt` metadata matched `.doc`'s
    exactly, with `page_number` always None — codifying the defect rather than
    the contract. `.ppt` reuses `.doc`'s *chunkers*, which is an implementation
    detail; it should not have made a presentation describe itself as a
    document.
    """
    chunks, _ = chunk_ppt(str(fp))
    meta = chunks[0]["metadata"]
    assert set(meta) == {
        "source",
        "chunk_index",
        "total_chunks",
        "paragraph_type",
        "heading_level",
        "page_number",
        "slide_number",
        "slide_title",
        # Slide titles are headings to the shared builders, so a deck gets the
        # same section trail `.doc` gained in TECH_DEBT #12.
        "section_heading",
        "section_heading_level",
        "heading_path",
        "document_metadata",
    }
    # Slides are real structure, unlike `.doc` pages: every chunk has one.
    assert isinstance(meta["slide_number"], int) and meta["slide_number"] >= 1
    # page_number is the shipped key and means the same thing for a deck.
    assert meta["page_number"] == meta["slide_number"]
    assert meta["document_metadata"]["source_type"] == "ppt"
    assert meta["document_metadata"]["total_slides"] >= meta["slide_number"]


# These four tests were written against three hand-picked decks and asserted
# properties of *those fixtures* rather than of the engine. The 2026-08-14 corpus
# expansion (3 -> 28 real-world decks) falsified them, measured as follows:
#
#   * `section` GROUPS slides under a heading, so it emits FEWER chunks than
#     slides whenever a deck has untitled slides (poi_bug60345_paperfigures:
#     10 slides -> 7 sections; poi_bug60345_suba: 13 -> 12).
#   * `semantic` merges by continuity AND splits at the 1,500-char cap, so it has
#     no fixed relation to slide count in EITHER direction
#     (poi_br_tvcamboriu_pensar: 14 -> 3; poi_42486: 33 slides -> 34 chunks).
#   * Real decks exist with no titled slide and with no bullet list at all.
#
# Only `page_aware` is genuinely one-chunk-per-slide. The assertions below keep
# every real engine invariant and drop only the fixture coincidences — with
# corpus-level guards so nothing can pass vacuously.


@pytest.mark.parametrize("fp", PPT_FILES, ids=lambda p: p.name)
@pytest.mark.parametrize("mode", PER_SLIDE_MODES)
def test_per_slide_mode_chunk_counts(fp, mode):
    n_slides = _slide_count(fp)
    chunks, _ = chunk_ppt(str(fp), mode=mode)
    if mode == "page_aware":
        assert len(chunks) == n_slides
    elif mode == "section":
        # Groups by slide title; never splits a slide.
        assert 1 <= len(chunks) <= n_slides
    else:  # semantic
        assert len(chunks) >= 1


@pytest.mark.parametrize("fp", PPT_FILES, ids=lambda p: p.name)
def test_headings_are_slide_level(fp):
    """Any heading a deck yields must be slide-level; not every deck has one."""
    chunks, _ = chunk_ppt(str(fp), mode="default")
    for h in (c for c in chunks if c["content_type"] == "heading"):
        assert h["metadata"]["heading_level"] == 2


def test_corpus_exercises_titled_decks():
    """Guard: `test_headings_are_slide_level` must not pass vacuously."""
    titled = sum(
        1
        for fp in PPT_FILES
        if any(c["content_type"] == "heading" for c in chunk_ppt(str(fp), mode="default")[0])
    )
    assert titled >= 1, "no deck in the corpus yields a heading"


@pytest.mark.parametrize("fp", PPT_FILES, ids=lambda p: p.name)
def test_markdown_has_slide_separators(fp):
    md = ppt_to_markdown(str(fp))
    assert md.strip()
    if _slide_count(fp) > 1:
        assert "---" in md  # slide separators


@pytest.mark.parametrize("fp", PPT_FILES, ids=lambda p: p.name)
def test_bullets_agree_between_chunks_and_markdown(fp):
    """A deck yielding bullet_list chunks must also render Markdown bullets.

    The implication runs one way: plenty of real decks have no multi-line body
    placeholder and so no bullets at all. What must never happen is the two
    surfaces disagreeing — `get_chunks` seeing a list that `get_markdown` drops.
    """
    chunks, _ = chunk_ppt(str(fp), mode="default")
    if not any(c["content_type"] == "bullet_list" for c in chunks):
        return
    md = ppt_to_markdown(str(fp))
    assert "\n- " in md or md.startswith("- ")


def test_corpus_exercises_bulleted_decks():
    """Guard: `test_bullets_agree_between_chunks_and_markdown` must not pass vacuously."""
    bulleted = sum(
        1
        for fp in PPT_FILES
        if any(c["content_type"] == "bullet_list" for c in chunk_ppt(str(fp), mode="default")[0])
    )
    assert bulleted >= 1, "no deck in the corpus yields a bullet_list chunk"


@pytest.mark.parametrize("fp", PPT_FILES, ids=lambda p: p.name)
def test_streaming_matches_batch_structural(fp):
    batch, _ = chunk_ppt(str(fp), mode="structural")
    streamed = list(stream_chunk_ppt(str(fp), mode="structural"))
    assert len(streamed) == len(batch)
    assert [c["content"] for c in streamed] == [c["content"] for c in batch]


@pytest.mark.parametrize("fp", PPT_FILES, ids=lambda p: p.name)
def test_public_api_dispatch(fp):
    # get_chunks / get_markdown / stream_chunks route .ppt correctly.
    chunks = get_chunks(str(fp))
    assert isinstance(chunks, list) and chunks
    md = get_markdown(str(fp))
    assert isinstance(md, str) and md.strip()
    streamed = list(stream_chunks(str(fp), mode="structural"))
    assert len(streamed) == len(chunks)


def test_wrong_extension_raises():
    pptx = TEST_FILES_DIR / "pptx" / "minimal.pptx"
    if pptx.exists():
        with pytest.raises(ValueError):
            chunk_ppt(str(pptx))


def test_missing_file_raises():
    with pytest.raises(FileNotFoundError):
        chunk_ppt(str(PPT_DIR / "does_not_exist.ppt"))


@pytest.mark.parametrize("fp", PPT_FILES, ids=lambda p: p.name)
def test_invalid_params_raise(fp):
    with pytest.raises(ValueError):
        chunk_ppt(str(fp), mode="bogus")
    with pytest.raises(ValueError):
        chunk_ppt(str(fp), mode="sliding_window", window_size=2, overlap=2)


# ── Streaming argument validation (regression) ────────────────────────────────
#
# `stream_chunk_ppt` ran NO validation: a typo'd mode silently streamed
# default-mode chunks (100 of them) instead of raising, and every numeric
# argument was unchecked. The batch route always validated, so the two entry
# points disagreed about what a valid call even is.


def test_stream_rejects_an_invalid_mode_instead_of_defaulting():
    fp = PPT_FILES[0]
    default_count = len(list(stream_chunk_ppt(str(fp))))
    assert default_count > 0, "sanity: the default mode must stream something"
    with pytest.raises(ValueError, match="^mode must be one of "):
        stream_chunk_ppt(str(fp), mode="not_a_real_mode")


@pytest.mark.parametrize(
    "kwargs, message",
    [
        (
            {"mode": "sliding_window", "window_size": 0, "overlap": 0},
            "^window_size must be greater than 0$",
        ),
        (
            {"mode": "sliding_window", "window_size": 2, "overlap": 2},
            "^overlap must be less than window_size$",
        ),
        (
            {"mode": "sentence", "sentences_per_chunk": 0},
            "^sentences_per_chunk must be greater than 0$",
        ),
        (
            {"mode": "page_aware", "paragraphs_per_page": 0},
            "^paragraphs_per_page must be greater than 0$",
        ),
    ],
)
def test_stream_numeric_validation_uses_the_engine_wording(kwargs, message):
    with pytest.raises(ValueError, match=message):
        stream_chunk_ppt(str(PPT_FILES[0]), **kwargs)


@pytest.mark.parametrize(
    "kwargs, message",
    [
        (
            {"mode": "sliding_window", "window_size": 0, "overlap": 0},
            "^window_size must be greater than 0$",
        ),
        (
            {"mode": "sentence", "sentences_per_chunk": 0},
            "^sentences_per_chunk must be greater than 0$",
        ),
        (
            {"mode": "page_aware", "paragraphs_per_page": 0},
            "^paragraphs_per_page must be greater than 0$",
        ),
    ],
)
def test_batch_and_with_images_share_the_same_wording(kwargs, message):
    from py_chunks.chunkers.ppt import chunk_ppt_with_images

    fp = str(PPT_FILES[0])
    with pytest.raises(ValueError, match=message):
        chunk_ppt(fp, **kwargs)
    with pytest.raises(ValueError, match=message):
        chunk_ppt_with_images(fp, **kwargs)
