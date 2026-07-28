"""Fixture-driven coverage for the real `.csv` and `.htm` corpora (vega,
plotly, fivethirtyeight, seaborn datasets; W3C `.htm` pages). These formats
previously had no dedicated fixtures in test_files/."""

from pathlib import Path

import pytest

from py_chunks import get_chunks, get_chunks_from_bytes, get_markdown, stream_chunks

_TEST_FILES = Path(__file__).resolve().parents[2] / "test_files"
CSV_DIR = _TEST_FILES / "csv"
HTML_DIR = _TEST_FILES / "html"

STANDARD_KEYS = {"content", "content_type", "metadata"}


def csv_files() -> list[Path]:
    return sorted(CSV_DIR.glob("*.csv")) if CSV_DIR.is_dir() else []


def htm_files() -> list[Path]:
    return sorted(HTML_DIR.glob("*.htm")) if HTML_DIR.is_dir() else []


def _require(files) -> None:
    if not files:
        pytest.skip("no fixtures present")


def test_csv_fixtures_present():
    _require(csv_files())
    assert len(csv_files()) >= 5


def test_csv_schema_and_parity():
    _require(csv_files())
    for f in csv_files():
        b = get_chunks(str(f))
        assert b, f"{f.name}: no chunks"
        for c in b:
            assert STANDARD_KEYS.issubset(c.keys())
        s = list(stream_chunks(str(f)))
        y = get_chunks_from_bytes(f.read_bytes(), f.name)
        assert len(b) == len(s) == len(y), f"{f.name}: paths diverge"


def test_csv_markdown_is_table():
    _require(csv_files())
    md = get_markdown(str(csv_files()[0]))
    assert "|" in md and "---" in md, "CSV markdown should render a pipe table"


def test_htm_fixtures_present_and_parse():
    _require(htm_files())
    for f in htm_files():
        b = get_chunks(str(f))
        assert b, f"{f.name}: no chunks"
        s = list(stream_chunks(str(f)))
        y = get_chunks_from_bytes(f.read_bytes(), f.name)
        assert len(b) == len(s) == len(y), f"{f.name}: paths diverge"
