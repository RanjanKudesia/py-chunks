"""Contract tests for XLS (.xls) chunking — all 6 modes, batch + streaming."""

from io import BytesIO
from pathlib import Path

import pytest
import xlwt

from py_chunks import get_chunks, get_chunks_from_bytes, stream_chunks
from py_chunks.chunkers.xlsx import chunk_xlsx, stream_chunk_xlsx


# ── Fixture helpers ───────────────────────────────────────────────────────────

def _make_xls_bytes(*, rows: int = 10, second_sheet: bool = True) -> bytes:
    wb = xlwt.Workbook()
    ws = wb.add_sheet("Sales")
    ws.write(0, 0, "Product")
    ws.write(0, 1, "Region")
    ws.write(0, 2, "Amount")
    categories = ["Electronics", "Clothing", "Food"]
    regions = ["North", "South", "East"]
    for i in range(rows):
        ws.write(i + 1, 0, categories[i % len(categories)])
        ws.write(i + 1, 1, regions[i % len(regions)])
        ws.write(i + 1, 2, (i + 1) * 100)

    if second_sheet:
        ws2 = wb.add_sheet("Staff")
        ws2.write(0, 0, "Name")
        ws2.write(0, 1, "Role")
        ws2.write(1, 0, "Alice")
        ws2.write(1, 1, "Manager")
        ws2.write(2, 0, "Bob")
        ws2.write(2, 1, "Engineer")

    buf = BytesIO()
    wb.save(buf)
    return buf.getvalue()


def _write_xls(tmp_path: Path, name: str = "sample.xls", **kwargs) -> Path:
    path = tmp_path / name
    path.write_bytes(_make_xls_bytes(**kwargs))
    return path


MODES = ["row", "sliding_window", "table", "sheet", "page_aware", "semantic"]


# ── Basic schema tests ────────────────────────────────────────────────────────

def test_xls_row_mode_returns_chunks(tmp_path):
    path = _write_xls(tmp_path)
    chunks, timing = chunk_xlsx(str(path), mode="row", rows_per_chunk=3)
    assert chunks
    for c in chunks:
        assert {"content", "content_type", "metadata"} <= c.keys()
        assert c["content"]
        assert c["content_type"] == "row_document"
    assert timing["rust_ms"] > 0


def test_xls_header_detected(tmp_path):
    path = _write_xls(tmp_path, second_sheet=False)
    chunks, _ = chunk_xlsx(str(path), mode="row", rows_per_chunk=1)
    assert chunks[0]["metadata"]["header_row"] == ["Product", "Region", "Amount"]


def test_xls_row_count_correct(tmp_path):
    path = _write_xls(tmp_path, rows=10, second_sheet=False)
    chunks, _ = chunk_xlsx(str(path), mode="row", rows_per_chunk=1)
    assert len(chunks) == 10


def test_xls_include_headers_false(tmp_path):
    path = _write_xls(tmp_path, second_sheet=False)
    chunks, _ = chunk_xlsx(str(path), mode="row", rows_per_chunk=1, include_headers=False)
    assert chunks
    assert "Product:" not in chunks[0]["content"]


# ── All 6 modes run without error ─────────────────────────────────────────────

@pytest.mark.parametrize("mode", MODES)
def test_xls_all_modes_no_error(tmp_path, mode):
    path = _write_xls(tmp_path, rows=20)
    chunks, _ = chunk_xlsx(
        str(path),
        mode=mode,
        rows_per_chunk=5,
        window_size=4,
        overlap=1,
        max_chunk_chars=2000,
    )
    assert chunks
    for c in chunks:
        assert c["content"]
        assert c["content_type"]


# ── Multi-sheet ───────────────────────────────────────────────────────────────

def test_xls_multi_sheet_chunks(tmp_path):
    path = _write_xls(tmp_path, second_sheet=True)
    chunks, _ = chunk_xlsx(str(path), mode="row", rows_per_chunk=1)
    sheet_names = {c["metadata"]["sheet_name"] for c in chunks}
    assert "Sales" in sheet_names
    assert "Staff" in sheet_names


def test_xls_sheet_filter(tmp_path):
    path = _write_xls(tmp_path, second_sheet=True)
    chunks, _ = chunk_xlsx(str(path), mode="row", rows_per_chunk=1, sheet_names=["Staff"])
    assert all(c["metadata"]["sheet_name"] == "Staff" for c in chunks)


# ── Sliding window ────────────────────────────────────────────────────────────

def test_xls_sliding_window_metadata(tmp_path):
    path = _write_xls(tmp_path, rows=10, second_sheet=False)
    chunks, _ = chunk_xlsx(str(path), mode="sliding_window", window_size=3, overlap=1)
    for c in chunks:
        assert c["content_type"] == "row_window"
        m = c["metadata"]
        assert "window_size" in m
        assert "overlap" in m
        assert m["window_size"] == 3
        assert m["overlap"] == 1


# ── Table mode falls back to heuristic (no named tables in XLS) ───────────────

def test_xls_table_mode_heuristic_fallback(tmp_path):
    path = _write_xls(tmp_path, rows=10, second_sheet=False)
    chunks, _ = chunk_xlsx(str(path), mode="table", max_chunk_chars=5000)
    assert chunks
    for c in chunks:
        assert c["content_type"] == "table_region"
        assert c["metadata"]["is_named_table"] is False


# ── Page-aware falls back to full sheet (no print areas in XLS) ───────────────

def test_xls_page_aware_no_print_area(tmp_path):
    path = _write_xls(tmp_path, rows=10, second_sheet=False)
    chunks, _ = chunk_xlsx(str(path), mode="page_aware", max_chunk_chars=5000)
    assert chunks
    for c in chunks:
        assert c["content_type"] == "sheet_region"
        assert c["metadata"]["has_print_area"] is False


# ── Semantic mode ─────────────────────────────────────────────────────────────

def test_xls_semantic_groups_by_category(tmp_path):
    path = _write_xls(tmp_path, rows=12, second_sheet=False)
    chunks, _ = chunk_xlsx(str(path), mode="semantic", rows_per_chunk=3)
    assert chunks
    for c in chunks:
        assert c["content_type"] == "semantic_group"


# ── Streaming parity ──────────────────────────────────────────────────────────

@pytest.mark.parametrize("mode", MODES)
def test_xls_stream_matches_batch(tmp_path, mode):
    path = _write_xls(tmp_path, rows=15)
    batch_chunks, _ = chunk_xlsx(
        str(path), mode=mode, rows_per_chunk=4, window_size=4, overlap=1, max_chunk_chars=3000
    )
    stream_chunks_list = list(stream_chunk_xlsx(
        str(path), mode=mode, rows_per_chunk=4, window_size=4, overlap=1, max_chunk_chars=3000
    ))
    assert len(stream_chunks_list) == len(batch_chunks)
    for i, (b, s) in enumerate(zip(batch_chunks, stream_chunks_list)):
        assert b["content"] == s["content"], f"content mismatch at index {i}"
        assert b["content_type"] == s["content_type"]
        assert b["metadata"] == s["metadata"], f"metadata mismatch at index {i}"


# ── Extension validation ──────────────────────────────────────────────────────

def test_xls_wrong_extension_raises(tmp_path):
    path = tmp_path / "data.csv"
    path.write_bytes(b"a,b\n1,2")
    with pytest.raises(ValueError, match=r"\.xls"):
        chunk_xlsx(str(path))


def test_xls_missing_file_raises(tmp_path):
    with pytest.raises(FileNotFoundError):
        chunk_xlsx(str(tmp_path / "missing.xls"))


# ── Public API routing ────────────────────────────────────────────────────────

def test_xls_get_chunks_from_path(tmp_path):
    path = _write_xls(tmp_path)
    chunks = get_chunks(str(path))
    assert chunks
    assert all(c["content_type"] == "row_document" for c in chunks)


def test_xls_get_chunks_from_bytes(tmp_path):
    data = _make_xls_bytes(rows=5, second_sheet=False)
    chunks = get_chunks_from_bytes(data, "report.xls")
    assert chunks


def test_xls_stream_chunks_from_path(tmp_path):
    path = _write_xls(tmp_path)
    result = list(stream_chunks(str(path)))
    assert result
    assert all(c["content_type"] == "row_document" for c in result)
