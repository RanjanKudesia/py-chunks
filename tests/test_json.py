"""Contract + quality tests for `.json`, driven off real fixtures (nst/JSONTestSuite,
vega-datasets, CLDR, GeoJSON)."""

from pathlib import Path

import pytest

from py_chunks import get_chunks, get_chunks_from_bytes, get_markdown, stream_chunks
from py_chunks.chunkers.json import chunk_json

_TEST_FILES = Path(__file__).resolve().parents[2] / "test_files"
JSON_DIR = _TEST_FILES / "json"

JSON_MODES = ["default", "section", "semantic", "sentence", "sliding_window", "page_aware"]
STANDARD_KEYS = {"content", "content_type", "metadata"}
# Deliberately-invalid / implementation-defined fixtures (must fail cleanly).
INVALID = {
    "adversarial_not_json_readme.json",
    "nst_n_array_incomplete.json",
    "nst_n_object_trailing_comma.json",
    "nst_n_single_space.json",
    "nst_n_structure_unclosed_array.json",
    "nst_n_structure_100000_opening_arrays.json",
    "nst_i_number_huge_exp.json",
    "nst_i_structure_500_nested_arrays.json",
}


def json_files() -> list[Path]:
    if not JSON_DIR.is_dir():
        return []
    return sorted(JSON_DIR.glob("*.json"))


def good_files() -> list[Path]:
    return [f for f in json_files() if f.name not in INVALID]


def require(files) -> None:
    if not files:
        pytest.skip("no .json fixtures present")


def find(name: str) -> Path:
    match = [f for f in json_files() if f.name == name]
    require(match)
    return match[0]


@pytest.mark.parametrize("mode", JSON_MODES)
def test_all_modes(mode):
    f = find("vega_penguins.json")
    chunks, timing = chunk_json(str(f), mode=mode)
    assert isinstance(chunks, list) and chunks
    for c in chunks:
        assert STANDARD_KEYS.issubset(c.keys())
    assert "rust_ms" in timing


def test_batch_stream_bytes_agree():
    for f in good_files():
        b = get_chunks(str(f))
        s = list(stream_chunks(str(f)))
        y = get_chunks_from_bytes(f.read_bytes(), f.name)
        assert len(b) == len(s) == len(y), f"{f.name}: paths diverge"


def test_array_of_records_is_one_chunk_per_record():
    """The classic RAG case: each array element becomes exactly one chunk."""
    f = find("vega_penguins.json")
    c = get_chunks(str(f))
    meta = c[0]["metadata"]["document_metadata"]
    assert meta["source_type"] == "json"
    assert meta["top_level"] == "array"
    assert len(c) == meta["record_count"]
    # Record content carries the fields.
    assert "Island" in c[0]["content"]


def test_geojson_envelope_unwrapped():
    """A GeoJSON FeatureCollection unwraps its `features` array into records."""
    f = find("geojson_countries.json")
    meta = get_chunks(str(f))[0]["metadata"]["document_metadata"]
    assert meta["envelope_key"] == "features"
    assert meta["record_count"] > 1


def test_unicode_preserved():
    f = find("cldr_zh_languages.json")
    md = get_markdown(str(f))
    import re

    assert re.search(r"[一-鿿]", md), "CJK content not preserved"


def test_empty_array_is_zero_chunks():
    f = find("nst_y_array_empty.json")
    assert get_chunks(str(f)) == []


@pytest.mark.parametrize("name", sorted(INVALID))
def test_invalid_fails_cleanly_no_panic(name):
    """Every malformed / impl-defined fixture — including the 100k-deep recursion
    torture file — must raise a *catchable* error, never crash the interpreter."""
    match = [f for f in json_files() if f.name == name]
    require(match)
    with pytest.raises(Exception):  # noqa: B017
        get_chunks(str(match[0]))


def test_wrong_extension_raises(tmp_path):
    p = tmp_path / "file.txt"
    p.write_text("{}", encoding="utf-8")
    with pytest.raises(ValueError):
        chunk_json(str(p))
