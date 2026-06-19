import pytest
from pathlib import Path

FIXTURES_DIR = Path(__file__).parent / "fixtures"


def doc_files():
    if not FIXTURES_DIR.exists():
        return []
    return list(FIXTURES_DIR.glob("*.doc"))


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


@pytest.mark.skipif(not has_fixtures, reason="No .doc fixtures in tests/fixtures/")
@pytest.mark.parametrize("doc_path", doc_files())
def test_chunk_doc_default_returns_list(doc_path):
    chunks, timing = chunk_doc(str(doc_path))
    assert isinstance(chunks, list)
    assert len(chunks) > 0


@pytest.mark.skipif(not has_fixtures, reason="No .doc fixtures in tests/fixtures/")
@pytest.mark.parametrize("doc_path", doc_files())
def test_chunk_doc_chunk_shape(doc_path):
    chunks, timing = chunk_doc(str(doc_path))
    for c in chunks:
        assert "content" in c
        assert "content_type" in c
        assert "metadata" in c
        assert isinstance(c["content"], str)


@pytest.mark.skipif(not has_fixtures, reason="No .doc fixtures in tests/fixtures/")
@pytest.mark.parametrize("doc_path", doc_files())
def test_chunk_doc_timing_keys(doc_path):
    _, timing = chunk_doc(str(doc_path))
    assert "rust_ms" in timing
    assert "python_ms" in timing


@pytest.mark.skipif(not has_fixtures, reason="No .doc fixtures in tests/fixtures/")
@pytest.mark.parametrize("doc_path", doc_files())
def test_chunk_doc_section_mode(doc_path):
    chunks, _ = chunk_doc(str(doc_path), mode="section")
    assert isinstance(chunks, list)


@pytest.mark.skipif(not has_fixtures, reason="No .doc fixtures in tests/fixtures/")
@pytest.mark.parametrize("doc_path", doc_files())
def test_chunk_doc_semantic_mode(doc_path):
    chunks, _ = chunk_doc(str(doc_path), mode="semantic")
    assert isinstance(chunks, list)


@pytest.mark.skipif(not has_fixtures, reason="No .doc fixtures in tests/fixtures/")
@pytest.mark.parametrize("doc_path", doc_files())
def test_chunk_doc_sliding_window_mode(doc_path):
    chunks, _ = chunk_doc(str(doc_path), mode="sliding_window", window_size=3, overlap=1)
    assert isinstance(chunks, list)


@pytest.mark.skipif(not has_fixtures, reason="No .doc fixtures in tests/fixtures/")
@pytest.mark.parametrize("doc_path", doc_files())
def test_chunk_doc_sentence_mode(doc_path):
    chunks, _ = chunk_doc(str(doc_path), mode="sentence", sentences_per_chunk=3)
    assert isinstance(chunks, list)


@pytest.mark.skipif(not has_fixtures, reason="No .doc fixtures in tests/fixtures/")
@pytest.mark.parametrize("doc_path", doc_files())
def test_chunk_doc_page_aware_mode(doc_path):
    chunks, _ = chunk_doc(str(doc_path), mode="page_aware", paragraphs_per_page=10)
    assert isinstance(chunks, list)


@pytest.mark.skipif(not has_fixtures, reason="No .doc fixtures in tests/fixtures/")
@pytest.mark.parametrize("doc_path", doc_files())
def test_stream_chunk_doc_default(doc_path):
    it = stream_chunk_doc(str(doc_path))
    chunks = list(it)
    assert len(chunks) > 0
    assert all("content" in c for c in chunks)


@pytest.mark.skipif(not has_fixtures, reason="No .doc fixtures in tests/fixtures/")
@pytest.mark.parametrize("doc_path", doc_files())
def test_stream_matches_batch(doc_path):
    batch, _ = chunk_doc(str(doc_path))
    streamed = list(stream_chunk_doc(str(doc_path)))
    assert len(batch) == len(streamed)
    for b, s in zip(batch, streamed):
        assert b["content"] == s["content"]
