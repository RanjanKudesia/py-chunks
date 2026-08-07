"""Tests for image-aware chunking via get_chunks(..., list_images=True)."""

import re
from pathlib import Path

import pytest

from py_chunks import ChunksResult, get_chunks

_TEST_FILES = Path(__file__).resolve().parents[2] / "test_files"
DOCX = _TEST_FILES / "docx" / "all_round.docx"
PPTX = _TEST_FILES / "pptx" / "sample.pptx"
IMAGE_KEY_PATTERN = re.compile(r"^[0-9a-f]{16}\.(png|jpg|jpeg|gif|webp)$")

_FIXTURE_EXISTS = DOCX.exists()
_PPTX_FIXTURE_EXISTS = PPTX.exists()
_SKIP_NO_PPTX = pytest.mark.skipif(
    not _PPTX_FIXTURE_EXISTS, reason=f"Missing test fixture: {PPTX}"
)

_ALL_PPTX_MODES = [
    "default",
    "structural",
    "section",
    "semantic",
    "sliding_window",
    "sentence",
    "page_aware",
]

_PPTX_MODE_KWARGS = {
    "sliding_window": {"window_size": 3, "overlap": 1},
    "sentence": {"sentences_per_chunk": 3},
    "page_aware": {"paragraphs_per_page": 5},
}

# ---------------------------------------------------------------------------
# Always-run tests (no fixture needed)
# ---------------------------------------------------------------------------


def test_list_images_false_missing_file_raises():
    with pytest.raises(FileNotFoundError):
        get_chunks("missing_file_that_does_not_exist.docx")


def test_list_images_true_missing_file_raises():
    with pytest.raises(FileNotFoundError):
        get_chunks("missing_file_that_does_not_exist.docx", list_images=True)


def test_bytes_without_filename_raises():
    with pytest.raises(ValueError):
        get_chunks(b"some data", list_images=True)


def test_non_docx_list_images_true_returns_chunks_result(tmp_path):
    """For unsupported formats, list_images=True should return ChunksResult with images={}."""
    fake_pdf = tmp_path / "test.pdf"
    fake_pdf.write_bytes(b"%PDF-1.4 fake content")
    try:
        result = get_chunks(str(fake_pdf), list_images=True)
        assert isinstance(result, ChunksResult)
        assert result.images == {}
    except (ValueError, RuntimeError, OSError):
        # Bad file content can raise; that's acceptable — it must NOT raise TypeError
        pass


# ---------------------------------------------------------------------------
# Fixture-dependent tests
# ---------------------------------------------------------------------------

_SKIP_NO_FIXTURE = pytest.mark.skipif(
    not _FIXTURE_EXISTS, reason="Missing test fixture: ../test_files/docx/all_round.docx"
)


@_SKIP_NO_FIXTURE
def test_list_images_false_returns_plain_list():
    result = get_chunks(str(DOCX))
    assert isinstance(result, list)
    assert not isinstance(result, ChunksResult)

    result2 = get_chunks(str(DOCX), list_images=False)
    assert isinstance(result2, list)
    assert not isinstance(result2, ChunksResult)


@_SKIP_NO_FIXTURE
def test_list_images_true_returns_chunks_result():
    result = get_chunks(str(DOCX), list_images=True)
    assert isinstance(result, ChunksResult)
    assert isinstance(result.chunks, list)
    assert len(result.chunks) > 0
    assert isinstance(result.images, dict)
    assert len(result.images) > 0


@_SKIP_NO_FIXTURE
def test_image_key_format():
    result = get_chunks(str(DOCX), list_images=True)
    for key, value in result.images.items():
        assert IMAGE_KEY_PATTERN.match(key), f"Key {key!r} does not match expected pattern"
        assert isinstance(value, bytes)
        assert len(value) > 0, f"Image bytes for {key!r} are empty"


@_SKIP_NO_FIXTURE
def test_image_chunks_structure():
    result = get_chunks(str(DOCX), list_images=True)
    image_chunks = [c for c in result.chunks if c["content_type"] == "image"]
    assert len(image_chunks) > 0, "Expected at least one image chunk"
    for chunk in image_chunks:
        assert IMAGE_KEY_PATTERN.match(chunk["content"]), (
            f"Image chunk content {chunk['content']!r} does not match hash pattern"
        )
        assert "image_name" in chunk["metadata"]
        assert "alt_text" in chunk["metadata"]


@_SKIP_NO_FIXTURE
def test_image_chunk_content_matches_images_dict():
    result = get_chunks(str(DOCX), list_images=True)
    image_chunks = [c for c in result.chunks if c["content_type"] == "image"]
    for chunk in image_chunks:
        assert chunk["content"] in result.images, (
            f"Image chunk content {chunk['content']!r} not found in images dict"
        )
    unique_image_contents = {c["content"] for c in image_chunks}
    assert len(unique_image_contents) == len(result.images), (
        f"Expected {len(result.images)} unique image hashes, got {len(unique_image_contents)}"
    )


@_SKIP_NO_FIXTURE
def test_no_placeholder_text_in_chunks():
    result = get_chunks(str(DOCX), list_images=True)
    # Image-typed chunks must have real hash names, not "[Image...]" placeholders.
    # Mixed-content/section chunks may still contain image alt text (when images
    # appear inside heading sections), which is acceptable.
    image_chunks = [c for c in result.chunks if c["content_type"] == "image"]
    for chunk in image_chunks:
        assert "[Image" not in chunk["content"], (
            f"Image chunk still contains placeholder text: {chunk['content']!r}"
        )


@_SKIP_NO_FIXTURE
def test_text_chunk_quality_unchanged():
    plain = get_chunks(str(DOCX), list_images=False)
    with_images = get_chunks(str(DOCX), list_images=True)

    plain_text_chunks = [c for c in plain if c["content_type"] != "image"]
    image_stripped_chunks = [c for c in with_images.chunks if c["content_type"] != "image"]

    assert plain_text_chunks == image_stripped_chunks, (
        "Text chunks differ between list_images=False and list_images=True"
    )


@_SKIP_NO_FIXTURE
def test_bytes_source():
    data = DOCX.read_bytes()
    result = get_chunks(data, filename=DOCX.name, list_images=True)
    assert isinstance(result, ChunksResult)
    assert len(result.images) > 0

    path_result = get_chunks(str(DOCX), list_images=True)
    assert set(result.images.keys()) == set(path_result.images.keys())


@_SKIP_NO_FIXTURE
def test_filelike_source():
    with open(DOCX, "rb") as f:
        result = get_chunks(f, list_images=True)
    assert isinstance(result, ChunksResult)
    assert len(result.images) > 0

    path_result = get_chunks(str(DOCX), list_images=True)
    assert set(result.images.keys()) == set(path_result.images.keys())


# ---------------------------------------------------------------------------
# Multi-mode tests (all 7 DOCX modes must work with list_images=True)
# ---------------------------------------------------------------------------

_ALL_DOCX_MODES = [
    "default",
    "structural",
    "section",
    "semantic",
    "sliding_window",
    "sentence",
    "page_aware",
]

_MODE_KWARGS = {
    "sliding_window": {"window_size": 3, "overlap": 1},
    "sentence": {"sentences_per_chunk": 3},
    "page_aware": {"paragraphs_per_page": 10},
}


@pytest.mark.skipif(
    not _FIXTURE_EXISTS, reason="Missing test fixture: ../test_files/docx/all_round.docx"
)
@pytest.mark.parametrize("mode", _ALL_DOCX_MODES)
def test_all_modes_return_chunks_result(mode):
    kwargs = _MODE_KWARGS.get(mode, {})
    result = get_chunks(str(DOCX), mode=mode, list_images=True, **kwargs)
    assert isinstance(result, ChunksResult)
    assert isinstance(result.chunks, list)
    assert len(result.chunks) > 0
    assert isinstance(result.images, dict)


@pytest.mark.skipif(
    not _FIXTURE_EXISTS, reason="Missing test fixture: ../test_files/docx/all_round.docx"
)
@pytest.mark.parametrize("mode", _ALL_DOCX_MODES)
def test_all_modes_have_images(mode):
    kwargs = _MODE_KWARGS.get(mode, {})
    result = get_chunks(str(DOCX), mode=mode, list_images=True, **kwargs)
    assert len(result.images) > 0, f"Expected images for mode={mode!r}"
    for key, val in result.images.items():
        assert IMAGE_KEY_PATTERN.match(key), f"Bad key {key!r} for mode={mode!r}"
        assert isinstance(val, bytes) and len(val) > 0


@pytest.mark.skipif(
    not _FIXTURE_EXISTS, reason="Missing test fixture: ../test_files/docx/all_round.docx"
)
@pytest.mark.parametrize("mode", _ALL_DOCX_MODES)
def test_all_modes_text_chunk_quality_unchanged(mode):
    """Non-image chunks must be byte-for-byte identical between list_images=False and True."""
    kwargs = _MODE_KWARGS.get(mode, {})
    plain = get_chunks(str(DOCX), mode=mode, list_images=False, **kwargs)
    with_images = get_chunks(str(DOCX), mode=mode, list_images=True, **kwargs)

    plain_text = [c for c in plain if c["content_type"] != "image"]
    with_images_text = [c for c in with_images.chunks if c["content_type"] != "image"]

    assert plain_text == with_images_text, (
        f"Text chunks differ for mode={mode!r}:\n"
        f"  plain has {len(plain_text)} chunks\n"
        f"  with_images has {len(with_images_text)} chunks"
    )


@pytest.mark.skipif(
    not _FIXTURE_EXISTS, reason="Missing test fixture: ../test_files/docx/all_round.docx"
)
@pytest.mark.parametrize("mode", _ALL_DOCX_MODES)
def test_all_modes_image_chunks_reference_images_dict(mode):
    kwargs = _MODE_KWARGS.get(mode, {})
    result = get_chunks(str(DOCX), mode=mode, list_images=True, **kwargs)
    image_chunks = [c for c in result.chunks if c["content_type"] == "image"]
    for chunk in image_chunks:
        assert IMAGE_KEY_PATTERN.match(
            chunk["content"]
        ), f"Bad image chunk content: {chunk['content']!r}"
        assert chunk["content"] in result.images, (
            f"Image chunk {chunk['content']!r} not found in images dict for mode={mode!r}"
        )


# ---------------------------------------------------------------------------
# PPTX image tests
# ---------------------------------------------------------------------------


@_SKIP_NO_PPTX
def test_pptx_list_images_false_returns_plain_list():
    result = get_chunks(str(PPTX))
    assert isinstance(result, list)
    assert not isinstance(result, ChunksResult)


@_SKIP_NO_PPTX
def test_pptx_list_images_true_returns_chunks_result():
    result = get_chunks(str(PPTX), list_images=True)
    assert isinstance(result, ChunksResult)
    assert isinstance(result.chunks, list)
    assert len(result.chunks) > 0
    assert isinstance(result.images, dict)
    assert len(result.images) >= 1


@_SKIP_NO_PPTX
def test_pptx_image_key_format():
    result = get_chunks(str(PPTX), list_images=True)
    for key, value in result.images.items():
        assert IMAGE_KEY_PATTERN.match(key), f"Key {key!r} does not match expected pattern"
        assert isinstance(value, bytes)
        assert len(value) > 0


@_SKIP_NO_PPTX
def test_pptx_image_chunks_structure():
    result = get_chunks(str(PPTX), list_images=True)
    image_chunks = [c for c in result.chunks if c["content_type"] == "image"]
    assert len(image_chunks) > 0
    for chunk in image_chunks:
        assert IMAGE_KEY_PATTERN.match(chunk["content"]), (
            f"Image chunk content {chunk['content']!r} does not match hash pattern"
        )
        assert "slide_number" in chunk["metadata"]
        assert "image_name" in chunk["metadata"]
        assert "alt_text" in chunk["metadata"]


@_SKIP_NO_PPTX
def test_pptx_image_chunk_content_matches_images_dict():
    result = get_chunks(str(PPTX), list_images=True)
    image_chunks = [c for c in result.chunks if c["content_type"] == "image"]
    for chunk in image_chunks:
        assert chunk["content"] in result.images


@_SKIP_NO_PPTX
def test_pptx_text_chunk_quality_unchanged():
    plain = get_chunks(str(PPTX), list_images=False)
    with_images = get_chunks(str(PPTX), list_images=True)
    plain_text = [c for c in plain if c["content_type"] != "image"]
    with_images_text = [c for c in with_images.chunks if c["content_type"] != "image"]
    assert plain_text == with_images_text


@pytest.mark.skipif(
    not _PPTX_FIXTURE_EXISTS, reason="Missing test fixture: ../test_files/pptx/sample.pptx"
)
@pytest.mark.parametrize("mode", _ALL_PPTX_MODES)
def test_pptx_all_modes_return_chunks_result(mode):
    kwargs = _PPTX_MODE_KWARGS.get(mode, {})
    result = get_chunks(str(PPTX), mode=mode, list_images=True, **kwargs)
    assert isinstance(result, ChunksResult)
    assert isinstance(result.chunks, list)
    assert len(result.chunks) > 0
    assert isinstance(result.images, dict)


@pytest.mark.skipif(
    not _PPTX_FIXTURE_EXISTS, reason="Missing test fixture: ../test_files/pptx/sample.pptx"
)
@pytest.mark.parametrize("mode", _ALL_PPTX_MODES)
def test_pptx_all_modes_have_images(mode):
    kwargs = _PPTX_MODE_KWARGS.get(mode, {})
    result = get_chunks(str(PPTX), mode=mode, list_images=True, **kwargs)
    assert len(result.images) >= 1, f"Expected images for mode={mode!r}"
    for key, val in result.images.items():
        assert IMAGE_KEY_PATTERN.match(key), f"Bad key {key!r} for mode={mode!r}"
        assert isinstance(val, bytes) and len(val) > 0


@pytest.mark.skipif(
    not _PPTX_FIXTURE_EXISTS, reason="Missing test fixture: ../test_files/pptx/sample.pptx"
)
@pytest.mark.parametrize("mode", _ALL_PPTX_MODES)
def test_pptx_all_modes_text_chunk_quality_unchanged(mode):
    """Non-image chunks must be byte-for-byte identical between list_images=False and True."""
    kwargs = _PPTX_MODE_KWARGS.get(mode, {})
    plain = get_chunks(str(PPTX), mode=mode, list_images=False, **kwargs)
    with_images = get_chunks(str(PPTX), mode=mode, list_images=True, **kwargs)
    plain_text = [c for c in plain if c["content_type"] != "image"]
    with_images_text = [c for c in with_images.chunks if c["content_type"] != "image"]
    assert plain_text == with_images_text, (
        f"Text chunks differ for mode={mode!r}:\n"
        f"  plain has {len(plain_text)} chunks\n"
        f"  with_images has {len(with_images_text)} chunks"
    )


@pytest.mark.skipif(
    not _PPTX_FIXTURE_EXISTS, reason="Missing test fixture: ../test_files/pptx/sample.pptx"
)
@pytest.mark.parametrize("mode", _ALL_PPTX_MODES)
def test_pptx_all_modes_image_chunks_reference_images_dict(mode):
    kwargs = _PPTX_MODE_KWARGS.get(mode, {})
    result = get_chunks(str(PPTX), mode=mode, list_images=True, **kwargs)
    image_chunks = [c for c in result.chunks if c["content_type"] == "image"]
    for chunk in image_chunks:
        assert IMAGE_KEY_PATTERN.match(chunk["content"]), (
            f"Bad image chunk content: {chunk['content']!r}"
        )
        assert chunk["content"] in result.images, (
            f"Image chunk {chunk['content']!r} not found in images dict for mode={mode!r}"
        )
