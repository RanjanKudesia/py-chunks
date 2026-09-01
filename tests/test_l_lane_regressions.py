"""Regressions for the L-lane defects fixed 2026-08-16 (L4, L5, L6, L14).

Each of these was a *silent* data loss — the call succeeded and returned
plausible output with something missing — which is the hardest kind to notice.
They are grouped here because they share that shape, not because they share code.
"""

from __future__ import annotations

import re
import zipfile
from pathlib import Path

import pytest

from py_chunks import get_chunks, get_markdown

TEST_FILES = Path(__file__).resolve().parents[2] / "test_files"
ADDR = re.compile(r"\b[A-Za-z0-9._%+-]{2,}@[A-Za-z0-9.-]+\.[A-Za-z]{2,10}\b")


def chunks_of(path, **kw):
    r = get_chunks(str(path), **kw)
    if hasattr(r, "chunks"):
        return r.chunks
    return r[0] if isinstance(r, tuple) else r


def joined(path, **kw):
    return " ".join(c["content"] for c in chunks_of(path, **kw))


@pytest.mark.skipif(not TEST_FILES.is_dir(), reason="fixture corpus absent")
class TestL4EmailAddressesSurvive:
    """`<addr@host>` is a CommonMark *autolink*, not raw inline HTML.

    Treating it as a tag deleted every angle-bracketed address from chunk
    content: 34 across `.eml`, 210 across `.mbox`, 10 across `.msg`.
    `get_markdown` was always correct — only `get_chunks` lost them.
    """

    @pytest.mark.parametrize("subdir,ext", [("eml", "*.eml"), ("mbox", "*.mbox"), ("msg", "*.msg")])
    def test_addresses_in_markdown_also_reach_the_chunks(self, subdir, ext):
        files = sorted((TEST_FILES / subdir).glob(ext))
        if not files:
            pytest.skip(f"no {subdir} fixtures")
        total = kept = 0
        for f in files:
            try:
                md, ch = get_markdown(str(f)), joined(f)
            except Exception:  # noqa: BLE001 — parse failures are other tests' business
                continue
            in_md = set(ADDR.findall(md))
            total += len(in_md)
            kept += len(in_md & set(ADDR.findall(ch)))
        assert total, f"no addresses found in {subdir} markdown — test is vacuous"
        # Not 100%: one address lives inside an HTML body part where the angle
        # brackets really are markup. Retention was ~0% for these before.
        assert kept / total >= 0.95, f"{subdir}: only {kept}/{total} addresses survived"

    def test_the_headline_fixture(self):
        f = TEST_FILES / "eml" / "cpython_msg_01_plain.eml"
        if not f.exists():
            pytest.skip("fixture missing")
        assert "bbb@ddd.com" in joined(f)


class TestL4RawHtmlStillStripped:
    """The other half: `.md` must keep its CommonMark behaviour."""

    def test_tags_go_but_their_text_stays(self, tmp_path):
        p = tmp_path / "t.md"
        p.write_text('A <span class="x">tagged</span> word.\n', encoding="utf-8")
        out = joined(p)
        assert "<span" not in out
        assert "tagged" in out

    def test_autolinks_stay(self, tmp_path):
        p = tmp_path / "a.md"
        p.write_text("Mail <me@example.com> or see <https://example.com>.\n", encoding="utf-8")
        out = joined(p)
        assert "me@example.com" in out
        assert "https://example.com" in out


@pytest.mark.skipif(not TEST_FILES.is_dir(), reason="fixture corpus absent")
class TestL5ImagesDoNotEatProse:
    """`list_images=True` must not change the text chunks.

    Images anchored outside any heading lost their element text entirely,
    because the outside-a-section branch emitted the image chunk and dropped the
    prose the in-a-section branch preserved.
    """

    FIXTURES = [
        "docx/corpus_floating_image.docx",
        "docx/poi_chartex.docx",
        "docm/oxml_00_Comment061.docm",
        "dotx/oxml_00_Comment064.dotx",
        "dotm/oxml_00_Comment063.dotm",
    ]

    @staticmethod
    def _prose(path, list_images):
        out = []
        for c in chunks_of(path, list_images=list_images):
            # Real image chunks carry metadata.image_name and their content is
            # the hash filename — exclude those, keep all prose.
            if list_images and (c.get("metadata") or {}).get("image_name"):
                continue
            out.append(c["content"])
        return "".join(out)

    @pytest.mark.parametrize("rel", FIXTURES)
    def test_text_is_identical_with_and_without_images(self, rel):
        f = TEST_FILES / rel
        if not f.exists():
            pytest.skip(f"fixture missing: {rel}")
        assert self._prose(f, False) == self._prose(f, True)


class TestL6EntitiesAreNotSpaced:
    """A `&amp;` splits one `<a:t>` into several XML events.

    Space-joining them turned `AT&amp;T` into `AT & T` in `get_markdown` while
    `get_chunks` — which uses a different walker — was correct all along.
    """

    @staticmethod
    def _deck(path: Path) -> Path:
        slide = (
            '<?xml version="1.0"?><p:sld '
            'xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" '
            'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">'
            "<p:cSld><p:spTree><p:sp><p:txBody><a:bodyPr/><a:p><a:r>"
            "<a:t>AT&amp;T merger</a:t></a:r></a:p></p:txBody></p:sp>"
            "</p:spTree></p:cSld></p:sld>"
        )
        pres = (
            '<?xml version="1.0"?><p:presentation '
            'xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" '
            'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">'
            '<p:sldIdLst><p:sldId id="256" r:id="rId1"/></p:sldIdLst></p:presentation>'
        )
        ct = (
            '<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
            '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
            '<Default Extension="xml" ContentType="application/xml"/>'
            '<Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/>'
            '<Override PartName="/ppt/slides/slide1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/></Types>'
        )
        rels = (
            '<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
            '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/></Relationships>'
        )
        prels = (
            '<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
            '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>'
        )
        with zipfile.ZipFile(path, "w") as z:
            z.writestr("[Content_Types].xml", ct)
            z.writestr("_rels/.rels", rels)
            z.writestr("ppt/presentation.xml", pres)
            z.writestr("ppt/_rels/presentation.xml.rels", prels)
            z.writestr("ppt/slides/slide1.xml", slide)
        return path

    def test_markdown_and_chunks_agree_on_the_entity(self, tmp_path):
        deck = self._deck(tmp_path / "entity.pptx")
        md = get_markdown(str(deck))
        assert "AT&T" in md, f"entity was spaced apart: {md!r}"
        assert "AT & T" not in md
        assert "AT&T" in joined(deck)


@pytest.mark.skipif(not TEST_FILES.is_dir(), reason="fixture corpus absent")
class TestL14EpubStillChunks:
    """epub stopped swallowing per-chapter failures, so an empty chapter must
    still be *empty* rather than an error — otherwise a legitimate image-only
    cover page would abort the whole book."""

    def test_every_epub_fixture_still_chunks(self):
        files = sorted((TEST_FILES / "epub").glob("*.epub"))
        if not files:
            pytest.skip("no epub fixtures")
        failures = []
        for f in files:
            try:
                chunks_of(f)
            except Exception as e:  # noqa: BLE001
                failures.append(f"{f.name}: {type(e).__name__}: {e}")
        assert not failures, "epub regressions:\n" + "\n".join(failures)
