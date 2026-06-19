"""Tests for image-aware chunking and markdown for XLSX via list_images=True."""

import re
from pathlib import Path

import pytest

from py_chunks import ChunksResult, MarkdownResult, get_chunks, get_markdown

XLSX_FIXTURE = Path("../test_files/excel/sample_with_image.xlsx")
XLS_FIXTURE = Path("../test_files/excel/file_example_XLS_10.xls")
IMAGE_KEY_PATTERN = re.compile(r"^[0-9a-f]{16}\.(png|jpg|jpeg|gif|webp)$")

_FIXTURE_EXISTS = XLSX_FIXTURE.exists()
_XLS_FIXTURE_EXISTS = XLS_FIXTURE.exists()
_SKIP = pytest.mark.skipif(
    not _FIXTURE_EXISTS, reason=f"Missing fixture: {XLSX_FIXTURE}")
_SKIP_XLS = pytest.mark.skipif(
    not _XLS_FIXTURE_EXISTS, reason=f"Missing fixture: {XLS_FIXTURE}")

_ALL_XLSX_MODES = ["row", "table", "sheet",
                   "sliding_window", "page_aware", "semantic"]

_XLSX_MODE_KWARGS = {
    "sliding_window": {"window_size": 3, "overlap": 1},
}


# ---------------------------------------------------------------------------
# Always-run smoke tests
# ---------------------------------------------------------------------------


def test_xlsx_list_images_true_missing_file_raises():
    with pytest.raises(FileNotFoundError):
        get_chunks("missing_that_does_not_exist.xlsx", list_images=True)


def test_xlsx_markdown_list_images_true_missing_file_raises():
    with pytest.raises(FileNotFoundError):
        get_markdown("missing_that_does_not_exist.xlsx", list_images=True)


# ---------------------------------------------------------------------------
# get_chunks tests
# ---------------------------------------------------------------------------


@_SKIP
def test_xlsx_list_images_false_returns_plain_list():
    result = get_chunks(str(XLSX_FIXTURE), mode="row")
    assert isinstance(result, list)
    assert not isinstance(result, ChunksResult)


@_SKIP
def test_xlsx_list_images_true_returns_chunks_result():
    result = get_chunks(str(XLSX_FIXTURE), list_images=True)
    assert isinstance(result, ChunksResult)
    assert isinstance(result.chunks, list)
    assert len(result.chunks) > 0
    assert isinstance(result.images, dict)
    assert len(result.images) >= 1


@_SKIP
def test_xlsx_image_key_format():
    result = get_chunks(str(XLSX_FIXTURE), list_images=True)
    for key, value in result.images.items():
        assert IMAGE_KEY_PATTERN.match(
            key), f"Key {key!r} does not match pattern"
        assert isinstance(value, bytes)
        assert len(value) > 0


@_SKIP
def test_xlsx_image_chunks_structure():
    result = get_chunks(str(XLSX_FIXTURE), list_images=True)
    image_chunks = [c for c in result.chunks if c["content_type"] == "image"]
    assert len(image_chunks) >= 1, "Expected at least one image chunk"
    for chunk in image_chunks:
        assert IMAGE_KEY_PATTERN.match(chunk["content"]), (
            f"Image chunk content {chunk['content']!r} does not match hash pattern"
        )
        meta = chunk["metadata"]
        assert "sheet_name" in meta, "Missing sheet_name in image chunk metadata"
        assert "sheet_index" in meta, "Missing sheet_index in image chunk metadata"
        assert "image_name" in meta, "Missing image_name in image chunk metadata"
        assert "alt_text" in meta, "Missing alt_text in image chunk metadata"
        assert meta["sheet_name"] != "", "sheet_name must not be empty"
        assert isinstance(meta["sheet_index"], int)


@_SKIP
def test_xlsx_image_chunk_content_matches_images_dict():
    result = get_chunks(str(XLSX_FIXTURE), list_images=True)
    image_chunks = [c for c in result.chunks if c["content_type"] == "image"]
    for chunk in image_chunks:
        assert chunk["content"] in result.images, (
            f"Image chunk content {chunk['content']!r} not found in images dict"
        )


@_SKIP
def test_xlsx_text_chunk_quality_unchanged():
    """Non-image chunks must be byte-for-byte identical between list_images=False and True."""
    plain = get_chunks(str(XLSX_FIXTURE), mode="row")
    with_images = get_chunks(str(XLSX_FIXTURE), mode="row", list_images=True)
    text_chunks = [
        c for c in with_images.chunks if c["content_type"] != "image"]
    assert plain == text_chunks, (
        f"Text chunks differ: plain={len(plain)}, with_images_text={len(text_chunks)}"
    )


@_SKIP
def test_xlsx_bytes_source():
    data = XLSX_FIXTURE.read_bytes()
    result = get_chunks(data, filename=XLSX_FIXTURE.name, list_images=True)
    assert isinstance(result, ChunksResult)
    assert len(result.images) >= 1
    path_result = get_chunks(str(XLSX_FIXTURE), list_images=True)
    assert set(result.images.keys()) == set(path_result.images.keys())


@_SKIP
def test_xlsx_filelike_source():
    with open(XLSX_FIXTURE, "rb") as f:
        result = get_chunks(f, list_images=True)
    assert isinstance(result, ChunksResult)
    assert len(result.images) >= 1
    path_result = get_chunks(str(XLSX_FIXTURE), list_images=True)
    assert set(result.images.keys()) == set(path_result.images.keys())


@_SKIP
@pytest.mark.parametrize("mode", _ALL_XLSX_MODES)
def test_xlsx_all_modes_return_chunks_result(mode):
    kwargs = _XLSX_MODE_KWARGS.get(mode, {})
    result = get_chunks(str(XLSX_FIXTURE), mode=mode,
                        list_images=True, **kwargs)
    assert isinstance(result, ChunksResult)
    assert isinstance(result.chunks, list)
    assert len(result.chunks) > 0
    assert isinstance(result.images, dict)


@_SKIP
@pytest.mark.parametrize("mode", _ALL_XLSX_MODES)
def test_xlsx_all_modes_have_images(mode):
    kwargs = _XLSX_MODE_KWARGS.get(mode, {})
    result = get_chunks(str(XLSX_FIXTURE), mode=mode,
                        list_images=True, **kwargs)
    assert len(result.images) >= 1, f"Expected images for mode={mode!r}"
    for key, val in result.images.items():
        assert IMAGE_KEY_PATTERN.match(
            key), f"Bad key {key!r} for mode={mode!r}"
        assert isinstance(val, bytes) and len(val) > 0


@_SKIP
@pytest.mark.parametrize("mode", _ALL_XLSX_MODES)
def test_xlsx_all_modes_text_chunk_quality_unchanged(mode):
    kwargs = _XLSX_MODE_KWARGS.get(mode, {})
    plain = get_chunks(str(XLSX_FIXTURE), mode=mode,
                       list_images=False, **kwargs)
    with_images = get_chunks(
        str(XLSX_FIXTURE), mode=mode, list_images=True, **kwargs)
    text_chunks = [
        c for c in with_images.chunks if c["content_type"] != "image"]
    assert plain == text_chunks, (
        f"Text chunks differ for mode={mode!r}: plain={len(plain)}, text={len(text_chunks)}"
    )


@_SKIP
@pytest.mark.parametrize("mode", _ALL_XLSX_MODES)
def test_xlsx_all_modes_image_chunks_reference_images_dict(mode):
    kwargs = _XLSX_MODE_KWARGS.get(mode, {})
    result = get_chunks(str(XLSX_FIXTURE), mode=mode,
                        list_images=True, **kwargs)
    image_chunks = [c for c in result.chunks if c["content_type"] == "image"]
    for chunk in image_chunks:
        assert IMAGE_KEY_PATTERN.match(chunk["content"]), (
            f"Bad image chunk content: {chunk['content']!r}"
        )
        assert chunk["content"] in result.images, (
            f"Image chunk {chunk['content']!r} not found in images dict for mode={mode!r}"
        )


# ---------------------------------------------------------------------------
# get_markdown tests
# ---------------------------------------------------------------------------


@_SKIP
def test_xlsx_markdown_list_images_false_returns_str():
    result = get_markdown(str(XLSX_FIXTURE))
    assert isinstance(result, str)
    result2 = get_markdown(str(XLSX_FIXTURE), list_images=False)
    assert isinstance(result2, str)


@_SKIP
def test_xlsx_markdown_list_images_true_returns_markdown_result():
    result = get_markdown(str(XLSX_FIXTURE), list_images=True)
    assert isinstance(result, MarkdownResult)
    assert isinstance(result.markdown, str)
    assert result.markdown.strip() != ""
    assert isinstance(result.images, dict)
    assert len(result.images) >= 1


@_SKIP
def test_xlsx_markdown_image_key_format():
    result = get_markdown(str(XLSX_FIXTURE), list_images=True)
    for key, value in result.images.items():
        assert IMAGE_KEY_PATTERN.match(
            key), f"Key {key!r} does not match pattern"
        assert isinstance(value, bytes)
        assert len(value) > 0


@_SKIP
def test_xlsx_markdown_images_referenced_in_markdown():
    """Every extracted image must appear as ![](hash.ext) in the markdown string."""
    result = get_markdown(str(XLSX_FIXTURE), list_images=True)
    for hash_name in result.images:
        assert f"![]({hash_name})" in result.markdown, (
            f"Image {hash_name!r} not referenced in markdown output"
        )


@_SKIP
def test_xlsx_markdown_bytes_source():
    data = XLSX_FIXTURE.read_bytes()
    result = get_markdown(data, filename=XLSX_FIXTURE.name, list_images=True)
    assert isinstance(result, MarkdownResult)
    assert len(result.images) >= 1


@_SKIP
def test_xlsx_markdown_filelike_source():
    with open(XLSX_FIXTURE, "rb") as f:
        result = get_markdown(f, list_images=True)
    assert isinstance(result, MarkdownResult)
    assert len(result.images) >= 1


# ---------------------------------------------------------------------------
# XLS graceful fallback - images not supported, must return empty dict silently
# ---------------------------------------------------------------------------


@_SKIP_XLS
def test_xls_list_images_true_returns_chunks_result_with_empty_images():
    result = get_chunks(str(XLS_FIXTURE), list_images=True)
    assert isinstance(result, ChunksResult)
    assert isinstance(result.chunks, list)
    assert result.images == {}, f"Expected empty images for .xls, got {result.images.keys()}"


@_SKIP_XLS
def test_xls_markdown_list_images_true_returns_empty_images():
    result = get_markdown(str(XLS_FIXTURE), list_images=True)
    assert isinstance(result, MarkdownResult)
    assert isinstance(result.markdown, str)
    assert result.images == {}, f"Expected empty images for .xls, got {result.images.keys()}"
