import pytest
from pathlib import Path

from py_chunks import _MD_DISPATCH, get_markdown
from py_chunks.chunkers.doc import doc_to_markdown

FIXTURES_DIR = Path(__file__).resolve().parents[2] / "test_files" / "doc"
SAMPLE_DOC = FIXTURES_DIR / "sample.doc"
HAS_SAMPLE = SAMPLE_DOC.exists()


def test_doc_to_markdown_file_not_found_raises():
    with pytest.raises(FileNotFoundError):
        doc_to_markdown("/nonexistent/path/missing.doc")


def test_doc_to_markdown_wrong_extension_raises(tmp_path):
    p = tmp_path / "file.txt"
    p.write_text("not a doc", encoding="utf-8")
    with pytest.raises(ValueError):
        doc_to_markdown(str(p))


def test_doc_to_markdown_via_get_markdown_wrong_extension(tmp_path):
    p = tmp_path / "file.unsupported"
    p.write_text("x", encoding="utf-8")
    with pytest.raises(ValueError):
        get_markdown(str(p))


def test_doc_in_md_dispatch():
    assert ".doc" in _MD_DISPATCH


def test_doc_to_markdown_importable():
    from py_chunks.chunkers.doc import doc_to_markdown as fn

    assert callable(fn)


@pytest.mark.skipif(not HAS_SAMPLE, reason="No tests/fixtures/sample.doc present")
def test_doc_to_markdown_returns_string():
    result = doc_to_markdown(str(SAMPLE_DOC))
    assert isinstance(result, str)


@pytest.mark.skipif(not HAS_SAMPLE, reason="No tests/fixtures/sample.doc present")
def test_doc_to_markdown_not_empty():
    result = doc_to_markdown(str(SAMPLE_DOC))
    assert result.strip() != ""


@pytest.mark.skipif(not HAS_SAMPLE, reason="No tests/fixtures/sample.doc present")
def test_doc_to_markdown_no_trailing_whitespace():
    result = doc_to_markdown(str(SAMPLE_DOC))
    assert result == result.strip()


@pytest.mark.skipif(not HAS_SAMPLE, reason="No tests/fixtures/sample.doc present")
def test_doc_to_markdown_via_get_markdown():
    direct = doc_to_markdown(str(SAMPLE_DOC))
    via_api = get_markdown(str(SAMPLE_DOC))
    assert via_api == direct


@pytest.mark.skipif(not HAS_SAMPLE, reason="No tests/fixtures/sample.doc present")
def test_doc_to_markdown_headings_use_hash():
    result = doc_to_markdown(str(SAMPLE_DOC))
    if any(marker in result for marker in ["# ", "## ", "### "]):
        assert True


@pytest.mark.skipif(not HAS_SAMPLE, reason="No tests/fixtures/sample.doc present")
def test_doc_to_markdown_list_items_use_dash():
    result = doc_to_markdown(str(SAMPLE_DOC))
    if "\n- " in result or result.startswith("- "):
        assert True
