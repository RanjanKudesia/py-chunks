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

_TF_ROOT = Path(__file__).resolve().parents[2] / "test_files"
# PDF fixtures were reorganised into test_files/pdf/; fall back to the root.
TEST_FILES_DIR = _TF_ROOT / "pdf" if (_TF_ROOT / "pdf").is_dir() else _TF_ROOT
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


def test_pdf_table_content_survives_in_default_and_structural(table_pdf: Path):
    # liteparse renders pipe-aligned text as a preformatted/code block; the
    # markdown chunker preserves the tabular content verbatim. Formal `table`
    # classification is not guaranteed by the liteparse pipeline.
    for mode in ("default", "structural"):
        chunks, _ = chunk_pdf(str(table_pdf), mode=mode)
        stream_chunks = list(stream_chunk_pdf(str(table_pdf), mode=mode))

        assert _find_table_like_chunk(chunks) is not None, mode
        assert _find_table_like_chunk(stream_chunks) is not None, mode


def test_pdf_table_content_preserved_in_semantic_mode(table_pdf: Path):
    chunks, _ = chunk_pdf(str(table_pdf), mode="semantic")
    stream_chunks = list(stream_chunk_pdf(str(table_pdf), mode="semantic"))

    assert _find_table_like_chunk(chunks) is not None
    assert _find_table_like_chunk(stream_chunks) is not None


def test_pdf_sentence_mode_preserves_table_content(table_pdf: Path):
    chunks, _ = chunk_pdf(str(table_pdf), mode="sentence", sentences_per_chunk=3)
    joined = " ".join(c["content"] for c in chunks)
    assert "Widget | 10 | 2" in joined and "Gadget | 15 | 4" in joined


def test_section_mode_produces_non_empty_chunks():
    # Markdown headings are legitimately short chunks, so the old ">= 20 chars"
    # floor no longer applies; require non-empty content instead.
    chunks, _ = chunk_pdf(str(SAMPLE_PDF), mode="section")
    assert all(c["content"].strip() for c in chunks)

    stream_chunks = list(stream_chunk_pdf(str(SAMPLE_PDF), mode="section"))
    assert all(c["content"].strip() for c in stream_chunks)


def test_document_metadata_reports_total_pages():
    # Page attribution moved from per-chunk (page_number) to document level:
    # the markdown pipeline joins pages, exposing total_pages on document_metadata.
    chunks, _ = chunk_pdf(str(SAMPLE_PDF), mode="sentence", sentences_per_chunk=3)
    assert all(c["metadata"]["document_metadata"]["total_pages"] >= 1 for c in chunks)

    stream_chunks = list(
        stream_chunk_pdf(str(SAMPLE_PDF), mode="sentence", sentences_per_chunk=3)
    )
    assert all(
        c["metadata"]["document_metadata"]["total_pages"] >= 1 for c in stream_chunks
    )


def test_sliding_window_has_document_metadata():
    chunks, _ = chunk_pdf(str(SAMPLE_PDF), mode="sliding_window", window_size=3, overlap=1)
    assert all(
        c["metadata"]["document_metadata"]["source_type"] == "pdf" for c in chunks
    )

    stream_chunks = list(
        stream_chunk_pdf(str(SAMPLE_PDF), mode="sliding_window", window_size=3, overlap=1)
    )
    assert all(
        c["metadata"]["document_metadata"]["source_type"] == "pdf" for c in stream_chunks
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

    bad_ext = tmp_path / "not-a-pdf.txt"
    bad_ext.write_text("not a pdf")
    with pytest.raises(ValueError):
        chunk_pdf(str(bad_ext), mode="default")

    with pytest.raises(ValueError):
        chunk_pdf(str(SAMPLE_PDF), mode="bad-mode")


def test_scanned_pdf_handled_gracefully():
    # With OCR disabled, a scanned/image-only PDF yields (mostly empty) chunks
    # rather than raising an OCR-hint error — no crash.
    if not SCANNED_PDF.exists():
        pytest.skip("Missing test_files/large-doc.pdf")

    chunks, _ = chunk_pdf(str(SCANNED_PDF), mode="default")
    assert isinstance(chunks, list)
    streamed = list(stream_chunk_pdf(str(SCANNED_PDF), mode="default"))
    assert isinstance(streamed, list)


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
