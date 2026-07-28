"""Contract + quality tests for `.jsonl` / `.ndjson`, driven off real fixtures
(elastic, OpenAI, Spark, HuggingFace, ndjson corpora)."""

from pathlib import Path

import pytest

from py_chunks import get_chunks, get_chunks_from_bytes, get_markdown, stream_chunks
from py_chunks.chunkers.json import chunk_json

_TEST_FILES = Path(__file__).resolve().parents[2] / "test_files"
JSONL_DIR = _TEST_FILES / "jsonl"

STANDARD_KEYS = {"content", "content_type", "metadata"}


def jsonl_files() -> list[Path]:
    if not JSONL_DIR.is_dir():
        return []
    return sorted(list(JSONL_DIR.glob("*.jsonl")) + list(JSONL_DIR.glob("*.ndjson")))


def require(files) -> None:
    if not files:
        pytest.skip("no .jsonl/.ndjson fixtures present")


def find(name: str) -> Path:
    match = [f for f in jsonl_files() if f.name == name]
    require(match)
    return match[0]


def test_no_crash_contract():
    require(jsonl_files())
    for f in jsonl_files():
        chunks = get_chunks(str(f))
        assert isinstance(chunks, list) and chunks
        for c in chunks:
            assert STANDARD_KEYS.issubset(c.keys())


def test_batch_stream_bytes_agree():
    require(jsonl_files())
    for f in jsonl_files():
        b = get_chunks(str(f))
        s = list(stream_chunks(str(f)))
        y = get_chunks_from_bytes(f.read_bytes(), f.name)
        assert len(b) == len(s) == len(y), f"{f.name}: paths diverge"


def test_record_count_matches_lines():
    f = find("spark_employees.jsonl")
    c = get_chunks(str(f))
    meta = c[0]["metadata"]["document_metadata"]
    assert meta["source_type"] == "jsonl"
    assert meta["top_level"] == "lines"
    # 4 records → 4 chunks (one per line).
    assert meta["record_count"] == 4
    assert len(c) == 4


def test_ndjson_source_type():
    f = find("zino_logs.ndjson")
    meta = get_chunks(str(f))[0]["metadata"]["document_metadata"]
    assert meta["source_type"] == "ndjson"


def test_blank_lines_skipped():
    """A file with blank lines between records counts only real records."""
    f = find("extrap_blanklines.jsonl")
    meta = get_chunks(str(f))[0]["metadata"]["document_metadata"]
    assert meta["record_count"] == 125


def test_cjk_records_decoded():
    import re

    f = find("more_dynvqa_cjk.jsonl")
    md = get_markdown(str(f))
    assert re.search(r"[一-鿿]", md), "CJK records not decoded"


def test_large_stream_parses():
    """The 51k-line nginx log stream parses to one chunk per line."""
    f = find("elastic_nginx_json_logs.jsonl")
    meta = get_chunks(str(f))[0]["metadata"]["document_metadata"]
    assert meta["record_count"] > 50000
