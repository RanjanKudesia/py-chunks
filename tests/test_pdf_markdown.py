import pytest

reportlab = pytest.importorskip("reportlab", reason="reportlab required for PDF fixtures")

from pathlib import Path
from reportlab.lib.pagesizes import letter
from reportlab.pdfgen import canvas

from py_chunks import get_markdown
from py_chunks.chunkers.pdf import pdf_to_markdown


def make_pdf(tmp_path, name, draw_fn):
    """Helper: create a PDF at tmp_path/name, call draw_fn(c, width, height)."""
    p = tmp_path / name
    c = canvas.Canvas(str(p), pagesize=letter)
    w, h = letter
    draw_fn(c, w, h)
    c.save()
    return p


def test_returns_string(tmp_path):
    path = make_pdf(tmp_path, "one.pdf", lambda c, w, h: c.drawString(72, h - 72, "Hello PDF"))
    result = get_markdown(str(path))
    assert isinstance(result, str)
    assert result.strip()


def test_heading_uses_hash(tmp_path):
    def draw(c, w, h):
        c.setFont("Helvetica-Bold", 36)
        c.drawString(72, h - 72, "Big Heading")
        c.setFont("Helvetica", 12)
        c.drawString(72, h - 110, "Body paragraph text")

    path = make_pdf(tmp_path, "heading.pdf", draw)
    result = get_markdown(str(path))
    heading_line = next((l for l in result.splitlines() if "Big Heading" in l), "")
    assert heading_line.startswith("#")


def test_body_text_present(tmp_path):
    path = make_pdf(
        tmp_path,
        "body.pdf",
        lambda c, w, h: (
            c.setFont("Helvetica", 12),
            c.drawString(72, h - 72, "This is normal body text."),
        ),
    )
    result = get_markdown(str(path))
    assert "This is normal body text." in result
    body_line = next((l for l in result.splitlines() if "This is normal body text." in l), "")
    assert not body_line.startswith("#")


def test_multi_page_has_separator(tmp_path):
    def draw(c, w, h):
        c.drawString(72, h - 72, "Page one text")
        c.showPage()
        c.drawString(72, h - 72, "Page two text")

    path = make_pdf(tmp_path, "multi.pdf", draw)
    result = get_markdown(str(path))
    assert "Page one text" in result
    assert "Page two text" in result
    assert "---" in result


def test_single_page_no_separator(tmp_path):
    path = make_pdf(tmp_path, "single.pdf", lambda c, w, h: c.drawString(72, h - 72, "Only page"))
    result = get_markdown(str(path))
    assert "Only page" in result
    assert "---" not in result


def test_empty_pdf_returns_empty_string(tmp_path):
    def draw(c, w, h):
        c.showPage()

    path = make_pdf(tmp_path, "empty.pdf", draw)
    result = get_markdown(str(path))
    assert isinstance(result, str)
    assert len(result.strip()) <= 5


def test_file_not_found_raises():
    with pytest.raises((FileNotFoundError, Exception)):
        get_markdown("missing.pdf")


def test_wrong_extension_raises(tmp_path):
    path = tmp_path / "file.txt"
    path.write_text("not a pdf", encoding="utf-8")
    with pytest.raises((ValueError, Exception)):
        get_markdown(str(path))


def test_output_is_markdown_string(tmp_path):
    path = make_pdf(tmp_path, "md.pdf", lambda c, w, h: c.drawString(72, h - 72, "Markdown me"))
    result = get_markdown(str(path))
    assert type(result) is str


def test_heading_level_increases_with_larger_font(tmp_path):
    def draw(c, w, h):
        c.setFont("Helvetica-Bold", 48)
        c.drawString(72, h - 72, "Big Title")
        c.setFont("Helvetica-Bold", 30)
        c.drawString(72, h - 115, "Mid Title")
        c.setFont("Helvetica", 12)
        y = h - 155
        for i in range(8):
            c.drawString(72, y - (i * 16), f"Body line {i + 1}")

    path = make_pdf(tmp_path, "levels.pdf", draw)
    result = get_markdown(str(path))

    big = next((l for l in result.splitlines() if "Big Title" in l), "")
    mid = next((l for l in result.splitlines() if "Mid Title" in l), "")
    body = next((l for l in result.splitlines() if "Body line 1" in l), "")

    assert big.startswith("#")
    assert mid.startswith("##")
    assert not body.startswith("#")


def test_bullet_list_lines_present(tmp_path):
    def draw(c, w, h):
        c.setFont("Helvetica", 12)
        c.drawString(72, h - 72, "- Bullet one")
        c.drawString(72, h - 88, "- Bullet two")

    path = make_pdf(tmp_path, "bullets.pdf", draw)
    result = get_markdown(str(path))
    assert "Bullet one" in result
    assert "Bullet two" in result


def test_pdf_to_markdown_direct(tmp_path):
    path = make_pdf(tmp_path, "direct.pdf", lambda c, w, h: c.drawString(72, h - 72, "Direct call"))
    result = pdf_to_markdown(str(path))
    assert "Direct call" in result


def test_two_headings_same_level(tmp_path):
    def draw(c, w, h):
        c.setFont("Helvetica-Bold", 30)
        c.drawString(72, h - 72, "Heading A")
        c.drawString(72, h - 108, "Heading B")
        c.setFont("Helvetica", 12)
        c.drawString(72, h - 150, "Body")

    path = make_pdf(tmp_path, "same_level.pdf", draw)
    result = get_markdown(str(path))

    line_a = next((l for l in result.splitlines() if "Heading A" in l), "")
    line_b = next((l for l in result.splitlines() if "Heading B" in l), "")

    assert line_a.startswith("#")
    assert line_b.startswith("#")
    assert len(line_a) - len(line_a.lstrip("#")) == len(line_b) - len(line_b.lstrip("#"))


def test_content_not_empty_after_strip(tmp_path):
    path = make_pdf(tmp_path, "nonempty.pdf", lambda c, w, h: c.drawString(72, h - 72, "Not empty"))
    result = get_markdown(str(path))
    assert result.strip()
