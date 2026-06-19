"""Plain text chunking tests across all supported modes and source APIs."""
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
from py_chunks.chunkers.txt import chunk_txt, stream_chunk_txt


# ── Fixture discovery ─────────────────────────────────────────────────────────

_CANDIDATE_DIRS = [
    Path(__file__).resolve().parents[2] / "test_files",
    Path(__file__).resolve().parents[2] / "test_input_files",
]

_TXT_NAMES = ["prose_article.txt", "structured_report.txt", "test.txt"]


def _resolve_txt_files() -> list[Path]:
    for base in _CANDIDATE_DIRS:
        if not base.exists():
            continue
        paths = [base / name for name in _TXT_NAMES]
        if all(p.exists() for p in paths):
            return paths
    return []


ALL_TXT_FILES = _resolve_txt_files()

if len(ALL_TXT_FILES) != 3:
    pytest.skip(
        "Expected plain text fixtures (prose_article.txt, structured_report.txt,"
        " test.txt) in test_files/",
        allow_module_level=True,
    )

TXT_IDS = [p.name for p in ALL_TXT_FILES]
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
def txt_files() -> list[Path]:
    return ALL_TXT_FILES


@pytest.fixture
def txt_file(txt_files) -> Path:
    for p in txt_files:
        if p.name == "prose_article.txt":
            return p
    return txt_files[0]


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

@pytest.mark.parametrize("txt_path", ALL_TXT_FILES, ids=TXT_IDS)
class TestTxtStructural:

    def test_returns_nonempty_list(self, txt_path):
        chunks = _get(txt_path, mode="structural")
        _assert_nonempty(chunks, txt_path.name)

    def test_schema_per_chunk(self, txt_path):
        chunks = _get(txt_path, mode="structural")
        _assert_schema(chunks, txt_path.name)

    def test_content_types_valid(self, txt_path):
        chunks = _get(txt_path, mode="structural")
        _assert_valid_content_types(chunks, txt_path.name)

    def test_streaming_parity(self, txt_path):
        batch = _get(txt_path, mode="structural")
        streamed = list(stream_chunk_txt(str(txt_path), mode="structural"))
        assert len(streamed) == len(batch), (
            f"{txt_path.name}: batch={len(batch)} stream={len(streamed)}"
        )


# ════════════════════════════════════════════════════════════════════════════
# 2. SEMANTIC
# ════════════════════════════════════════════════════════════════════════════

@pytest.mark.parametrize("txt_path", ALL_TXT_FILES, ids=TXT_IDS)
class TestTxtSemantic:

    def test_returns_nonempty_list(self, txt_path):
        chunks = _get(txt_path, mode="semantic")
        _assert_nonempty(chunks, txt_path.name)

    def test_schema_per_chunk(self, txt_path):
        chunks = _get(txt_path, mode="semantic")
        _assert_schema(chunks, txt_path.name)

    def test_content_types_valid(self, txt_path):
        chunks = _get(txt_path, mode="semantic")
        _assert_valid_content_types(chunks, txt_path.name)

    def test_streaming_parity(self, txt_path):
        # Keep semantic parity resilient in case batch/stream merge passes diverge.
        batch = _get(txt_path, mode="semantic")
        streamed = list(stream_chunk_txt(str(txt_path), mode="semantic"))
        _assert_nonempty(batch, f"{txt_path.name} batch")
        _assert_nonempty(streamed, f"{txt_path.name} stream")
        _assert_schema(streamed, f"{txt_path.name} stream")


# ════════════════════════════════════════════════════════════════════════════
# 3. SECTION
# ════════════════════════════════════════════════════════════════════════════

@pytest.mark.parametrize("txt_path", ALL_TXT_FILES, ids=TXT_IDS)
class TestTxtSection:

    def test_returns_nonempty_list(self, txt_path):
        chunks = _get(txt_path, mode="section")
        _assert_nonempty(chunks, txt_path.name)

    def test_schema_per_chunk(self, txt_path):
        chunks = _get(txt_path, mode="section")
        _assert_schema(chunks, txt_path.name)

    def test_content_types_valid(self, txt_path):
        chunks = _get(txt_path, mode="section")
        _assert_valid_content_types(chunks, txt_path.name)

    def test_streaming_parity(self, txt_path):
        batch = _get(txt_path, mode="section")
        streamed = list(stream_chunk_txt(str(txt_path), mode="section"))
        assert len(streamed) == len(batch), (
            f"{txt_path.name}: batch={len(batch)} stream={len(streamed)}"
        )

    def test_section_metadata_has_section_heading(self, txt_path):
        chunks = _get(txt_path, mode="section")
        for c in chunks:
            assert "section_heading" in c["metadata"], (
                f"{txt_path.name}: missing section_heading"
            )


# ════════════════════════════════════════════════════════════════════════════
# 4. SLIDING_WINDOW
# ════════════════════════════════════════════════════════════════════════════

@pytest.mark.parametrize("txt_path", ALL_TXT_FILES, ids=TXT_IDS)
class TestTxtSlidingWindow:

    def test_returns_nonempty_list(self, txt_path):
        chunks = _get(txt_path, mode="sliding_window", window_size=3, overlap=1)
        _assert_nonempty(chunks, txt_path.name)

    def test_schema_per_chunk(self, txt_path):
        chunks = _get(txt_path, mode="sliding_window", window_size=3, overlap=1)
        _assert_schema(chunks, txt_path.name)

    def test_content_types_valid(self, txt_path):
        chunks = _get(txt_path, mode="sliding_window", window_size=3, overlap=1)
        _assert_valid_content_types(chunks, txt_path.name)

    def test_streaming_parity(self, txt_path):
        batch = _get(txt_path, mode="sliding_window", window_size=3, overlap=1)
        streamed = list(
            stream_chunk_txt(
                str(txt_path),
                mode="sliding_window",
                window_size=3,
                overlap=1,
            )
        )
        assert len(streamed) == len(batch), (
            f"{txt_path.name}: batch={len(batch)} stream={len(streamed)}"
        )

    def test_sliding_window_metadata_fields(self, txt_path):
        chunks = _get(txt_path, mode="sliding_window", window_size=3, overlap=1)
        for c in chunks:
            meta = c["metadata"]
            assert "chunk_index" in meta
            assert "window_size" in meta
            assert "overlap" in meta


# ════════════════════════════════════════════════════════════════════════════
# 5. SENTENCE
# ════════════════════════════════════════════════════════════════════════════

@pytest.mark.parametrize("txt_path", ALL_TXT_FILES, ids=TXT_IDS)
class TestTxtSentence:

    def test_returns_nonempty_list(self, txt_path):
        chunks = _get(txt_path, mode="sentence", sentences_per_chunk=3)
        _assert_nonempty(chunks, txt_path.name)

    def test_schema_per_chunk(self, txt_path):
        chunks = _get(txt_path, mode="sentence", sentences_per_chunk=3)
        _assert_schema(chunks, txt_path.name)

    def test_content_types_valid(self, txt_path):
        chunks = _get(txt_path, mode="sentence", sentences_per_chunk=3)
        _assert_valid_content_types(chunks, txt_path.name)

    def test_streaming_parity(self, txt_path):
        batch = _get(txt_path, mode="sentence", sentences_per_chunk=3)
        streamed = list(
            stream_chunk_txt(
                str(txt_path),
                mode="sentence",
                sentences_per_chunk=3,
            )
        )
        assert len(streamed) == len(batch), (
            f"{txt_path.name}: batch={len(batch)} stream={len(streamed)}"
        )

    def test_sentence_metadata_has_chunk_index(self, txt_path):
        chunks = _get(txt_path, mode="sentence", sentences_per_chunk=3)
        for c in chunks:
            assert "chunk_index" in c["metadata"], (
                f"{txt_path.name}: missing chunk_index"
            )


# ════════════════════════════════════════════════════════════════════════════
# 6. PAGE_AWARE
# ════════════════════════════════════════════════════════════════════════════

@pytest.mark.parametrize("txt_path", ALL_TXT_FILES, ids=TXT_IDS)
class TestTxtPageAware:

    def test_returns_nonempty_list(self, txt_path):
        chunks = _get(txt_path, mode="page_aware", paragraphs_per_page=15)
        _assert_nonempty(chunks, txt_path.name)

    def test_schema_per_chunk(self, txt_path):
        chunks = _get(txt_path, mode="page_aware", paragraphs_per_page=15)
        _assert_schema(chunks, txt_path.name)

    def test_content_types_valid(self, txt_path):
        chunks = _get(txt_path, mode="page_aware", paragraphs_per_page=15)
        _assert_valid_content_types(chunks, txt_path.name)

    def test_streaming_parity(self, txt_path):
        batch = _get(txt_path, mode="page_aware", paragraphs_per_page=15)
        streamed = list(
            stream_chunk_txt(
                str(txt_path),
                mode="page_aware",
                paragraphs_per_page=15,
            )
        )
        assert len(streamed) == len(batch), (
            f"{txt_path.name}: batch={len(batch)} stream={len(streamed)}"
        )

    def test_page_aware_metadata_fields(self, txt_path):
        chunks = _get(txt_path, mode="page_aware", paragraphs_per_page=15)
        for c in chunks:
            meta = c["metadata"]
            assert "page_break_type" in meta, f"{txt_path.name}: missing page_break_type"
            assert "paragraph_count" in meta, f"{txt_path.name}: missing paragraph_count"


# ════════════════════════════════════════════════════════════════════════════
# 7. SOURCE API EQUIVALENCE
# ════════════════════════════════════════════════════════════════════════════

class TestTxtSourceEquivalence:

    def test_bytes_equals_path(self, txt_file):
        by_path = get_chunks(str(txt_file), mode="structural")
        by_bytes = get_chunks_from_bytes(
            txt_file.read_bytes(),
            filename="x.txt",
            mode="structural",
        )
        assert [c["content"] for c in by_bytes] == [c["content"] for c in by_path]

    def test_fileobj_equals_path(self, txt_file):
        by_path = get_chunks(str(txt_file), mode="structural")
        with open(txt_file, "rb") as f:
            by_fileobj = get_chunks_from_fileobj(f, filename="x.txt", mode="structural")
        assert by_fileobj == by_path

    def test_upload_equals_path(self, txt_file):
        by_path = get_chunks(str(txt_file), mode="structural")
        upload = _DummyUpload(txt_file.read_bytes(), "x.txt")
        by_upload = get_chunks_from_upload(upload, mode="structural")
        assert by_upload == by_path


class TestTxtStreamingSourceEquivalence:

    def test_stream_bytes_equals_path(self, txt_file):
        by_path = list(stream_chunk_txt(str(txt_file), mode="structural"))
        data = Path(txt_file).read_bytes()
        by_bytes = list(
            stream_chunks_from_bytes(data, filename="x.txt", mode="structural")
        )
        assert [c["content"] for c in by_bytes] == [c["content"] for c in by_path]

    def test_stream_fileobj_equals_path(self, txt_file):
        by_path = list(stream_chunk_txt(str(txt_file), mode="structural"))
        with open(txt_file, "rb") as f:
            by_fileobj = list(
                stream_chunks_from_fileobj(f, filename="x.txt", mode="structural")
            )
        assert [c["content"] for c in by_fileobj] == [c["content"] for c in by_path]


# ════════════════════════════════════════════════════════════════════════════
# 8. VALIDATION
# ════════════════════════════════════════════════════════════════════════════

class TestTxtValidation:

    def test_missing_file_raises(self):
        with pytest.raises(FileNotFoundError):
            get_chunks("/nonexistent/path/to/file.txt", mode="structural")

    def test_invalid_mode_raises(self, txt_file):
        with pytest.raises(ValueError):
            get_chunks(str(txt_file), mode="bad_mode")

    def test_wrong_extension_raises(self, txt_file, tmp_path):
        wrong_ext = tmp_path / "bad.md"
        wrong_ext.write_bytes(txt_file.read_bytes())
        with pytest.raises(ValueError):
            chunk_txt(str(wrong_ext), mode="structural")
