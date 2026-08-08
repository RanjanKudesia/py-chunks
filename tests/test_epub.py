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


# ── Argument validation (regression: the EPUB facade validated nowhere) ───────
#
# The engine's EPUB facade never ran the shared argument check, and
# `extract.rs::chunk_package` deliberately swallows per-chapter builder
# failures, so an invalid argument produced ZERO CHUNKS instead of an error —
# and py-chunks had no numeric check of its own to catch it either (only a
# mode-set check). A silent empty result is worse than an exception.


def _book():
    return str(_pick("gutenberg_moby_dick.epub"))


def test_bad_args_raise_instead_of_returning_no_chunks():
    # The contrast is the whole point: with a usable overlap the same call
    # returns a real book, so the old `[]` was indistinguishable from "this book
    # has no content" rather than from an error.
    assert len(get_chunks(_book())) > 1000, "sanity: default mode must chunk the book"
    valid = get_chunks(_book(), mode="sliding_window", window_size=100, overlap=20)
    assert len(valid) > 10, f"sanity: a valid call must chunk, got {len(valid)}"
    with pytest.raises(ValueError, match="^overlap must be less than window_size$"):
        get_chunks(_book(), mode="sliding_window", window_size=100, overlap=100)


@pytest.mark.parametrize(
    "kwargs, message",
    [
        (
            {"mode": "sliding_window", "window_size": 0, "overlap": 0},
            "^window_size must be greater than 0$",
        ),
        (
            {"mode": "sliding_window", "window_size": 100, "overlap": 100},
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
def test_every_bad_arg_raises_with_the_engine_wording(kwargs, message):
    with pytest.raises(ValueError, match=message):
        get_chunks(_book(), **kwargs)


@pytest.mark.parametrize(
    "kwargs, message",
    [
        (
            {"mode": "sliding_window", "window_size": 100, "overlap": 100},
            "^overlap must be less than window_size$",
        ),
        (
            {"mode": "page_aware", "paragraphs_per_page": 0},
            "^paragraphs_per_page must be greater than 0$",
        ),
    ],
)
def test_bytes_route_validates_before_parsing(kwargs, message):
    # Validation runs before any parsing, so unparseable bytes still reject on
    # the argument — the check is not hiding behind a successful parse.
    with pytest.raises(ValueError, match=message):
        get_chunks_from_bytes(b"not an epub at all", filename="x.epub", **kwargs)


def test_streaming_route_validates_too():
    with pytest.raises(ValueError, match="^overlap must be less than window_size$"):
        list(stream_chunks(_book(), mode="sliding_window", window_size=100, overlap=100))


def test_with_images_route_validates_too():
    from py_chunks.chunkers.epub import chunk_epub_with_images

    with pytest.raises(ValueError, match="^overlap must be less than window_size$"):
        chunk_epub_with_images(
            _book(), mode="sliding_window", window_size=100, overlap=100
        )
