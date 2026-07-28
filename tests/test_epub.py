"""Contract tests for .epub (EPUB 2 & 3) — OCF/OPF navigation + HTML chunking."""

import io
import zipfile
from pathlib import Path

import pytest

from py_chunks import get_chunks, get_chunks_from_bytes, get_markdown, stream_chunks
from py_chunks.chunkers.epub import chunk_epub
from _spreadsheet_fixtures import fixtures, require

EPUB = fixtures("epub", "*.epub")
MODES = ["default", "section", "semantic", "sentence", "sliding_window", "page_aware"]
STANDARD_KEYS = {"content", "content_type", "metadata"}


def _pick(name):
    m = [f for f in EPUB if f.name == name]
    require(m)
    return m[0]


def _doc_meta(chunks):
    return chunks[0]["metadata"]["document_metadata"] if chunks else {}


def test_all_fixtures_parse():
    require(EPUB)
    for f in EPUB:
        chunks = get_chunks(str(f))
        assert chunks, f"no chunks for {f.name}"
        for c in chunks:
            assert STANDARD_KEYS.issubset(c.keys())


@pytest.mark.parametrize("mode", MODES)
def test_all_modes(mode):
    chunks, _ = chunk_epub(str(_pick("gutenberg_frankenstein.epub")), mode=mode)
    assert chunks


def test_batch_stream_bytes_agree():
    for f in EPUB:
        batch = get_chunks(str(f))
        streamed = list(stream_chunks(str(f)))
        from_bytes = get_chunks_from_bytes(f.read_bytes(), "x.epub")
        assert len(batch) == len(streamed) == len(from_bytes)


def test_reading_order_follows_spine():
    """Chunks must follow the OPF spine, not zip order. Early-book text must
    precede late-book text."""
    chunks = get_chunks(str(_pick("gutenberg_frankenstein.epub")))
    full = "".join(c["content"] for c in chunks)
    early = full.find("Petersburgh")   # Letter 1, near the start
    late = full.rfind("Walton")        # recurs through to the end
    assert 0 <= early < late, "spine reading order not preserved"


@pytest.mark.parametrize("name,version", [
    ("epubcheck_epub2_minimal.epub", "2.0"),
    ("epubcheck_epub3_minimal.epub", "3.0"),
])
def test_epub2_and_epub3(name, version):
    chunks = get_chunks(str(_pick(name)))
    assert _doc_meta(chunks).get("epub_version") == version


def test_metadata_surfaced():
    chunks = get_chunks(str(_pick("gutenberg_frankenstein.epub")))
    m = _doc_meta(chunks)
    assert m["source_type"] == "epub"
    assert "Frankenstein" in (m["title"] or "")
    assert m["language"] == "en"
    assert "Shelley" in (m["creator"] or "")
    assert m["spine_count"] >= 1


def test_opf_dir_variants_resolve():
    """The OPF lives in different dirs across producers (OEBPS/, EPUB/, OPS/);
    all must resolve and yield content."""
    for name in ("gutenberg_moby_dick.epub", "epubcheck_epub3_minimal.epub", "tika_testEPUB.epub"):
        assert get_chunks(str(_pick(name)))


def test_real_book_prose_quality():
    """A real chapter chunk should be coherent prose, not nav/CSS boilerplate."""
    chunks = get_chunks(str(_pick("gutenberg_pride_and_prejudice.epub")))
    # Normalise whitespace (source line-wraps) and case (drop-cap "IT").
    joined = " ".join(" ".join(c["content"].split()) for c in chunks).lower()
    assert "it is a truth universally acknowledged" in joined
    assert "must be in want of a wife" in joined


def test_chapter_provenance_metadata():
    chunks = get_chunks(str(_pick("gutenberg_frankenstein.epub")))
    assert all("spine_index" in c["metadata"] and "href" in c["metadata"] for c in chunks)
    # spine_index is non-decreasing (reading order)
    idxs = [c["metadata"]["spine_index"] for c in chunks]
    assert idxs == sorted(idxs)


def test_images_extracted():
    result = get_chunks(str(_pick("gutenberg_alice_images.epub")), list_images=True)
    assert len(result.images) >= 1
    assert any(c["content_type"] == "image" for c in result.chunks)


def test_markdown():
    md = get_markdown(str(_pick("gutenberg_sherlock.epub")))
    assert isinstance(md, str) and md.strip()
    assert md.startswith("# ")


def test_non_epub_raises_clean_error(tmp_path):
    fake = tmp_path / "fake.epub"
    fake.write_bytes(b"not an epub")
    with pytest.raises(Exception):  # noqa: B017 — catchable, not a panic
        get_chunks(str(fake))


def test_zip_without_container_raises(tmp_path):
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w") as z:
        z.writestr("hello.txt", "hi")
    fake = tmp_path / "nocont.epub"
    fake.write_bytes(buf.getvalue())
    with pytest.raises(Exception):  # noqa: B017
        get_chunks(str(fake))
