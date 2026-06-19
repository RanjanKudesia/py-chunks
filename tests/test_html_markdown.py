import pytest
from pathlib import Path
from py_chunks import get_markdown
from py_chunks.chunkers.html import html_to_markdown


def _write_html(tmp_path, name, content):
    path = tmp_path / name
    path.write_text(content, encoding="utf-8")
    return path


# -- Headings -----------------------------------------------------------------

def test_h1_becomes_h1(tmp_path):
    path = _write_html(tmp_path, "t.html", "<h1>Title</h1>")
    result = html_to_markdown(str(path))
    assert result.startswith("# Title")


def test_h2_becomes_h2(tmp_path):
    path = _write_html(tmp_path, "t.html", "<h2>Sub</h2>")
    result = html_to_markdown(str(path))
    assert "## Sub" in result


def test_h3_through_h6(tmp_path):
    html = "<h3>A</h3><h4>B</h4><h5>C</h5><h6>D</h6>"
    path = _write_html(tmp_path, "t.html", html)
    result = html_to_markdown(str(path))
    assert "### A" in result
    assert "#### B" in result
    assert "##### C" in result
    assert "###### D" in result


# -- Paragraphs ---------------------------------------------------------------

def test_paragraph_text_included(tmp_path):
    path = _write_html(tmp_path, "t.html", "<p>Hello world.</p>")
    result = html_to_markdown(str(path))
    assert "Hello world." in result


def test_multiple_paragraphs_separated(tmp_path):
    path = _write_html(tmp_path, "t.html", "<p>First.</p><p>Second.</p>")
    result = html_to_markdown(str(path))
    assert "First." in result
    assert "Second." in result
    assert result.index("First.") < result.index("Second.")


# -- Lists --------------------------------------------------------------------

def test_unordered_list_dash_prefix(tmp_path):
    html = "<ul><li>Apple</li><li>Banana</li></ul>"
    path = _write_html(tmp_path, "t.html", html)
    result = html_to_markdown(str(path))
    assert "- Apple" in result
    assert "- Banana" in result


def test_ordered_list_numbered(tmp_path):
    html = "<ol><li>Step one</li><li>Step two</li></ol>"
    path = _write_html(tmp_path, "t.html", html)
    result = html_to_markdown(str(path))
    assert "1. Step one" in result
    assert "2. Step two" in result


# -- Code blocks --------------------------------------------------------------

def test_pre_becomes_fenced_code(tmp_path):
    html = "<pre>def hello():\n    pass</pre>"
    path = _write_html(tmp_path, "t.html", html)
    result = html_to_markdown(str(path))
    assert "```" in result
    assert "def hello()" in result


# -- Tables -------------------------------------------------------------------

def test_table_pipe_format(tmp_path):
    html = "<table><tr><th>Name</th><th>Age</th></tr><tr><td>Alice</td><td>30</td></tr></table>"
    path = _write_html(tmp_path, "t.html", html)
    result = html_to_markdown(str(path))
    assert "|" in result
    assert "Name" in result
    assert "Alice" in result


def test_table_header_separator(tmp_path):
    html = "<table><tr><th>A</th><th>B</th></tr><tr><td>1</td><td>2</td></tr></table>"
    path = _write_html(tmp_path, "t.html", html)
    result = html_to_markdown(str(path))
    assert "---" in result


# -- Noise filtering ----------------------------------------------------------

def test_script_tags_excluded(tmp_path):
    html = "<p>Visible</p><script>alert('xss')</script>"
    path = _write_html(tmp_path, "t.html", html)
    result = html_to_markdown(str(path))
    assert "alert" not in result
    assert "Visible" in result


def test_html_comments_excluded(tmp_path):
    html = "<p>Content</p><!-- This is a comment -->"
    path = _write_html(tmp_path, "t.html", html)
    result = html_to_markdown(str(path))
    assert "comment" not in result
    assert "Content" in result


def test_html_entities_decoded(tmp_path):
    html = "<p>AT&amp;T &lt;telecom&gt;</p>"
    path = _write_html(tmp_path, "t.html", html)
    result = html_to_markdown(str(path))
    assert "AT&T" in result


# -- Full document ------------------------------------------------------------

def test_full_document_structure(tmp_path):
    html = """
    <html><head><title>Test</title></head>
    <body>
      <h1>Main Title</h1>
      <p>Introduction paragraph.</p>
      <h2>Section</h2>
      <ul><li>Item A</li><li>Item B</li></ul>
    </body></html>
    """
    path = _write_html(tmp_path, "t.html", html)
    result = html_to_markdown(str(path))
    assert "# Main Title" in result
    assert "## Section" in result
    assert "Introduction paragraph." in result
    assert "- Item A" in result


def test_no_excess_blank_lines(tmp_path):
    html = "<p>A</p><p>B</p><p>C</p>"
    path = _write_html(tmp_path, "t.html", html)
    result = html_to_markdown(str(path))
    assert "\n\n\n" not in result


# -- .htm extension -----------------------------------------------------------

def test_htm_extension_accepted(tmp_path):
    path = _write_html(tmp_path, "t.htm", "<h1>HTM File</h1>")
    result = html_to_markdown(str(path))
    assert "HTM File" in result


# -- Error cases --------------------------------------------------------------

def test_missing_file_raises(tmp_path):
    with pytest.raises((FileNotFoundError, Exception)):
        html_to_markdown(str(tmp_path / "nope.html"))


def test_wrong_extension_raises(tmp_path):
    path = tmp_path / "f.txt"
    path.write_text("hello")
    with pytest.raises((ValueError, Exception)):
        html_to_markdown(str(path))


# -- get_markdown routing -----------------------------------------------------

def test_get_markdown_routes_html(tmp_path):
    path = _write_html(tmp_path, "t.html", "<h1>Hello</h1><p>World</p>")
    result = get_markdown(str(path))
    assert "Hello" in result
    assert "World" in result


def test_get_markdown_routes_htm(tmp_path):
    path = _write_html(tmp_path, "t.htm", "<p>HTM content</p>")
    result = get_markdown(str(path))
    assert "HTM content" in result
