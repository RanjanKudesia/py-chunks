"""Tests for TXT and MD → Markdown via get_markdown / direct wrappers."""

import pytest
from pathlib import Path

from py_chunks import get_markdown
from py_chunks.chunkers.txt import txt_to_markdown
from py_chunks.chunkers.md import md_to_markdown


# ── fixtures ──────────────────────────────────────────────────────────────────

@pytest.fixture
def txt_file(tmp_path):
    p = tmp_path / "sample.txt"
    p.write_text("Hello world\n\nSecond paragraph.", encoding="utf-8")
    return p


@pytest.fixture
def md_file(tmp_path):
    p = tmp_path / "sample.md"
    p.write_text("# Heading\n\nSome **bold** text.\n\n- item one\n- item two", encoding="utf-8")
    return p


@pytest.fixture
def empty_txt(tmp_path):
    p = tmp_path / "empty.txt"
    p.write_text("", encoding="utf-8")
    return p


@pytest.fixture
def empty_md(tmp_path):
    p = tmp_path / "empty.md"
    p.write_text("", encoding="utf-8")
    return p


# ── txt_to_markdown ────────────────────────────────────────────────────────────

def test_txt_returns_string(txt_file):
    result = txt_to_markdown(str(txt_file))
    assert isinstance(result, str)


def test_txt_content_preserved(txt_file):
    result = txt_to_markdown(str(txt_file))
    assert "Hello world" in result
    assert "Second paragraph" in result


def test_txt_content_exact(txt_file):
    result = txt_to_markdown(str(txt_file))
    assert result == "Hello world\n\nSecond paragraph."


def test_txt_empty_file(empty_txt):
    result = txt_to_markdown(str(empty_txt))
    assert result == ""


def test_txt_file_not_found_raises():
    with pytest.raises(FileNotFoundError):
        txt_to_markdown("/nonexistent/path/file.txt")


def test_txt_wrong_extension_raises(tmp_path):
    p = tmp_path / "file.docx"
    p.write_text("content")
    with pytest.raises(ValueError):
        txt_to_markdown(str(p))


def test_txt_via_get_markdown(txt_file):
    result = get_markdown(str(txt_file))
    assert "Hello world" in result


def test_txt_multiline_preserved(tmp_path):
    p = tmp_path / "multi.txt"
    content = "Line 1\nLine 2\nLine 3"
    p.write_text(content, encoding="utf-8")
    assert txt_to_markdown(str(p)) == content


def test_txt_unicode_preserved(tmp_path):
    p = tmp_path / "unicode.txt"
    p.write_text("Héllo wörld — café", encoding="utf-8")
    assert "Héllo wörld" in txt_to_markdown(str(p))


# ── md_to_markdown ─────────────────────────────────────────────────────────────

def test_md_returns_string(md_file):
    result = md_to_markdown(str(md_file))
    assert isinstance(result, str)


def test_md_content_preserved(md_file):
    result = md_to_markdown(str(md_file))
    assert "# Heading" in result
    assert "**bold**" in result
    assert "- item one" in result


def test_md_content_exact(md_file):
    result = md_to_markdown(str(md_file))
    assert result == "# Heading\n\nSome **bold** text.\n\n- item one\n- item two"


def test_md_empty_file(empty_md):
    result = md_to_markdown(str(empty_md))
    assert result == ""


def test_md_file_not_found_raises():
    with pytest.raises(FileNotFoundError):
        md_to_markdown("/nonexistent/path/file.md")


def test_md_wrong_extension_raises(tmp_path):
    p = tmp_path / "file.txt"
    p.write_text("content")
    with pytest.raises(ValueError):
        md_to_markdown(str(p))


def test_md_via_get_markdown(md_file):
    result = get_markdown(str(md_file))
    assert "# Heading" in result
    assert "**bold**" in result


def test_md_unicode_preserved(tmp_path):
    p = tmp_path / "unicode.md"
    p.write_text("# Títle\n\nContéñt", encoding="utf-8")
    result = md_to_markdown(str(p))
    assert "# Títle" in result
    assert "Contéñt" in result
