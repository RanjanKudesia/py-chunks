"""Tests for image-aware chunking and markdown for HTML via list_images=True."""

import re
from pathlib import Path

import pytest

from py_chunks import ChunksResult, MarkdownResult, get_chunks, get_markdown

HTML_FIXTURE = Path("../test_files/html/sample_with_image.html")
IMAGE_KEY_PATTERN = re.compile(r"^[0-9a-f]{16}\.(png|jpg|jpeg|gif|webp)$")

_FIXTURE_EXISTS = HTML_FIXTURE.exists()
_SKIP = pytest.mark.skipif(not _FIXTURE_EXISTS, reason=f"Missing fixture: {HTML_FIXTURE}")

_ALL_HTML_MODES = ["default", "structural", "semantic", "section",
                   "sliding_window", "sentence", "page_aware"]

_MODE_KWARGS = {
    "sliding_window": {"window_size": 3, "overlap": 1},
    "sentence": {"sentences_per_chunk": 3},
    "page_aware": {"paragraphs_per_page": 15},
}


# ---------------------------------------------------------------------------
# Always-run smoke tests (no fixture needed)
# ---------------------------------------------------------------------------

def test_html_list_images_true_missing_file_raises():
    with pytest.raises(FileNotFoundError):
        get_chunks("missing_that_does_not_exist.html", list_images=True)


def test_htm_list_images_true_missing_file_raises():
    with pytest.raises(FileNotFoundError):
        get_chunks("missing_that_does_not_exist.htm", list_images=True)


def test_html_markdown_list_images_true_missing_file_raises():
    with pytest.raises(FileNotFoundError):
        get_markdown("missing_that_does_not_exist.html", list_images=True)


# ---------------------------------------------------------------------------
# get_chunks tests
# ---------------------------------------------------------------------------

@_SKIP
def test_html_list_images_false_returns_plain_list():
    result = get_chunks(str(HTML_FIXTURE))
    assert isinstance(result, list)
    assert not isinstance(result, ChunksResult)


@_SKIP
def test_html_list_images_true_returns_chunks_result():
    result = get_chunks(str(HTML_FIXTURE), list_images=True)
    assert isinstance(result, ChunksResult)
    assert isinstance(result.chunks, list)
    assert len(result.chunks) > 0
    assert isinstance(result.images, dict)
    assert len(result.images) >= 1


@_SKIP
def test_html_image_key_format():
    result = get_chunks(str(HTML_FIXTURE), list_images=True)
    for key, value in result.images.items():
        assert IMAGE_KEY_PATTERN.match(key), f"Key {key!r} does not match pattern"
        assert isinstance(value, bytes)
        assert len(value) > 0


@_SKIP
def test_html_image_chunks_structure():
    result = get_chunks(str(HTML_FIXTURE), list_images=True)
    image_chunks = [c for c in result.chunks if c["content_type"] == "image"]
    assert len(image_chunks) >= 1, "Expected at least one image chunk"
    for chunk in image_chunks:
        assert IMAGE_KEY_PATTERN.match(chunk["content"]), (
            f"Image chunk content {chunk['content']!r} does not match hash pattern"
        )
        meta = chunk["metadata"]
        assert "image_name" in meta
        assert "alt_text" in meta
        assert meta["image_name"] == chunk["content"]


@_SKIP
def test_html_image_chunk_content_matches_images_dict():
    result = get_chunks(str(HTML_FIXTURE), list_images=True)
    image_chunks = [c for c in result.chunks if c["content_type"] == "image"]
    for chunk in image_chunks:
        assert chunk["content"] in result.images, (
            f"Image chunk content {chunk['content']!r} not found in images dict"
        )


@_SKIP
def test_html_text_chunk_quality_unchanged():
    """Non-image chunks must be byte-for-byte identical between list_images=False and True."""
    plain = get_chunks(str(HTML_FIXTURE))
    with_images = get_chunks(str(HTML_FIXTURE), list_images=True)
    text_chunks = [c for c in with_images.chunks if c["content_type"] != "image"]
    assert plain == text_chunks, (
        f"Text chunks differ: plain={len(plain)}, with_images_text={len(text_chunks)}"
    )


@_SKIP
def test_html_bytes_source():
    data = HTML_FIXTURE.read_bytes()
    result = get_chunks(data, filename=HTML_FIXTURE.name, list_images=True)
    assert isinstance(result, ChunksResult)
    assert len(result.images) >= 1
    path_result = get_chunks(str(HTML_FIXTURE), list_images=True)
    assert set(result.images.keys()) == set(path_result.images.keys())


@_SKIP
def test_html_filelike_source():
    with open(HTML_FIXTURE, "rb") as f:
        result = get_chunks(f, list_images=True)
    assert isinstance(result, ChunksResult)
    assert len(result.images) >= 1
    path_result = get_chunks(str(HTML_FIXTURE), list_images=True)
    assert set(result.images.keys()) == set(path_result.images.keys())


@_SKIP
@pytest.mark.parametrize("mode", _ALL_HTML_MODES)
def test_html_all_modes_return_chunks_result(mode):
    kwargs = _MODE_KWARGS.get(mode, {})
    result = get_chunks(str(HTML_FIXTURE), mode=mode, list_images=True, **kwargs)
    assert isinstance(result, ChunksResult)
    assert isinstance(result.chunks, list)
    assert len(result.chunks) > 0
    assert isinstance(result.images, dict)


@_SKIP
@pytest.mark.parametrize("mode", _ALL_HTML_MODES)
def test_html_all_modes_have_images(mode):
    kwargs = _MODE_KWARGS.get(mode, {})
    result = get_chunks(str(HTML_FIXTURE), mode=mode, list_images=True, **kwargs)
    assert len(result.images) >= 1, f"Expected images for mode={mode!r}"
    for key, val in result.images.items():
        assert IMAGE_KEY_PATTERN.match(key), f"Bad key {key!r} for mode={mode!r}"
        assert isinstance(val, bytes) and len(val) > 0


@_SKIP
@pytest.mark.parametrize("mode", _ALL_HTML_MODES)
def test_html_all_modes_text_chunk_quality_unchanged(mode):
    """Non-image chunks must be byte-for-byte identical between list_images=False and True."""
    kwargs = _MODE_KWARGS.get(mode, {})
    plain = get_chunks(str(HTML_FIXTURE), mode=mode, list_images=False, **kwargs)
    with_images = get_chunks(str(HTML_FIXTURE), mode=mode, list_images=True, **kwargs)
    text_chunks = [c for c in with_images.chunks if c["content_type"] != "image"]
    assert plain == text_chunks, (
        f"Text chunks differ for mode={mode!r}: plain={len(plain)}, text={len(text_chunks)}"
    )


@_SKIP
@pytest.mark.parametrize("mode", _ALL_HTML_MODES)
def test_html_all_modes_image_chunks_reference_images_dict(mode):
    kwargs = _MODE_KWARGS.get(mode, {})
    result = get_chunks(str(HTML_FIXTURE), mode=mode, list_images=True, **kwargs)
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
def test_html_markdown_list_images_false_returns_str():
    result = get_markdown(str(HTML_FIXTURE))
    assert isinstance(result, str)
    result2 = get_markdown(str(HTML_FIXTURE), list_images=False)
    assert isinstance(result2, str)


@_SKIP
def test_html_markdown_list_images_true_returns_markdown_result():
    result = get_markdown(str(HTML_FIXTURE), list_images=True)
    assert isinstance(result, MarkdownResult)
    assert isinstance(result.markdown, str)
    assert result.markdown.strip() != ""
    assert isinstance(result.images, dict)
    assert len(result.images) >= 1


@_SKIP
def test_html_markdown_image_key_format():
    result = get_markdown(str(HTML_FIXTURE), list_images=True)
    for key, value in result.images.items():
        assert IMAGE_KEY_PATTERN.match(key), f"Key {key!r} does not match pattern"
        assert isinstance(value, bytes)
        assert len(value) > 0


@_SKIP
def test_html_markdown_images_referenced_in_markdown():
    """Every extracted image must appear as ![...](hash.ext) in the markdown string."""
    result = get_markdown(str(HTML_FIXTURE), list_images=True)
    for hash_name in result.images:
        assert f"({hash_name})" in result.markdown, (
            f"Image {hash_name!r} not referenced in markdown output"
        )


@_SKIP
def test_html_markdown_bytes_source():
    data = HTML_FIXTURE.read_bytes()
    result = get_markdown(data, filename=HTML_FIXTURE.name, list_images=True)
    assert isinstance(result, MarkdownResult)
    assert len(result.images) >= 1


@_SKIP
def test_html_markdown_filelike_source():
    with open(HTML_FIXTURE, "rb") as f:
        result = get_markdown(f, list_images=True)
    assert isinstance(result, MarkdownResult)
    assert len(result.images) >= 1


@_SKIP
def test_html_markdown_text_parity():
    """Markdown text (minus appended image lines) must equal the non-image markdown."""
    plain_md = get_markdown(str(HTML_FIXTURE))
    with_images = get_markdown(str(HTML_FIXTURE), list_images=True)

    # Strip appended image lines from the with_images markdown to recover the text portion.
    img_lines = {f"![]({h})" for h in with_images.images}
    lines = [
        line for line in with_images.markdown.splitlines()
        if not any(img_ref in line for img_ref in img_lines)
    ]
    recovered = "\n".join(lines).strip()
    assert recovered == plain_md.strip(), (
        "Markdown text portion differs between list_images=False and True"
    )


# ---------------------------------------------------------------------------
# alt_text test
# ---------------------------------------------------------------------------

@_SKIP
def test_html_alt_text_captured():
    """Images with alt attributes must have their alt_text preserved in chunk metadata."""
    result = get_chunks(str(HTML_FIXTURE), list_images=True)
    image_chunks = [c for c in result.chunks if c["content_type"] == "image"]
    alts = [c["metadata"].get("alt_text") for c in image_chunks]
    # The fixture has at least one image with alt="Test image"
    assert any(a and len(a) > 0 for a in alts), "Expected at least one non-empty alt_text"
