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


@pytest.mark.parametrize("fp", PPT_FILES, ids=lambda p: p.name)
@pytest.mark.parametrize("mode", PER_SLIDE_MODES)
def test_per_slide_modes_equal_slide_count(fp, mode):
    n_slides = _slide_count(fp)
    chunks, _ = chunk_ppt(str(fp), mode=mode)
    assert len(chunks) == n_slides


@pytest.mark.parametrize("fp", PPT_FILES, ids=lambda p: p.name)
def test_titles_detected_as_headings(fp):
    chunks, _ = chunk_ppt(str(fp), mode="default")
    headings = [c for c in chunks if c["content_type"] == "heading"]
    # Every deck in the suite has at least one titled slide.
    assert len(headings) >= 1
    for h in headings:
        assert h["metadata"]["heading_level"] == 2


@pytest.mark.parametrize("fp", PPT_FILES, ids=lambda p: p.name)
def test_markdown_has_headings_and_separators(fp):
    md = ppt_to_markdown(str(fp))
    assert md.strip()
    n_slides = _slide_count(fp)
    if n_slides > 1:
        assert "---" in md  # slide separators
    assert md.count("## ") >= 1  # at least one slide title


@pytest.mark.parametrize("fp", PPT_FILES, ids=lambda p: p.name)
def test_bullets_emitted_for_multi_item_slides(fp):
    # All sample decks have multi-line body placeholders -> bullet lists, which
    # must surface both as bullet_list chunks and as Markdown "- " bullets.
    chunks, _ = chunk_ppt(str(fp), mode="default")
    assert any(c["content_type"] == "bullet_list" for c in chunks)
    md = ppt_to_markdown(str(fp))
    assert "\n- " in md or md.startswith("- ")


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
