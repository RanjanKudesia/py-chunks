"""Tests for csv_to_markdown."""

import csv
import os

import pytest
from pathlib import Path

from py_chunks import get_markdown
from py_chunks.chunkers.csv import csv_to_markdown


def write_csv(path, rows, delimiter=","):
    with open(path, "w", newline="", encoding="utf-8") as f:
        writer = csv.writer(f, delimiter=delimiter)
        writer.writerows(rows)
    return path


# ---------------------------------------------------------------------------
# Basic correctness
# ---------------------------------------------------------------------------


def test_returns_string(tmp_path):
    p = write_csv(tmp_path / "t.csv", [["H1", "H2"], ["a", "b"]])
    result = csv_to_markdown(str(p))
    assert isinstance(result, str)


def test_basic_pipe_table(tmp_path):
    p = write_csv(tmp_path / "t.csv", [["H1", "H2", "H3"], ["1", "2", "3"]])
    result = csv_to_markdown(str(p))
    lines = result.splitlines()
    assert lines[0] == "| H1 | H2 | H3 |"
    assert lines[1] == "| --- | --- | --- |"


def test_header_row_present(tmp_path):
    p = write_csv(tmp_path / "t.csv", [["Name", "Age"], ["Alice", "30"]])
    result = csv_to_markdown(str(p))
    lines = result.splitlines()
    assert "Name" in lines[0]
    assert "Age" in lines[0]
    assert "---" in lines[1]


def test_data_rows_present(tmp_path):
    p = write_csv(tmp_path / "t.csv", [["X", "Y"], ["10", "20"], ["30", "40"]])
    result = csv_to_markdown(str(p))
    lines = result.splitlines()
    assert len(lines) == 4  # header + sep + 2 data rows
    assert "10" in lines[2]
    assert "30" in lines[3]


def test_pipe_in_cell_escaped(tmp_path):
    p = write_csv(tmp_path / "t.csv", [["Col"], ["a|b"]])
    result = csv_to_markdown(str(p))
    assert r"a\|b" in result


def test_empty_rows_skipped(tmp_path):
    p = tmp_path / "t.csv"
    p.write_text("H1,H2\na,b\n,,\nc,d\n", encoding="utf-8")
    result = csv_to_markdown(str(p))
    lines = result.splitlines()
    # header + sep + 2 real rows (empty row skipped)
    assert len(lines) == 4


def test_empty_file_returns_empty_string(tmp_path):
    p = tmp_path / "t.csv"
    p.write_text("", encoding="utf-8")
    assert csv_to_markdown(str(p)) == ""


def test_single_header_no_data(tmp_path):
    p = write_csv(tmp_path / "t.csv", [["A", "B", "C"]])
    result = csv_to_markdown(str(p))
    lines = result.splitlines()
    assert len(lines) == 2
    assert "A" in lines[0]
    assert "---" in lines[1]


# ---------------------------------------------------------------------------
# Delimiters
# ---------------------------------------------------------------------------


def test_tab_delimited(tmp_path):
    p = write_csv(tmp_path / "t.csv", [["A", "B"], ["1", "2"]], delimiter="\t")
    result = csv_to_markdown(str(p), delimiter="\t")
    assert "| A | B |" in result
    assert "| 1 | 2 |" in result


def test_semicolon_delimited(tmp_path):
    p = write_csv(tmp_path / "t.csv", [["X", "Y"], ["foo", "bar"]], delimiter=";")
    result = csv_to_markdown(str(p), delimiter=";")
    assert "| X | Y |" in result
    assert "| foo | bar |" in result


def test_auto_detect_delimiter(tmp_path):
    p = tmp_path / "t.csv"
    p.write_text("Name;Score\nAlice;95\nBob;80\n", encoding="utf-8")
    result = csv_to_markdown(str(p), delimiter=None)
    assert "Name" in result
    assert "Alice" in result


# ---------------------------------------------------------------------------
# Error handling
# ---------------------------------------------------------------------------


def test_file_not_found_raises(tmp_path):
    with pytest.raises(FileNotFoundError):
        csv_to_markdown(str(tmp_path / "missing.csv"))


def test_wrong_extension_raises(tmp_path):
    p = tmp_path / "file.txt"
    p.write_text("a,b\n1,2\n", encoding="utf-8")
    with pytest.raises(ValueError):
        csv_to_markdown(str(p))


# ---------------------------------------------------------------------------
# Integration
# ---------------------------------------------------------------------------


def test_via_get_markdown(tmp_path):
    p = write_csv(tmp_path / "t.csv", [["Col1", "Col2"], ["val1", "val2"]])
    result = get_markdown(str(p))
    assert isinstance(result, str)
    assert len(result) > 0
    assert "|" in result


def test_row_count_matches(tmp_path):
    rows = [["H1", "H2"]] + [[str(i), str(i * 2)] for i in range(5)]
    p = write_csv(tmp_path / "t.csv", rows)
    result = csv_to_markdown(str(p))
    lines = result.splitlines()
    assert len(lines) == 5 + 2  # 5 data rows + header + separator


def test_unicode_values(tmp_path):
    p = write_csv(tmp_path / "t.csv", [["Word1", "Word2"], ["héllo", "wörld"]])
    result = csv_to_markdown(str(p))
    assert "héllo" in result
    assert "wörld" in result


def test_direct_import():
    from py_chunks.chunkers.csv import csv_to_markdown as _fn  # noqa: F401

    assert callable(_fn)
