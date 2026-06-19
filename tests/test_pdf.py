# pylint: disable=redefined-outer-name
from io import BytesIO
from pathlib import Path

import pytest

import py_chunks as _py_chunks
from py_chunks import (
    get_chunks,
    get_chunks_from_bytes,
    get_chunks_from_fileobj,
    get_chunks_from_upload,
    stream_chunks_from_bytes,
    stream_chunks_from_fileobj,
)

chunk_pdf = getattr(_py_chunks, "chunk_pdf")
stream_chunk_pdf = getattr(_py_chunks, "stream_chunk_pdf")


class _DummyUpload:
    def __init__(self, data: bytes, filename: str):
        self.filename = filename
        self.file = BytesIO(data)

TEST_FILES_DIR = Path(__file__).resolve().parents[2] / "test_files"
SAMPLE_PDF = TEST_FILES_DIR / "sample-pdf.pdf"
SCANNED_PDF = TEST_FILES_DIR / "large-doc.pdf"

if not SAMPLE_PDF.exists():
    pytest.skip("Missing test_files/sample-pdf.pdf", allow_module_level=True)

MODES = [
    "default",
    "structural",
    "section",
    "semantic",
    "sentence",
    "sliding_window",
    "page_aware",
]


def _escape_pdf_text(value: str) -> str:
    return value.replace("\\", "\\\\").replace("(", "\\(").replace(")", "\\)")


def _build_table_pdf_bytes() -> bytes:
    lines = [
        (760, 18, "Quarterly Sales Summary"),
        (728, 12, "Product | Price | Qty"),
        (714, 12, "Widget | 10 | 2"),
        (701, 12, "Gadget | 15 | 4"),
        (688, 12, "Totals | 25 | 6"),
        (656, 12, "Revenue remained stable across regions."),
    ]
    content_lines = []
    for y, font_size, text in lines:
        content_lines.append("BT")
        content_lines.append(f"/F1 {font_size} Tf")
        content_lines.append(f"1 0 0 1 72 {y} Tm")
        content_lines.append(f"({_escape_pdf_text(text)}) Tj")
        content_lines.append("ET")
    stream = "\n".join(content_lines).encode("ascii")

    objects = [
        b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n",
        b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n",
        b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792]"
        b" /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>\nendobj\n",
        b"4 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>\nendobj\n",
        b"5 0 obj\n<< /Length "
        + str(len(stream)).encode("ascii")
        + b" >>\nstream\n" + stream + b"\nendstream\nendobj\n",
    ]

    pdf = bytearray(b"%PDF-1.4\n")
    offsets = [0]
    for obj in objects:
        offsets.append(len(pdf))
        pdf.extend(obj)

    xref_start = len(pdf)
    pdf.extend(f"xref\n0 {len(offsets)}\n".encode("ascii"))
    pdf.extend(b"0000000000 65535 f \n")
    for offset in offsets[1:]:
        pdf.extend(f"{offset:010d} 00000 n \n".encode("ascii"))
    pdf.extend(
        f"trailer\n<< /Size {len(offsets)} /Root 1 0 R >>\nstartxref\n{xref_start}\n%%EOF\n".encode(
            "ascii"
        )
    )
    return bytes(pdf)


@pytest.fixture(scope="module")
def table_pdf(tmp_path_factory: pytest.TempPathFactory) -> Path:
    pdf_path = tmp_path_factory.mktemp("pdf_tables") / "table.pdf"
    pdf_path.write_bytes(_build_table_pdf_bytes())
    return pdf_path


def _mode_kwargs(mode: str) -> dict:
    if mode == "sentence":
        return {"sentences_per_chunk": 3}
    if mode == "sliding_window":
        return {"window_size": 3, "overlap": 1}
    if mode == "page_aware":
        return {"paragraphs_per_page": 15}
    return {}


@pytest.mark.parametrize("mode", MODES)
def test_pdf_schema_and_document_metadata(mode: str):
    chunks, timing = chunk_pdf(str(SAMPLE_PDF), mode=mode, **_mode_kwargs(mode))

    assert chunks, f"Expected non-empty chunks for mode={mode}"
    assert "rust_ms" in timing and "python_ms" in timing

    for chunk in chunks:
        assert {"content", "content_type", "metadata"}.issubset(chunk.keys())
        assert isinstance(chunk["content"], str) and chunk["content"].strip()
        assert isinstance(chunk["content_type"], str) and chunk["content_type"]
        assert isinstance(chunk["metadata"], dict)

        doc_meta = chunk["metadata"].get("document_metadata")
        assert isinstance(doc_meta, dict), f"Missing document_metadata for mode={mode}"
        assert doc_meta.get("source_type") == "pdf"
        assert isinstance(doc_meta.get("total_pages"), int) and doc_meta["total_pages"] >= 1


@pytest.mark.parametrize("mode", MODES)
def test_pdf_stream_schema_and_document_metadata(mode: str):
    chunks = list(stream_chunk_pdf(str(SAMPLE_PDF), mode=mode, **_mode_kwargs(mode)))
    assert chunks, f"Expected non-empty streamed chunks for mode={mode}"

    for chunk in chunks:
        assert {"content", "content_type", "metadata"}.issubset(chunk.keys())
        doc_meta = chunk["metadata"].get("document_metadata")
        assert isinstance(doc_meta, dict), f"Missing stream document_metadata for mode={mode}"
        assert doc_meta.get("source_type") == "pdf"
        assert isinstance(doc_meta.get("total_pages"), int) and doc_meta["total_pages"] >= 1


def _find_table_like_chunk(chunks: list[dict]) -> dict | None:
    for chunk in chunks:
        content = chunk["content"]
        if "Widget | 10 | 2" in content and "Gadget | 15 | 4" in content:
            return chunk
    return None


@pytest.mark.parametrize("mode", MODES)
def test_pdf_table_content_is_preserved_across_modes(mode: str, table_pdf: Path):
    chunks, _ = chunk_pdf(str(table_pdf), mode=mode, **_mode_kwargs(mode))
    stream_chunks = list(stream_chunk_pdf(str(table_pdf), mode=mode, **_mode_kwargs(mode)))

    table_chunk = _find_table_like_chunk(chunks)
    stream_table_chunk = _find_table_like_chunk(stream_chunks)

    assert table_chunk is not None, f"Expected table content in mode={mode}"
    assert stream_table_chunk is not None, f"Expected streamed table content in mode={mode}"
    assert "\n" in table_chunk["content"]
    assert "\n" in stream_table_chunk["content"]


def test_pdf_table_chunks_are_classified_in_default_and_structural(table_pdf: Path):
    for mode in ("default", "structural"):
        chunks, _ = chunk_pdf(str(table_pdf), mode=mode)
        stream_chunks = list(stream_chunk_pdf(str(table_pdf), mode=mode))

        assert any(chunk["content_type"] == "table" for chunk in chunks), mode
        assert any(chunk["content_type"] == "table" for chunk in stream_chunks), mode


def test_pdf_table_boundaries_are_preserved_in_semantic_mode(table_pdf: Path):
    chunks, _ = chunk_pdf(str(table_pdf), mode="semantic")
    stream_chunks = list(stream_chunk_pdf(str(table_pdf), mode="semantic"))

    table_chunk = _find_table_like_chunk(chunks)
    stream_table_chunk = _find_table_like_chunk(stream_chunks)

    assert table_chunk is not None
    assert stream_table_chunk is not None
    assert table_chunk["metadata"].get("merge_reason") == "table_boundary"
    assert stream_table_chunk["metadata"].get("merge_reason") == "table_boundary"


def test_pdf_sentence_mode_marks_table_chunks(table_pdf: Path):
    chunks, _ = chunk_pdf(str(table_pdf), mode="sentence", sentences_per_chunk=3)
    stream_chunks = list(
        stream_chunk_pdf(str(table_pdf), mode="sentence", sentences_per_chunk=3)
    )

    table_chunks = [chunk for chunk in chunks if chunk["metadata"].get("contains_table")]
    stream_table_chunks = [
        chunk for chunk in stream_chunks if chunk["metadata"].get("contains_table")
    ]

    assert table_chunks
    assert stream_table_chunks
    assert all("|" in chunk["content"] for chunk in table_chunks)
    assert all("|" in chunk["content"] for chunk in stream_table_chunks)
    assert any("\n" in chunk["content"] for chunk in table_chunks)
    assert any("\n" in chunk["content"] for chunk in stream_table_chunks)


def test_section_mode_has_no_micro_chunks():
    chunks, _ = chunk_pdf(str(SAMPLE_PDF), mode="section")
    assert min(len(c["content"]) for c in chunks) >= 20

    stream_chunks = list(stream_chunk_pdf(str(SAMPLE_PDF), mode="section"))
    assert min(len(c["content"]) for c in stream_chunks) >= 20


def test_sentence_mode_includes_page_number():
    chunks, _ = chunk_pdf(str(SAMPLE_PDF), mode="sentence", sentences_per_chunk=3)
    assert all("page_number" in c["metadata"] for c in chunks)

    stream_chunks = list(
        stream_chunk_pdf(str(SAMPLE_PDF), mode="sentence", sentences_per_chunk=3)
    )
    assert all("page_number" in c["metadata"] for c in stream_chunks)


def test_sliding_window_includes_page_range():
    chunks, _ = chunk_pdf(str(SAMPLE_PDF), mode="sliding_window", window_size=3, overlap=1)
    assert all(
        "page_range" in c["metadata"] and len(c["metadata"]["page_range"]) == 2
        for c in chunks
    )

    stream_chunks = list(
        stream_chunk_pdf(str(SAMPLE_PDF), mode="sliding_window", window_size=3, overlap=1)
    )
    assert all(
        "page_range" in c["metadata"] and len(c["metadata"]["page_range"]) == 2
        for c in stream_chunks
    )


def test_structural_heading_fraction_not_overclassified_on_uniform_pdf():
    uniform_pdf = TEST_FILES_DIR / "sample-10mb.pdf"
    if not uniform_pdf.exists():
        pytest.skip("Missing test_files/sample-10mb.pdf")

    chunks, _ = chunk_pdf(str(uniform_pdf), mode="structural")
    heading_count = sum(1 for c in chunks if c["content_type"] == "heading")
    heading_ratio = heading_count / max(len(chunks), 1)

    assert heading_ratio < 0.5, f"heading ratio too high: {heading_ratio:.3f}"


def test_invalid_inputs(tmp_path: Path):
    missing_pdf = tmp_path / "does-not-exist.pdf"
    with pytest.raises(FileNotFoundError):
        chunk_pdf(str(missing_pdf), mode="default")

    bad_ext = TEST_FILES_DIR / "test.txt"
    with pytest.raises(ValueError):
        chunk_pdf(str(bad_ext), mode="default")

    with pytest.raises(ValueError):
        chunk_pdf(str(SAMPLE_PDF), mode="bad-mode")


def test_scanned_pdf_error_has_ocr_hint():
    if not SCANNED_PDF.exists():
        pytest.skip("Missing test_files/large-doc.pdf")

    with pytest.raises(RuntimeError, match="OCR preprocessing"):
        chunk_pdf(str(SCANNED_PDF), mode="default")

    with pytest.raises(RuntimeError, match="OCR preprocessing"):
        list(stream_chunk_pdf(str(SCANNED_PDF), mode="default"))


@pytest.mark.parametrize("mode", MODES)
def test_sample_pdf_performance_under_500ms(mode: str):
    _, timing = chunk_pdf(str(SAMPLE_PDF), mode=mode, **_mode_kwargs(mode))
    assert timing["python_ms"] < 500, f"mode={mode} took {timing['python_ms']}ms"


class TestPdfSourceEquivalence:
    def test_bytes_equals_path(self):
        by_path = get_chunks(str(SAMPLE_PDF), mode="structural")
        by_bytes = get_chunks_from_bytes(
            Path(SAMPLE_PDF).read_bytes(), filename="x.pdf", mode="structural"
        )
        assert [c["content"] for c in by_bytes] == [c["content"] for c in by_path]

    def test_fileobj_equals_path(self):
        by_path = get_chunks(str(SAMPLE_PDF), mode="structural")
        with open(SAMPLE_PDF, "rb") as f:
            by_fileobj = get_chunks_from_fileobj(f, filename="x.pdf", mode="structural")
        assert [c["content"] for c in by_fileobj] == [c["content"] for c in by_path]

    def test_upload_equals_path(self):
        by_path = get_chunks(str(SAMPLE_PDF), mode="structural")
        upload = _DummyUpload(Path(SAMPLE_PDF).read_bytes(), "x.pdf")
        by_upload = get_chunks_from_upload(upload, mode="structural")
        assert [c["content"] for c in by_upload] == [c["content"] for c in by_path]


class TestPdfStreamingSourceEquivalence:
    def test_stream_bytes_equals_path(self):
        by_path = list(stream_chunk_pdf(str(SAMPLE_PDF), mode="structural"))
        data = Path(SAMPLE_PDF).read_bytes()
        by_bytes = list(
            stream_chunks_from_bytes(data, filename="x.pdf", mode="structural")
        )
        assert [c["content"] for c in by_bytes] == [c["content"] for c in by_path]

    def test_stream_fileobj_equals_path(self):
        by_path = list(stream_chunk_pdf(str(SAMPLE_PDF), mode="structural"))
        with open(SAMPLE_PDF, "rb") as f:
            by_fileobj = list(
                stream_chunks_from_fileobj(f, filename="x.pdf", mode="structural")
            )
        assert [c["content"] for c in by_fileobj] == [c["content"] for c in by_path]


class TestPdfStreamingValidation:
    def test_sliding_window_window_size_zero_raises(self):
        with pytest.raises(ValueError):
            stream_chunk_pdf(str(SAMPLE_PDF), mode="sliding_window", window_size=0)

    def test_sliding_window_overlap_gte_window_size_raises(self):
        with pytest.raises(ValueError):
            stream_chunk_pdf(
                str(SAMPLE_PDF), mode="sliding_window", window_size=3, overlap=3
            )

    def test_sentence_sentences_per_chunk_zero_raises(self):
        with pytest.raises(ValueError):
            stream_chunk_pdf(str(SAMPLE_PDF), mode="sentence", sentences_per_chunk=0)

    def test_invalid_mode_raises(self):
        with pytest.raises((ValueError, NotImplementedError)):
            stream_chunk_pdf(str(SAMPLE_PDF), mode="not_a_real_mode")
