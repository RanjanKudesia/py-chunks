"""Contract tests for .ipynb (Jupyter notebook) — JSON cell assembly + MD chunking."""

from pathlib import Path

import pytest

from py_chunks import get_chunks, get_chunks_from_bytes, get_markdown, stream_chunks
from py_chunks.chunkers.ipynb import chunk_ipynb
from _spreadsheet_fixtures import fixtures, require

IPYNB = fixtures("ipynb", "*.ipynb")
MODES = ["default", "section", "semantic", "sentence", "sliding_window", "page_aware"]
STANDARD_KEYS = {"content", "content_type", "metadata"}


def _pick(name):
    m = [f for f in IPYNB if f.name == name]
    require(m)
    return m[0]


def _doc_meta(chunks):
    return chunks[0]["metadata"]["document_metadata"] if chunks else {}


def test_all_fixtures_no_crash():
    """Every notebook chunks or fails cleanly — never a panic."""
    require(IPYNB)
    for f in IPYNB:
        try:
            chunks = get_chunks(str(f))
        except Exception:  # noqa: BLE001 — clean, catchable
            continue
        for c in chunks:
            assert STANDARD_KEYS.issubset(c.keys())


@pytest.mark.parametrize("mode", MODES)
def test_all_modes(mode):
    chunks, _ = chunk_ipynb(str(_pick("nbc_notebook2.ipynb")), mode=mode)
    assert chunks


def test_batch_stream_bytes_agree():
    for f in IPYNB:
        batch = get_chunks(str(f))
        streamed = list(stream_chunks(str(f)))
        from_bytes = get_chunks_from_bytes(f.read_bytes(), "x.ipynb")
        assert len(batch) == len(streamed) == len(from_bytes)


def test_code_cells_become_code_blocks():
    """Code cells render as fenced code blocks → `code_block` content type."""
    chunks = get_chunks(str(_pick("nbc_notebook2.ipynb")))
    assert any(c["content_type"] == "code_block" for c in chunks)


def test_markdown_and_code_interleaved_in_order():
    """A mixed notebook yields both prose and code chunks."""
    md = get_markdown(str(_pick("nbc_notebook2.ipynb")))
    assert "# NumPy and Matplotlib examples" in md   # a markdown cell
    assert "```python" in md                          # a code cell fence
    assert "import numpy" in md                        # code content
    # markdown heading precedes the first code fence (cell order preserved)
    assert md.index("NumPy and Matplotlib") < md.index("```python")


def test_outputs_included():
    """Code cell outputs (stream/result text) are surfaced."""
    md = get_markdown(str(_pick("nbc_helloworld.ipynb")))
    assert md.strip()  # HelloWorld prints output


def test_metadata_surfaced():
    chunks = get_chunks(str(_pick("nbc_notebook2.ipynb")))
    m = _doc_meta(chunks)
    assert m["source_type"] == "ipynb"
    assert m["language"] == "python"
    assert m["nbformat"].startswith("4")
    assert m["code_cell_count"] >= 1 and m["markdown_cell_count"] >= 1


def test_images_extracted():
    result = get_chunks(str(_pick("nbc_notebook2.ipynb")), list_images=True)
    assert len(result.images) >= 1
    assert any(c["content_type"] == "image" for c in result.chunks)


def test_legacy_heading_and_malformed_output_no_crash():
    """The adversarial notebook (legacy `heading` cells + a malformed
    `"bad stream"` output) must chunk without crashing."""
    m = [f for f in IPYNB if f.name == "nbf_invalid.ipynb"]
    require(m)
    chunks = get_chunks(str(m[0]))  # must not panic/raise
    assert isinstance(chunks, list) and chunks


def test_image_only_notebook_surfaces_its_image_references():
    """A notebook of only image references must not come back empty.

    This asserted `chunks == []` — the behaviour tracked as defect #42. The
    notebook's entire content is three image references (one Markdown, two raw
    <img>), and returning nothing for a valid file is total content loss, not a
    graceful empty case. Images now leave the same `[Image]` placeholder that
    docx and pptx already emit.
    """
    m = [f for f in IPYNB if f.name == "nbc_embed_images.ipynb"]
    require(m)
    chunks = get_chunks(str(m[0]))
    assert chunks, "an image-only notebook must still yield a chunk"
    text = "\n".join(c["content"] for c in chunks)
    assert text.count("[Image") == 3, f"expected all 3 references, got: {text!r}"


def test_non_json_raises_clean_error(tmp_path):
    fake = tmp_path / "fake.ipynb"
    fake.write_bytes(b"this is not json {")
    with pytest.raises(Exception):  # noqa: B017 — catchable, not a panic
        get_chunks(str(fake))
