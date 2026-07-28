"""Contract + quality tests for `.mbox` (mailbox archive), driven off real
fixtures (Apache Tika / MimeKit corpora)."""

from pathlib import Path

import pytest

from py_chunks import get_chunks, get_chunks_from_bytes, get_markdown, stream_chunks
from py_chunks.chunkers.eml import chunk_eml

_TEST_FILES = Path(__file__).resolve().parents[2] / "test_files"
MBOX_DIR = _TEST_FILES / "mbox"

STANDARD_KEYS = {"content", "content_type", "metadata"}


def mbox_files() -> list[Path]:
    if not MBOX_DIR.is_dir():
        return []
    return sorted(MBOX_DIR.glob("*.mbox"))


def require(files) -> None:
    if not files:
        pytest.skip("no .mbox fixtures present")


def find(name: str) -> Path:
    match = [f for f in mbox_files() if f.name == name]
    require(match)
    return match[0]


def test_no_crash_contract():
    require(mbox_files())
    for f in mbox_files():
        chunks = get_chunks(str(f))
        assert isinstance(chunks, list)
        for c in chunks:
            assert STANDARD_KEYS.issubset(c.keys())


def test_batch_stream_bytes_agree():
    require(mbox_files())
    for f in mbox_files():
        b = get_chunks(str(f))
        s = list(stream_chunks(str(f)))
        y = get_chunks_from_bytes(f.read_bytes(), f.name)
        assert len(b) == len(s) == len(y), f"{f.name}: path chunk counts diverge"


def test_multi_message_counts_and_sections():
    """A multi-message mailbox is one document with a section per message and a
    correct message_count in metadata."""
    f = find("tika_complex.mbox")
    md = get_markdown(str(f))
    assert "# Mailbox — 3 messages" in md
    assert "## Message 1" in md and "## Message 3" in md
    meta = get_chunks(str(f))[0]["metadata"]["document_metadata"]
    assert meta["source_type"] == "mbox"
    assert meta["message_count"] == 3


def test_unmunged_from_does_not_crash():
    """The 'unmunged' torture fixture (a body line starting with 'From ') must
    parse without crashing; perfect recovery is not expected."""
    f = find("mimekit_unmunged.mbox")
    chunks = get_chunks(str(f))
    assert isinstance(chunks, list) and chunks


def test_large_mailbox_parses():
    """The ~10 MB / 152-message adversarial mailbox parses to many messages."""
    match = [f for f in mbox_files() if f.name == "mimekit_jwz.mbox"]
    require(match)
    meta = get_chunks(str(match[0]))[0]["metadata"]["document_metadata"]
    assert meta["message_count"] > 100


def test_wrong_extension_raises(tmp_path):
    p = tmp_path / "file.txt"
    p.write_text("From x\n\nbody", encoding="utf-8")
    with pytest.raises(ValueError):
        chunk_eml(str(p))
