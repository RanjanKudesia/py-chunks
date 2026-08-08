from __future__ import annotations

import re
from pathlib import Path

import pytest

from py_chunks import MarkdownResult, get_markdown


# Real fixtures live at <repo>/test_files. Pick image-bearing docx/pptx so the
# image-reference assertions actually exercise, not just the empty-image path.
_TEST_FILES = Path(__file__).resolve().parents[2] / "test_files"
DOCX_FIXTURE = _TEST_FILES / "docx" / "all_round.docx"
PPTX_FIXTURE = _TEST_FILES / "pptx" / "sample.pptx"


def _first_existing_pdf() -> Path | None:
    candidates = sorted((_TEST_FILES / "pdf").glob("*.pdf"))
    return candidates[0] if candidates else None


PDF_FIXTURE = _first_existing_pdf()


def test_get_markdown_missing_docx_raises():
    with pytest.raises(FileNotFoundError):
        get_markdown("missing.docx")


def test_get_markdown_missing_docx_with_images_raises():
    with pytest.raises(FileNotFoundError):
        get_markdown("missing.docx", list_images=True)


def test_get_markdown_unknown_extension_raises(tmp_path):
    p = tmp_path / "file.xyz"
    p.write_text("x", encoding="utf-8")
    with pytest.raises(ValueError):
        get_markdown(str(p))


def test_get_markdown_bytes_without_filename_raises():
    with pytest.raises(ValueError):
        get_markdown(b"data")


def test_get_markdown_bytes_with_filename_list_images_flag_routes():
    # Parsing may fail for fake DOCX bytes, but list_images routing itself should not TypeError.
    try:
        get_markdown(b"data", filename="f.docx", list_images=False)
    except TypeError as exc:
        pytest.fail(f"list_images routing raised TypeError unexpectedly: {exc}")
    except (RuntimeError, ValueError, FileNotFoundError, OSError):
        pass


@pytest.mark.skipif(PDF_FIXTURE is None, reason="No PDF fixture available")
def test_get_markdown_pdf_list_images_returns_result_with_empty_images():
    result = get_markdown(str(PDF_FIXTURE), list_images=True)
    assert isinstance(result, MarkdownResult)
    assert isinstance(result.markdown, str)
    assert result.images == {}


@pytest.mark.skipif(PDF_FIXTURE is None, reason="No PDF fixture available")
def test_get_markdown_pdf_default_returns_str():
    result = get_markdown(str(PDF_FIXTURE))
    assert isinstance(result, str)


@pytest.mark.skipif(PDF_FIXTURE is None, reason="No PDF fixture available")
def test_get_markdown_pdf_list_images_false_returns_str():
    result = get_markdown(str(PDF_FIXTURE), list_images=False)
    assert isinstance(result, str)


@pytest.mark.skipif(not DOCX_FIXTURE.exists(), reason="Missing ../test_files/docx/all_round.docx")
def test_docx_get_markdown_with_images_result_shape():
    result = get_markdown(str(DOCX_FIXTURE), list_images=True)
    assert isinstance(result, MarkdownResult)
    assert isinstance(result.markdown, str)
    assert result.markdown.strip() != ""
    assert isinstance(result.images, dict)


@pytest.mark.skipif(not DOCX_FIXTURE.exists(), reason="Missing ../test_files/docx/all_round.docx")
def test_docx_image_keys_values_and_markdown_refs():
    result = get_markdown(str(DOCX_FIXTURE), list_images=True)
    pattern = re.compile(r"^[0-9a-f]{16}\.(png|jpg|jpeg|gif|webp)$")
    for key, value in result.images.items():
        assert pattern.match(key)
        assert isinstance(value, bytes)
        assert len(value) > 0
    if result.images:
        for key in result.images:
            assert f"![]({key})" in result.markdown


@pytest.mark.skipif(not DOCX_FIXTURE.exists(), reason="Missing ../test_files/docx/all_round.docx")
def test_docx_get_markdown_default_and_false_are_str():
    assert isinstance(get_markdown(str(DOCX_FIXTURE)), str)
    assert isinstance(get_markdown(str(DOCX_FIXTURE), list_images=False), str)


@pytest.mark.skipif(not DOCX_FIXTURE.exists(), reason="Missing ../test_files/docx/all_round.docx")
def test_docx_bytes_and_filelike_sources_with_images():
    data = DOCX_FIXTURE.read_bytes()
    from_bytes = get_markdown(data, filename=DOCX_FIXTURE.name, list_images=True)
    assert isinstance(from_bytes, MarkdownResult)

    with open(DOCX_FIXTURE, "rb") as fh:
        from_file_obj = get_markdown(fh, list_images=True)
    assert isinstance(from_file_obj, MarkdownResult)


@pytest.mark.skipif(not PPTX_FIXTURE.exists(), reason="Missing ../test_files/pptx/sample.pptx")
def test_pptx_get_markdown_with_images_result_shape():
    result = get_markdown(str(PPTX_FIXTURE), list_images=True)
    assert isinstance(result, MarkdownResult)
    assert isinstance(result.markdown, str)
    assert result.markdown.strip() != ""
    assert isinstance(result.images, dict)


@pytest.mark.skipif(not PPTX_FIXTURE.exists(), reason="Missing ../test_files/pptx/sample.pptx")
def test_pptx_image_keys_values_and_markdown_refs():
    result = get_markdown(str(PPTX_FIXTURE), list_images=True)
    pattern = re.compile(r"^[0-9a-f]{16}\.(png|jpg|jpeg|gif|webp)$")
    for key, value in result.images.items():
        assert pattern.match(key)
        assert isinstance(value, bytes)
        assert len(value) > 0
    if result.images:
        for key in result.images:
            assert f"![]({key})" in result.markdown


@pytest.mark.skipif(not PPTX_FIXTURE.exists(), reason="Missing ../test_files/pptx/sample.pptx")
def test_pptx_get_markdown_default_and_false_are_str():
    assert isinstance(get_markdown(str(PPTX_FIXTURE)), str)
    assert isinstance(get_markdown(str(PPTX_FIXTURE), list_images=False), str)


@pytest.mark.skipif(not PPTX_FIXTURE.exists(), reason="Missing ../test_files/pptx/sample.pptx")
def test_pptx_bytes_and_filelike_sources_with_images():
    data = PPTX_FIXTURE.read_bytes()
    from_bytes = get_markdown(data, filename=PPTX_FIXTURE.name, list_images=True)
    assert isinstance(from_bytes, MarkdownResult)

    with open(PPTX_FIXTURE, "rb") as fh:
        from_file_obj = get_markdown(fh, list_images=True)
    assert isinstance(from_file_obj, MarkdownResult)
