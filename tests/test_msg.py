"""Contract tests for Outlook .msg (MS-OXMSG) — MAPI envelope + body + attachments."""

from pathlib import Path

import pytest

from py_chunks import get_chunks, get_chunks_from_bytes, get_markdown, stream_chunks
from py_chunks.chunkers.msg import chunk_msg
from _spreadsheet_fixtures import fixtures, require

MSG = fixtures("msg", "*.msg")
MODES = ["default", "section", "semantic", "sentence", "sliding_window", "page_aware"]
STANDARD_KEYS = {"content", "content_type", "metadata"}


def _pick(name):
    m = [f for f in MSG if f.name == name]
    require(m)
    return m[0]


def _doc_meta(chunks):
    return chunks[0]["metadata"]["document_metadata"] if chunks else {}


def test_all_fixtures_extract():
    require(MSG)
    for f in MSG:
        chunks = get_chunks(str(f))
        assert chunks, f"no chunks for {f.name}"
        for c in chunks:
            assert STANDARD_KEYS.issubset(c.keys())


@pytest.mark.parametrize("mode", MODES)
def test_all_modes(mode):
    chunks, _ = chunk_msg(str(_pick("tika_test-outlook2003.msg")), mode=mode)
    assert chunks


def test_batch_stream_bytes_agree():
    for f in MSG:
        batch = get_chunks(str(f))
        streamed = list(stream_chunks(str(f)))
        from_bytes = get_chunks_from_bytes(f.read_bytes(), "x.msg")
        assert len(batch) == len(streamed) == len(from_bytes)


@pytest.mark.parametrize("name,expect_class", [
    ("tika_testMSG_Appointment.msg", "IPM.Appointment"),
    ("tika_testMSG_Contact.msg", "IPM.Contact"),
    ("tika_testMSG_Task.msg", "IPM.Task"),
    ("tika_testMSG_StickyNote.msg", "IPM.StickyNote"),
    ("tika_testMSG_Post.msg", "IPM.Post"),
])
def test_message_class_detected(name, expect_class):
    chunks = get_chunks(str(_pick(name)))
    assert _doc_meta(chunks).get("message_class") == expect_class


@pytest.mark.parametrize("name,needle", [
    ("poi_ASCII_UTF-8_CP1252_LCID1031.msg", "öäü"),      # cp1252
    ("poi_ASCII_CP1251_LCID1049.msg", "автоматически"),   # cp1251 (Cyrillic)
    ("tika_testMSG_chinese.msg", "格式測試"),              # Big5 (CJK)
])
def test_encodings_decoded_correctly(name, needle):
    md = get_markdown(str(_pick(name)))
    assert needle in md, f"encoding lost in {name}: {md[:80]!r}"


def test_envelope_metadata_present():
    """Subject, from, recipients, and dates surface in document_metadata."""
    chunks = get_chunks(str(_pick("tika_test-outlook2003.msg")))
    m = _doc_meta(chunks)
    assert m["source_type"] == "msg"
    assert m["subject"] == "Welcome to Microsoft Office Outlook 2003"
    assert "@" in (m["from"] or "")
    assert isinstance(m["to"], list) and m["to"]
    # ISO-8601 date derived from the FILETIME property.
    assert m["sent_date"] and m["sent_date"].startswith("20")


def test_recipient_table_resolved():
    """Real To recipients (with SMTP emails) come from the recipient table."""
    chunks = get_chunks(str(_pick("tika_testMSG_Appointment.msg")))
    md = get_markdown(str(_pick("tika_testMSG_Appointment.msg")))
    assert "tallison@mitre.org" in md  # recipient SMTP address resolved


def test_embedded_message_attachment_inlined():
    """An embedded .msg attachment is recursively parsed and its text inlined."""
    path = _pick("poi_attachment_msg_pdf.msg")
    md = get_markdown(str(path))
    chunks = get_chunks(str(path))
    assert "Attachments" in md
    assert _doc_meta(chunks).get("has_attachments") is True
    # the embedded message's body is inlined
    assert "Blah blah blah" in md


def test_appointment_date_is_sane():
    """Regression: FILETIME→ISO must produce a real date (was year 0795)."""
    md = get_markdown(str(_pick("tika_testMSG_Appointment.msg")))
    sent = [l for l in md.splitlines() if l.startswith("**Sent:**")]
    assert sent and "2017-" in sent[0]


def test_appointment_item_fields():
    """Named-property resolution surfaces appointment start/end/location."""
    md = get_markdown(str(_pick("tika_testMSG_Appointment.msg")))
    assert "**Start:**" in md and "**End:**" in md
    assert "**Location:** under lazy dog" in md


def test_task_item_fields():
    """Task due date, status, and % complete via named properties."""
    md = get_markdown(str(_pick("tika_testMSG_Task.msg")))
    assert "**Due:**" in md
    assert "**Status:** In Progress" in md
    assert "**% Complete:**" in md


def test_contact_item_fields():
    """Contact card fields (company/title/phones) from standard 0x3A** props."""
    md = get_markdown(str(_pick("tika_testMSG_Contact.msg")))
    assert "**Company:** Fence Co" in md
    assert "**Job Title:** Jumper" in md
    assert "Phone:" in md


def test_markdown_has_subject_heading():
    md = get_markdown(str(_pick("tika_testMSG.msg")))
    assert md.startswith("# ")


def test_non_msg_raises_clean_error(tmp_path):
    """A non-CFB file with a .msg name raises a catchable error, not a panic."""
    fake = tmp_path / "fake.msg"
    fake.write_bytes(b"not a compound file")
    with pytest.raises(Exception):  # noqa: B017
        get_chunks(str(fake))
