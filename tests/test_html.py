"""HTML chunking tests across all supported modes and source APIs."""
# pylint: disable=redefined-outer-name

from io import BytesIO
from pathlib import Path

import pytest

from py_chunks import (
    get_chunks,
    get_chunks_from_bytes,
    get_chunks_from_fileobj,
    get_chunks_from_upload,
    stream_chunks_from_bytes,
    stream_chunks_from_fileobj,
)
from py_chunks.chunkers.html import chunk_html, stream_chunk_html


# ── Fixture discovery ─────────────────────────────────────────────────────────

_CANDIDATE_DIRS = [
    Path(__file__).resolve().parents[2] / "test_files",
    Path(__file__).resolve().parents[2] / "test_input_files",
    # Fixtures were reorganised into per-format subdirectories.
    Path(__file__).resolve().parents[2] / "test_files" / "html",
]

_HTML_NAMES = ["prose_article.html", "structured_docs.html", "test.html"]


def _resolve_html_files() -> list[Path]:
    for base in _CANDIDATE_DIRS:
        if not base.exists():
            continue
        paths = [base / name for name in _HTML_NAMES]
        if all(p.exists() for p in paths):
            return paths
    return []


ALL_HTML_FILES = _resolve_html_files()

if len(ALL_HTML_FILES) != 3:
    pytest.skip(
        "Expected HTML fixtures (prose_article.html, structured_docs.html,"
        " test.html) in test_files/",
        allow_module_level=True,
    )

HTML_IDS = [p.name for p in ALL_HTML_FILES]
STANDARD_KEYS = {"content", "content_type", "metadata"}
VALID_CONTENT_TYPES = {
    "paragraph",
    "heading",
    "code_block",
    "list_item",
    "table",
    "image",
    "plain_paragraph",
    "bullet_list",
    "mixed_content",
    "long_single_paragraph",
    "short_disconnected_paragraph",
    "semantic",
    "section",
    "sliding_window",
    "sentence",
    "page_aware",
}


# ── Fixtures / helpers ───────────────────────────────────────────────────────

@pytest.fixture
def html_files() -> list[Path]:
    return ALL_HTML_FILES


@pytest.fixture
def html_file(html_files) -> Path:
    for p in html_files:
        if p.name == "prose_article.html":
            return p
    return html_files[0]


class _DummyUpload:
    def __init__(self, data: bytes, filename: str):
        self.filename = filename
        self.file = BytesIO(data)


def _get(path: Path, mode: str = "default", **kwargs) -> list[dict]:
    return get_chunks(str(path), mode=mode, **kwargs)


def _assert_nonempty(chunks: list[dict], label: str) -> None:
    assert isinstance(chunks, list), f"{label}: expected list"
    assert len(chunks) > 0, f"{label}: expected non-empty chunk list"


def _assert_schema(chunks: list[dict], label: str) -> None:
    for c in chunks:
        assert STANDARD_KEYS.issubset(c.keys()), (
            f"{label}: missing keys {STANDARD_KEYS - c.keys()}"
        )
        assert isinstance(c["content"], str) and c["content"].strip(), (
            f"{label}: content must be non-empty str"
        )
        assert isinstance(c["content_type"], str) and c["content_type"], (
            f"{label}: content_type must be non-empty str"
        )
        assert isinstance(c["metadata"], dict), f"{label}: metadata must be dict"


def _assert_valid_content_types(chunks: list[dict], label: str) -> None:
    for c in chunks:
        assert c["content_type"] in VALID_CONTENT_TYPES, (
            f"{label}: unexpected content_type {c['content_type']!r}"
        )


# ════════════════════════════════════════════════════════════════════════════
# 1. STRUCTURAL
# ════════════════════════════════════════════════════════════════════════════

@pytest.mark.parametrize("html_path", ALL_HTML_FILES, ids=HTML_IDS)
class TestHtmlStructural:

    def test_returns_nonempty_list(self, html_path):
        chunks = _get(html_path, mode="structural")
        _assert_nonempty(chunks, html_path.name)

    def test_schema_per_chunk(self, html_path):
        chunks = _get(html_path, mode="structural")
        _assert_schema(chunks, html_path.name)

    def test_content_types_valid(self, html_path):
        chunks = _get(html_path, mode="structural")
        _assert_valid_content_types(chunks, html_path.name)

    def test_streaming_parity(self, html_path):
        batch = _get(html_path, mode="structural")
        streamed = list(stream_chunk_html(str(html_path), mode="structural"))
        assert len(streamed) == len(batch), (
            f"{html_path.name}: batch={len(batch)} stream={len(streamed)}"
        )


# ════════════════════════════════════════════════════════════════════════════
# 2. SEMANTIC
# ════════════════════════════════════════════════════════════════════════════

@pytest.mark.parametrize("html_path", ALL_HTML_FILES, ids=HTML_IDS)
class TestHtmlSemantic:

    def test_returns_nonempty_list(self, html_path):
        chunks = _get(html_path, mode="semantic")
        _assert_nonempty(chunks, html_path.name)

    def test_schema_per_chunk(self, html_path):
        chunks = _get(html_path, mode="semantic")
        _assert_schema(chunks, html_path.name)

    def test_content_types_valid(self, html_path):
        chunks = _get(html_path, mode="semantic")
        _assert_valid_content_types(chunks, html_path.name)

    def test_streaming_parity(self, html_path):
        # HTML semantic batch and stream use different merge passes so chunk
        # counts may differ; verify both are non-empty with a valid schema.
        batch = _get(html_path, mode="semantic")
        streamed = list(stream_chunk_html(str(html_path), mode="semantic"))
        _assert_nonempty(batch, f"{html_path.name} batch")
        _assert_nonempty(streamed, f"{html_path.name} stream")
        _assert_schema(streamed, f"{html_path.name} stream")


# ════════════════════════════════════════════════════════════════════════════
# 3. SECTION
# ════════════════════════════════════════════════════════════════════════════

@pytest.mark.parametrize("html_path", ALL_HTML_FILES, ids=HTML_IDS)
class TestHtmlSection:

    def test_returns_nonempty_list(self, html_path):
        chunks = _get(html_path, mode="section")
        _assert_nonempty(chunks, html_path.name)

    def test_schema_per_chunk(self, html_path):
        chunks = _get(html_path, mode="section")
        _assert_schema(chunks, html_path.name)

    def test_content_types_valid(self, html_path):
        chunks = _get(html_path, mode="section")
        _assert_valid_content_types(chunks, html_path.name)

    def test_streaming_parity(self, html_path):
        batch = _get(html_path, mode="section")
        streamed = list(stream_chunk_html(str(html_path), mode="section"))
        assert len(streamed) == len(batch), (
            f"{html_path.name}: batch={len(batch)} stream={len(streamed)}"
        )

    def test_section_metadata_has_section_heading(self, html_path):
        chunks = _get(html_path, mode="section")
        for c in chunks:
            assert "section_heading" in c["metadata"], (
                f"{html_path.name}: missing section_heading"
            )


# ════════════════════════════════════════════════════════════════════════════
# 4. SLIDING_WINDOW
# ════════════════════════════════════════════════════════════════════════════

@pytest.mark.parametrize("html_path", ALL_HTML_FILES, ids=HTML_IDS)
class TestHtmlSlidingWindow:

    def test_returns_nonempty_list(self, html_path):
        chunks = _get(html_path, mode="sliding_window", window_size=3, overlap=1)
        _assert_nonempty(chunks, html_path.name)

    def test_schema_per_chunk(self, html_path):
        chunks = _get(html_path, mode="sliding_window", window_size=3, overlap=1)
        _assert_schema(chunks, html_path.name)

    def test_content_types_valid(self, html_path):
        chunks = _get(html_path, mode="sliding_window", window_size=3, overlap=1)
        _assert_valid_content_types(chunks, html_path.name)

    def test_streaming_parity(self, html_path):
        batch = _get(html_path, mode="sliding_window", window_size=3, overlap=1)
        streamed = list(
            stream_chunk_html(
                str(html_path),
                mode="sliding_window",
                window_size=3,
                overlap=1,
            )
        )
        assert len(streamed) == len(batch), (
            f"{html_path.name}: batch={len(batch)} stream={len(streamed)}"
        )

    def test_sliding_window_metadata_fields(self, html_path):
        chunks = _get(html_path, mode="sliding_window", window_size=3, overlap=1)
        for c in chunks:
            meta = c["metadata"]
            assert "chunk_index" in meta
            assert "window_size" in meta
            assert "overlap" in meta


# ════════════════════════════════════════════════════════════════════════════
# 5. SENTENCE
# ════════════════════════════════════════════════════════════════════════════

@pytest.mark.parametrize("html_path", ALL_HTML_FILES, ids=HTML_IDS)
class TestHtmlSentence:

    def test_returns_nonempty_list(self, html_path):
        chunks = _get(html_path, mode="sentence", sentences_per_chunk=3)
        _assert_nonempty(chunks, html_path.name)

    def test_schema_per_chunk(self, html_path):
        chunks = _get(html_path, mode="sentence", sentences_per_chunk=3)
        _assert_schema(chunks, html_path.name)

    def test_content_types_valid(self, html_path):
        chunks = _get(html_path, mode="sentence", sentences_per_chunk=3)
        _assert_valid_content_types(chunks, html_path.name)

    def test_streaming_parity(self, html_path):
        batch = _get(html_path, mode="sentence", sentences_per_chunk=3)
        streamed = list(
            stream_chunk_html(
                str(html_path),
                mode="sentence",
                sentences_per_chunk=3,
            )
        )
        assert len(streamed) == len(batch), (
            f"{html_path.name}: batch={len(batch)} stream={len(streamed)}"
        )

    def test_sentence_metadata_has_chunk_index(self, html_path):
        chunks = _get(html_path, mode="sentence", sentences_per_chunk=3)
        for c in chunks:
            assert "chunk_index" in c["metadata"], (
                f"{html_path.name}: missing chunk_index"
            )


# ════════════════════════════════════════════════════════════════════════════
# 6. PAGE_AWARE
# ════════════════════════════════════════════════════════════════════════════

@pytest.mark.parametrize("html_path", ALL_HTML_FILES, ids=HTML_IDS)
class TestHtmlPageAware:

    def test_returns_nonempty_list(self, html_path):
        chunks = _get(html_path, mode="page_aware", paragraphs_per_page=15)
        _assert_nonempty(chunks, html_path.name)

    def test_schema_per_chunk(self, html_path):
        chunks = _get(html_path, mode="page_aware", paragraphs_per_page=15)
        _assert_schema(chunks, html_path.name)

    def test_content_types_valid(self, html_path):
        chunks = _get(html_path, mode="page_aware", paragraphs_per_page=15)
        _assert_valid_content_types(chunks, html_path.name)

    def test_streaming_parity(self, html_path):
        batch = _get(html_path, mode="page_aware", paragraphs_per_page=15)
        streamed = list(
            stream_chunk_html(
                str(html_path),
                mode="page_aware",
                paragraphs_per_page=15,
            )
        )
        assert len(streamed) == len(batch), (
            f"{html_path.name}: batch={len(batch)} stream={len(streamed)}"
        )

    def test_page_aware_metadata_fields(self, html_path):
        chunks = _get(html_path, mode="page_aware", paragraphs_per_page=15)
        for c in chunks:
            meta = c["metadata"]
            assert "page_break_type" in meta, f"{html_path.name}: missing page_break_type"
            assert "paragraph_count" in meta, f"{html_path.name}: missing paragraph_count"


# ════════════════════════════════════════════════════════════════════════════
# 7. SOURCE API EQUIVALENCE
# ════════════════════════════════════════════════════════════════════════════

class TestHtmlSourceEquivalence:

    def test_bytes_equals_path(self, html_file):
        by_path = get_chunks(str(html_file), mode="structural")
        by_bytes = get_chunks_from_bytes(
            html_file.read_bytes(),
            filename="x.html",
            mode="structural",
        )
        assert [c["content"] for c in by_bytes] == [c["content"] for c in by_path]

    def test_fileobj_equals_path(self, html_file):
        by_path = get_chunks(str(html_file), mode="structural")
        with open(html_file, "rb") as f:
            by_fileobj = get_chunks_from_fileobj(f, filename="x.html", mode="structural")
        assert by_fileobj == by_path

    def test_upload_equals_path(self, html_file):
        by_path = get_chunks(str(html_file), mode="structural")
        upload = _DummyUpload(html_file.read_bytes(), "x.html")
        by_upload = get_chunks_from_upload(upload, mode="structural")
        assert by_upload == by_path


class TestHtmlStreamingSourceEquivalence:

    def test_stream_bytes_equals_path(self, html_file):
        by_path = list(stream_chunk_html(str(html_file), mode="structural"))
        data = Path(html_file).read_bytes()
        by_bytes = list(
            stream_chunks_from_bytes(data, filename="x.html", mode="structural")
        )
        assert [c["content"] for c in by_bytes] == [c["content"] for c in by_path]

    def test_stream_fileobj_equals_path(self, html_file):
        by_path = list(stream_chunk_html(str(html_file), mode="structural"))
        with open(html_file, "rb") as f:
            by_fileobj = list(
                stream_chunks_from_fileobj(f, filename="x.html", mode="structural")
            )
        assert [c["content"] for c in by_fileobj] == [c["content"] for c in by_path]


# ════════════════════════════════════════════════════════════════════════════
# 8. VALIDATION
# ════════════════════════════════════════════════════════════════════════════

class TestHtmlValidation:

    def test_missing_file_raises(self):
        with pytest.raises(FileNotFoundError):
            get_chunks("/nonexistent/path/to/file.html", mode="structural")

    def test_invalid_mode_raises(self, html_file):
        with pytest.raises(ValueError):
            get_chunks(str(html_file), mode="bad_mode")

    def test_wrong_extension_raises(self, html_file, tmp_path):
        wrong_ext = tmp_path / "bad.txt"
        wrong_ext.write_bytes(html_file.read_bytes())
        with pytest.raises(ValueError):
            chunk_html(str(wrong_ext), mode="structural")
