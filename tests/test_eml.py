"""Contract + quality tests for `.eml` (MIME email), driven off real fixtures
downloaded from Apache Tika / MimeKit / stalwart mail-parser / cpython corpora."""

from pathlib import Path

import pytest

from py_chunks import get_chunks, get_chunks_from_bytes, get_markdown, stream_chunks
from py_chunks.chunkers.eml import chunk_eml

_TEST_FILES = Path(__file__).resolve().parents[2] / "test_files"
EML_DIR = _TEST_FILES / "eml"

EML_MODES = ["default", "section", "semantic", "sentence", "sliding_window", "page_aware"]
STANDARD_KEYS = {"content", "content_type", "metadata"}


def eml_files() -> list[Path]:
    if not EML_DIR.is_dir():
        return []
    return sorted(EML_DIR.glob("*.eml"))


def require(files) -> None:
    if not files:
        pytest.skip("no .eml fixtures present")


def find(name: str) -> Path:
    match = [f for f in eml_files() if f.name == name]
    require(match)
    return match[0]


def assert_schema(chunks) -> None:
    for c in chunks:
        assert STANDARD_KEYS.issubset(c.keys())
        assert isinstance(c["content"], str)


def test_no_crash_contract():
    """Every .eml must return chunks or a *catchable* error — never a panic."""
    require(eml_files())
    parsed = 0
    for f in eml_files():
        try:
            get_chunks(str(f))
            parsed += 1
        except (RuntimeError, ValueError):
            pass
    assert parsed >= 10, f"expected most .eml fixtures to parse, got {parsed}"


@pytest.mark.parametrize("mode", EML_MODES)
def test_all_modes(mode):
    f = find("tika_testRFC822-multipart.eml")
    chunks, timing = chunk_eml(str(f), mode=mode)
    assert isinstance(chunks, list)
    assert_schema(chunks)
    assert "rust_ms" in timing and "python_ms" in timing


def test_batch_stream_bytes_agree():
    for f in eml_files():
        b = get_chunks(str(f))
        s = list(stream_chunks(str(f)))
        y = get_chunks_from_bytes(f.read_bytes(), f.name)
        assert len(b) == len(s) == len(y), f"{f.name}: path chunk counts diverge"


def test_markdown_nonempty_for_real_messages():
    f = find("tika_testRFC822-multipart.eml")
    md = get_markdown(str(f))
    assert isinstance(md, str) and md.strip()


def test_rfc2047_and_iso2022jp_decoded():
    """Encoded-word headers and a stateful ISO-2022-JP body must decode to real
    Unicode, not raw escape sequences."""
    f = find("mimekit_japanese.eml")
    md = get_markdown(str(f))
    assert "日本語メールテスト" in md, "RFC2047/ISO-2022-JP subject not decoded"
    assert "\x1b$B" not in md, "raw ISO-2022-JP escape leaked into output"
    meta = get_chunks(str(f))[0]["metadata"]["document_metadata"]
    assert meta["subject"].startswith("日本語メールテスト")


def test_iso8859_1_html_body_decoded():
    f = find("stalwart_legacy_002_html-only.eml")
    md = get_markdown(str(f))
    assert "Frösche" in md, "iso-8859-1 HTML body not decoded"


def test_attachment_listed_and_image_extracted():
    f = find("tika_testEmailWithPNGAtt.eml")
    md = get_markdown(str(f))
    assert "## Attachments" in md
    assert "testPNG.png" in md
    result = get_chunks(str(f), list_images=True)
    assert "testPNG.png" in result.images
    assert isinstance(result.images["testPNG.png"], bytes)
    assert len(result.images["testPNG.png"]) > 0


def test_metadata_shape():
    f = find("tika_testRFC822-multipart.eml")
    meta = get_chunks(str(f))[0]["metadata"]["document_metadata"]
    assert meta["source_type"] == "eml"
    for key in ("subject", "from", "to", "cc", "bcc", "date", "attachment_count"):
        assert key in meta


def test_empty_message_returns_no_chunks():
    """An adversarial message with empty MIME parts and no headers yields zero
    chunks rather than raising — consistent across all paths."""
    f = find("stalwart_malformed_000.eml")
    assert get_chunks(str(f)) == []
    assert list(stream_chunks(str(f))) == []


def test_wrong_extension_raises(tmp_path):
    p = tmp_path / "file.txt"
    p.write_text("Subject: x\n\nbody", encoding="utf-8")
    with pytest.raises(ValueError):
        chunk_eml(str(p))


def test_missing_file_raises():
    with pytest.raises(FileNotFoundError):
        chunk_eml("/nonexistent/message.eml")
