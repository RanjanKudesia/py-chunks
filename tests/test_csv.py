from io import StringIO
from pathlib import Path

import pytest

from py_chunks import get_chunks
from py_chunks.chunkers.csv import chunk_csv, stream_chunk_csv


def _write_csv(tmp_path: Path, name: str, text: str) -> Path:
    path = tmp_path / name
    buffer = StringIO()
    buffer.write(text)
    path.write_text(buffer.getvalue(), encoding="utf-8")
    return path


def _sample_csv_text() -> str:
    return """Name,Revenue,Region
Alpha,120,North

Beta,200,South
Gamma,340,West
Delta,500,East
"""


def test_row_mode_chunks_and_metadata(tmp_path: Path):
    path = _write_csv(tmp_path, "sample.csv", _sample_csv_text())

    chunks, timing = chunk_csv(str(path), mode="row", rows_per_chunk=2)

    assert len(chunks) == 2
    assert chunks[0]["content_type"] == "row_group"
    assert chunks[0]["metadata"]["row_start"] == 1
    assert chunks[0]["metadata"]["row_end"] == 2
    assert chunks[0]["content"] == "Name: Alpha | Revenue: 120 | Region: North\nName: Beta | Revenue: 200 | Region: South"
    assert chunks[1]["metadata"]["row_start"] == 3
    assert timing["rust_ms"] > 0


def test_include_headers_false_omits_column_names(tmp_path: Path):
    path = _write_csv(tmp_path, "sample.csv", _sample_csv_text())

    chunks, _ = chunk_csv(str(path), rows_per_chunk=2, include_headers=False)

    assert "Name:" not in chunks[0]["content"]
    assert chunks[0]["content"].startswith("Alpha | 120 | North")


def test_page_aware_uses_row_chunking(tmp_path: Path):
    path = _write_csv(tmp_path, "sample.csv", _sample_csv_text())

    chunks, _ = chunk_csv(str(path), mode="page_aware", rows_per_chunk=3)

    assert len(chunks) == 2
    assert chunks[0]["metadata"]["row_count"] == 3
    assert chunks[0]["content_type"] == "row_group"


def test_sliding_window_metadata(tmp_path: Path):
    path = _write_csv(tmp_path, "sample.csv", _sample_csv_text())

    chunks, _ = chunk_csv(str(path), mode="sliding_window", rows_per_chunk=2, window_size=3, overlap=1)

    assert len(chunks) == 2
    assert chunks[0]["content_type"] == "row_window"
    assert chunks[0]["metadata"]["window_index"] == 0
    assert chunks[0]["metadata"]["window_size"] == 3
    assert chunks[0]["metadata"]["overlap"] == 1
    assert chunks[0]["metadata"]["actual_row_count"] == 3


@pytest.mark.parametrize(
    "mode,kwargs",
    [
        ("row", {"rows_per_chunk": 2}),
        ("default", {"rows_per_chunk": 2}),
        ("page_aware", {"rows_per_chunk": 3}),
        ("sliding_window", {"rows_per_chunk": 2, "window_size": 3, "overlap": 1}),
    ],
)
def test_streaming_matches_batch(tmp_path: Path, mode: str, kwargs: dict):
    path = _write_csv(tmp_path, f"{mode}.csv", _sample_csv_text())

    batch, _ = chunk_csv(str(path), mode=mode, **kwargs)
    streamed = list(stream_chunk_csv(str(path), mode=mode, **kwargs))

    assert streamed == batch


def test_delimiter_auto_detection_for_tab_and_semicolon(tmp_path: Path):
    tab_path = _write_csv(tmp_path, "tab.csv", "Name\tValue\nA\t1\nB\t2\n")
    semi_path = _write_csv(tmp_path, "semi.csv", "Name;Value\nA;1\nB;2\n")

    tab_chunks, _ = chunk_csv(str(tab_path), rows_per_chunk=2, delimiter=None)
    semi_chunks, _ = chunk_csv(str(semi_path), rows_per_chunk=2, delimiter=None)

    assert tab_chunks[0]["metadata"]["delimiter_detected"] == "\t"
    assert semi_chunks[0]["metadata"]["delimiter_detected"] == ";"


def test_unified_api_supports_csv(tmp_path: Path):
    path = _write_csv(tmp_path, "data.csv", _sample_csv_text())

    chunks = get_chunks(str(path), mode="default")
    assert chunks
    assert chunks[0]["content_type"] == "row_group"


def test_wrong_extension_raises_value_error(tmp_path: Path):
    path = tmp_path / "data.txt"
    path.write_text(_sample_csv_text(), encoding="utf-8")

    with pytest.raises(ValueError):
        chunk_csv(str(path))
    with pytest.raises(ValueError):
        stream_chunk_csv(str(path))


def test_large_csv_streaming_completes(tmp_path: Path):
    lines = ["Name,Value"] + [f"row{i},{i}" for i in range(10_000)]
    path = _write_csv(tmp_path, "large.csv", "\n".join(lines) + "\n")

    count = 0
    for _ in stream_chunk_csv(str(path), rows_per_chunk=500):
        count += 1

    assert count == 20