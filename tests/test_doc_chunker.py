import pytest
from pathlib import Path

# Real fixtures live at <repo>/test_files (see _spreadsheet_fixtures.py). The
# perf fixtures (1mb/100mb .doc) are excluded so the suite stays fast; they are
# exercised by the dedicated performance benchmark, not the functional matrix.
FIXTURES_DIR = Path(__file__).resolve().parents[2] / "test_files" / "doc"
_MAX_FIXTURE_BYTES = 5 * 1024 * 1024


def doc_files():
    if not FIXTURES_DIR.exists():
        return []
    return [
        p
        for p in sorted(FIXTURES_DIR.glob("*.doc"))
        if p.stat().st_size <= _MAX_FIXTURE_BYTES
    ]


has_fixtures = len(doc_files()) > 0

from py_chunks.chunkers.doc import chunk_doc, stream_chunk_doc
from py_chunks import get_chunks_from_path


def test_file_not_found_raises():
    with pytest.raises(FileNotFoundError):
        chunk_doc("/nonexistent/file.doc")


def test_wrong_extension_raises(tmp_path):
    f = tmp_path / "test.docx"
    f.write_bytes(b"fake")
    with pytest.raises(ValueError, match=r"\.doc"):
        chunk_doc(str(f))


def test_invalid_mode_raises(tmp_path):
    f = tmp_path / "test.doc"
    f.write_bytes(b"fake")
    with pytest.raises(ValueError):
        chunk_doc(str(f), mode="nonexistent_mode")


def test_sliding_window_bad_args(tmp_path):
    f = tmp_path / "test.doc"
    f.write_bytes(b"fake")
    with pytest.raises(ValueError):
        chunk_doc(str(f), mode="sliding_window", window_size=0)
    with pytest.raises(ValueError):
        chunk_doc(str(f), mode="sliding_window", window_size=2, overlap=2)


def test_import_chunk_doc():
    from py_chunks.chunkers.doc import chunk_doc as fn

    assert callable(fn)


def test_import_stream_chunk_doc():
    from py_chunks.chunkers.doc import stream_chunk_doc as fn

    assert callable(fn)


def test_get_chunks_from_path_dispatch_doc(tmp_path):
    f = tmp_path / "test.doc"
    f.write_bytes(b"\x00" * 16)
    with pytest.raises(Exception):
        get_chunks_from_path(str(f))


@pytest.mark.skipif(not has_fixtures, reason="No .doc fixtures in ../test_files/doc/")
@pytest.mark.parametrize("doc_path", doc_files())
def test_chunk_doc_default_returns_list(doc_path):
    chunks, timing = chunk_doc(str(doc_path))
    assert isinstance(chunks, list)
    assert len(chunks) > 0


@pytest.mark.skipif(not has_fixtures, reason="No .doc fixtures in ../test_files/doc/")
@pytest.mark.parametrize("doc_path", doc_files())
def test_chunk_doc_chunk_shape(doc_path):
    chunks, timing = chunk_doc(str(doc_path))
    for c in chunks:
        assert "content" in c
        assert "content_type" in c
        assert "metadata" in c
        assert isinstance(c["content"], str)


@pytest.mark.skipif(not has_fixtures, reason="No .doc fixtures in ../test_files/doc/")
@pytest.mark.parametrize("doc_path", doc_files())
def test_chunk_doc_timing_keys(doc_path):
    _, timing = chunk_doc(str(doc_path))
    assert "rust_ms" in timing
    assert "python_ms" in timing


@pytest.mark.skipif(not has_fixtures, reason="No .doc fixtures in ../test_files/doc/")
@pytest.mark.parametrize("doc_path", doc_files())
def test_chunk_doc_section_mode(doc_path):
    chunks, _ = chunk_doc(str(doc_path), mode="section")
    assert isinstance(chunks, list)


@pytest.mark.skipif(not has_fixtures, reason="No .doc fixtures in ../test_files/doc/")
@pytest.mark.parametrize("doc_path", doc_files())
def test_chunk_doc_semantic_mode(doc_path):
    chunks, _ = chunk_doc(str(doc_path), mode="semantic")
    assert isinstance(chunks, list)


@pytest.mark.skipif(not has_fixtures, reason="No .doc fixtures in ../test_files/doc/")
@pytest.mark.parametrize("doc_path", doc_files())
def test_chunk_doc_sliding_window_mode(doc_path):
    chunks, _ = chunk_doc(str(doc_path), mode="sliding_window", window_size=3, overlap=1)
    assert isinstance(chunks, list)


@pytest.mark.skipif(not has_fixtures, reason="No .doc fixtures in ../test_files/doc/")
@pytest.mark.parametrize("doc_path", doc_files())
def test_chunk_doc_sentence_mode(doc_path):
    chunks, _ = chunk_doc(str(doc_path), mode="sentence", sentences_per_chunk=3)
    assert isinstance(chunks, list)


@pytest.mark.skipif(not has_fixtures, reason="No .doc fixtures in ../test_files/doc/")
@pytest.mark.parametrize("doc_path", doc_files())
def test_chunk_doc_page_aware_mode(doc_path):
    chunks, _ = chunk_doc(str(doc_path), mode="page_aware", paragraphs_per_page=10)
    assert isinstance(chunks, list)


@pytest.mark.skipif(not has_fixtures, reason="No .doc fixtures in ../test_files/doc/")
@pytest.mark.parametrize("doc_path", doc_files())
def test_stream_chunk_doc_default(doc_path):
    it = stream_chunk_doc(str(doc_path))
    chunks = list(it)
    assert len(chunks) > 0
    assert all("content" in c for c in chunks)


@pytest.mark.skipif(not has_fixtures, reason="No .doc fixtures in ../test_files/doc/")
@pytest.mark.parametrize("doc_path", doc_files())
def test_stream_matches_batch(doc_path):
    batch, _ = chunk_doc(str(doc_path))
    streamed = list(stream_chunk_doc(str(doc_path)))
    assert len(batch) == len(streamed)
    for b, s in zip(batch, streamed):
        assert b["content"] == s["content"]


@pytest.mark.skipif(not has_fixtures, reason="No .doc fixtures in ../test_files/doc/")
@pytest.mark.parametrize("doc_path", doc_files())
def test_no_oversized_chunks_and_no_nul(doc_path):
    """Regression: a paragraph with no breaks (e.g. examplefile.com's padded
    1mb.doc) must not surface as one unsplittable mega-chunk, and NUL/control
    bytes must never leak into extracted text."""
    chunks, _ = chunk_doc(str(doc_path))
    # Cap is MAX_CHUNK_CHARS (1200) plus a small tolerance for trim/boundary slack.
    for c in chunks:
        assert len(c["content"]) <= 1400, (
            f"{doc_path.name}: oversized chunk of {len(c['content'])} chars"
        )
        assert "\x00" not in c["content"], f"{doc_path.name}: NUL leaked into text"
