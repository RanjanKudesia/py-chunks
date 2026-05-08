use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};
use pyo3::wrap_pyfunction;
use pythonize::pythonize;
use quick_xml::escape::unescape;
use quick_xml::events::Event;
use quick_xml::name::QName;
use quick_xml::Reader;
use serde_json::{json, Value};
use std::fs;
use std::io::{BufReader, Cursor, Read};
use std::time::Instant;
use zip::ZipArchive;

const MAX_DOCX_AUX_XML_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContentType {
    PlainParagraph,
    HeadingSection,
    BulletNumberedList,
    Table,
    MixedContent,
    CodeBlock,
    FootnoteCaption,
    Image,
    LongSingleParagraph,
    ShortDisconnectedParagraph,
    HeaderFooter,
}

#[derive(Debug, Clone)]
struct DocumentElement {
    content_type: ContentType,
    text: String,
}

#[derive(Debug, Clone)]
struct DocParseResult {
    elements: Vec<DocumentElement>,
    doc_metadata: Value,
}

#[derive(Debug, Clone)]
struct ChunkRecordInput {
    content_type: ContentType,
    content: String,
    metadata: Value,
}

impl ContentType {
    fn as_str(self) -> &'static str {
        match self {
            ContentType::PlainParagraph => "plain_paragraph",
            ContentType::HeadingSection => "heading",
            ContentType::BulletNumberedList => "bullet_list",
            ContentType::Table => "table",
            ContentType::MixedContent => "mixed_content",
            ContentType::CodeBlock => "code_block",
            ContentType::FootnoteCaption => "footnote_caption",
            ContentType::Image => "image",
            ContentType::LongSingleParagraph => "long_single_paragraph",
            ContentType::ShortDisconnectedParagraph => "short_disconnected_paragraph",
            ContentType::HeaderFooter => "header_footer",
        }
    }
}

#[pyfunction]
fn chunk_docx(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    if !file_path.to_ascii_lowercase().ends_with(".docx") {
        return Err(PyValueError::new_err(format!(
            "Expected .docx file path, got: {file_path}"
        )));
    }

    // File I/O is not counted as chunking time - only parse + build is timed.
    let bytes = fs::read(file_path)
        .map_err(|e| PyIOError::new_err(format!("Failed to read DOCX file: {e}")))?;

    let rust_start = Instant::now();
    let parsed = parse_docx_document(&bytes)
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to parse DOCX: {e}")))?;
    let chunks_raw = build_chunks_from_elements(parsed.elements, &parsed.doc_metadata);
    let rust_ms = rust_start.elapsed().as_secs_f64() * 1000.0;

    let chunk_list: Vec<PyObject> = chunks_raw
        .into_iter()
        .map(|c| {
            let dict = PyDict::new_bound(py);
            dict.set_item("content", &c.content)?;
            dict.set_item("content_type", c.content_type.as_str())?;
            dict.set_item("metadata", pythonize(py, &c.metadata)?)?;
            Ok(dict.into_any().unbind())
        })
        .collect::<PyResult<_>>()?;

    let result = PyDict::new_bound(py);
    result.set_item("chunks", chunk_list)?;
    result.set_item("rust_ms", (rust_ms * 1000.0).round() / 1000.0)?;
    Ok(result.into_any().unbind())
}

fn parse_docx_document(bytes: &[u8]) -> Result<DocParseResult, String> {
    let cursor = Cursor::new(bytes);

    let mut archive =
        ZipArchive::new(cursor).map_err(|e| format!("DOCX is not a valid zip archive: {e}"))?;

    let mut document_xml_file = archive
        .by_name("word/document.xml")
        .map_err(|_| "word/document.xml not found in DOCX".to_string())?;

    let mut elements = parse_document_xml_elements_streaming(&mut document_xml_file)?;
    drop(document_xml_file);

    let footnotes_xml = read_zip_entry(&mut archive, "word/footnotes.xml", MAX_DOCX_AUX_XML_BYTES)?;
    let header_xml =
        read_first_prefixed_entry(&mut archive, "word/header", MAX_DOCX_AUX_XML_BYTES)?;
    let footer_xml =
        read_first_prefixed_entry(&mut archive, "word/footer", MAX_DOCX_AUX_XML_BYTES)?;
    let image_count = count_prefixed_entries(&mut archive, "word/media/")?;

    if let Some(footnote_xml) = footnotes_xml {
        for ft in extract_footnotes(&footnote_xml) {
            elements.push(DocumentElement {
                content_type: ContentType::FootnoteCaption,
                text: ft,
            });
        }
    }

    if let Some(header_text) = header_xml
        .as_ref()
        .and_then(|x| extract_text_from_xml(x).ok())
        .filter(|x| !x.trim().is_empty())
    {
        elements.push(DocumentElement {
            content_type: ContentType::HeaderFooter,
            text: header_text,
        });
    }

    if let Some(footer_text) = footer_xml
        .as_ref()
        .and_then(|x| extract_text_from_xml(x).ok())
        .filter(|x| !x.trim().is_empty())
    {
        elements.push(DocumentElement {
            content_type: ContentType::HeaderFooter,
            text: footer_text,
        });
    }

    let doc_metadata = json!({
        "header_text": header_xml.and_then(|x| extract_text_from_xml(&x).ok()),
        "footer_text": footer_xml.and_then(|x| extract_text_from_xml(&x).ok()),
        "image_count": image_count,
    });

    Ok(DocParseResult {
        elements,
        doc_metadata,
    })
}

fn qname_eq(name: QName<'_>, expected: &[u8]) -> bool {
    let n = name.as_ref();
    n == expected || n.rsplit(|b| *b == b':').next() == Some(expected)
}

fn push_text(target: &mut String, piece: &str) {
    let trimmed = piece.trim();
    if trimmed.is_empty() {
        return;
    }
    if !target.is_empty() {
        target.push(' ');
    }
    target.push_str(trimmed);
}

fn classify_paragraph_content(
    text: &str,
    style_val: Option<&str>,
    is_list: bool,
    has_drawing: bool,
) -> ContentType {
    let style_lc = style_val.map(|s| s.to_ascii_lowercase());
    let is_heading = style_lc
        .as_deref()
        .map(|s| s.starts_with("heading"))
        .unwrap_or(false);
    let is_caption = style_lc
        .as_deref()
        .map(|s| s.contains("caption"))
        .unwrap_or(false);
    let is_code = style_lc
        .as_deref()
        .map(|s| s.contains("code"))
        .unwrap_or(false)
        || text.contains("```");

    if is_heading {
        ContentType::HeadingSection
    } else if is_caption {
        ContentType::FootnoteCaption
    } else if is_list {
        ContentType::BulletNumberedList
    } else if is_code {
        ContentType::CodeBlock
    } else if has_drawing {
        ContentType::Image
    } else if text.len() > 500 {
        ContentType::LongSingleParagraph
    } else if text.len() < 80 {
        ContentType::ShortDisconnectedParagraph
    } else {
        ContentType::PlainParagraph
    }
}

fn parse_document_xml_elements_streaming<R: Read>(
    reader_src: R,
) -> Result<Vec<DocumentElement>, String> {
    let mut reader = Reader::from_reader(BufReader::new(reader_src));

    let mut buf = Vec::new();
    let mut elements = Vec::new();

    let mut in_table = false;
    let mut in_cell = false;
    let mut in_text = false;
    let mut in_paragraph = false;

    let mut para_text = String::new();
    let mut para_style: Option<String> = None;
    let mut para_is_list = false;
    let mut para_has_drawing = false;

    let mut table_rows: Vec<String> = Vec::new();
    let mut row_cells: Vec<String> = Vec::new();
    let mut cell_text = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let name = e.name();
                if qname_eq(name, b"tbl") {
                    in_table = true;
                    table_rows.clear();
                    row_cells.clear();
                    cell_text.clear();
                } else if qname_eq(name, b"tr") && in_table {
                    row_cells.clear();
                } else if qname_eq(name, b"tc") && in_table {
                    in_cell = true;
                    cell_text.clear();
                } else if qname_eq(name, b"p") {
                    if !in_table {
                        in_paragraph = true;
                        para_text.clear();
                        para_style = None;
                        para_is_list = false;
                        para_has_drawing = false;
                    }
                } else if qname_eq(name, b"numPr") && in_paragraph {
                    para_is_list = true;
                } else if qname_eq(name, b"drawing") && in_paragraph {
                    para_has_drawing = true;
                } else if qname_eq(name, b"pStyle") && in_paragraph {
                    for attr in e.attributes().flatten() {
                        if qname_eq(attr.key, b"val") {
                            let v = String::from_utf8_lossy(attr.value.as_ref()).to_string();
                            para_style = Some(v);
                            break;
                        }
                    }
                } else if qname_eq(name, b"t") {
                    in_text = true;
                }
            }
            Ok(Event::Empty(e)) => {
                let name = e.name();
                if qname_eq(name, b"numPr") && in_paragraph {
                    para_is_list = true;
                } else if qname_eq(name, b"drawing") && in_paragraph {
                    para_has_drawing = true;
                } else if qname_eq(name, b"pStyle") && in_paragraph {
                    for attr in e.attributes().flatten() {
                        if qname_eq(attr.key, b"val") {
                            let v = String::from_utf8_lossy(attr.value.as_ref()).to_string();
                            para_style = Some(v);
                            break;
                        }
                    }
                }
            }
            Ok(Event::End(e)) => {
                let name = e.name();
                if qname_eq(name, b"t") {
                    in_text = false;
                } else if qname_eq(name, b"tc") && in_table {
                    let cell = cell_text.trim().to_string();
                    if !cell.is_empty() {
                        row_cells.push(cell);
                    }
                    cell_text.clear();
                    in_cell = false;
                } else if qname_eq(name, b"tr") && in_table {
                    if !row_cells.is_empty() {
                        table_rows.push(row_cells.join(" | "));
                    }
                    row_cells.clear();
                } else if qname_eq(name, b"tbl") && in_table {
                    let table_text = table_rows.join("\n").trim().to_string();
                    if !table_text.is_empty() {
                        elements.push(DocumentElement {
                            content_type: ContentType::Table,
                            text: table_text,
                        });
                    }
                    in_table = false;
                    in_cell = false;
                    table_rows.clear();
                    row_cells.clear();
                    cell_text.clear();
                } else if qname_eq(name, b"p") && in_paragraph {
                    let text = para_text.trim().to_string();
                    let has_text = !text.is_empty();
                    let content_type = classify_paragraph_content(
                        &text,
                        para_style.as_deref(),
                        para_is_list,
                        para_has_drawing,
                    );

                    if has_text || matches!(content_type, ContentType::Image) {
                        let normalized = if has_text {
                            text
                        } else {
                            "[Image]".to_string()
                        };
                        elements.push(DocumentElement {
                            content_type,
                            text: normalized,
                        });
                    }

                    in_paragraph = false;
                    para_text.clear();
                    para_style = None;
                    para_is_list = false;
                    para_has_drawing = false;
                }
            }
            Ok(Event::Text(t)) => {
                if in_text {
                    let txt = match t.decode() {
                        Ok(v) => v.into_owned(),
                        Err(_) => String::new(),
                    };
                    if in_table && in_cell {
                        push_text(&mut cell_text, &txt);
                    } else if in_paragraph {
                        push_text(&mut para_text, &txt);
                    }
                }
            }
            Ok(Event::CData(t)) => {
                if in_text {
                    let txt = String::from_utf8_lossy(t.as_ref());
                    if in_table && in_cell {
                        push_text(&mut cell_text, &txt);
                    } else if in_paragraph {
                        push_text(&mut para_text, &txt);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("Failed to parse word/document.xml stream: {e}")),
            _ => {}
        }
        buf.clear();
    }

    Ok(elements)
}

fn build_chunks_from_elements(
    elements: Vec<DocumentElement>,
    doc_metadata: &Value,
) -> Vec<ChunkRecordInput> {
    let mut chunks = Vec::new();
    let mut section_heading: Option<String> = None;
    let mut section_parts: Vec<DocumentElement> = Vec::new();
    let mut outside_short_parts: Vec<String> = Vec::new();
    let mut pending_notes: Vec<String> = Vec::new();

    let mut i = 0usize;
    while i < elements.len() {
        let element = &elements[i];

        match element.content_type {
            ContentType::HeaderFooter => {}
            ContentType::FootnoteCaption => {
                pending_notes.push(element.text.clone());
            }
            ContentType::HeadingSection => {
                flush_outside_shorts(
                    &mut chunks,
                    &mut outside_short_parts,
                    &mut pending_notes,
                    doc_metadata,
                );
                flush_section(
                    &mut chunks,
                    &mut section_heading,
                    &mut section_parts,
                    &mut pending_notes,
                    doc_metadata,
                );
                section_heading = Some(element.text.clone());
                section_parts.push(element.clone());
            }
            ContentType::BulletNumberedList => {
                let mut bullets = vec![element.text.clone()];
                let mut j = i + 1;
                while j < elements.len()
                    && elements[j].content_type == ContentType::BulletNumberedList
                {
                    bullets.push(elements[j].text.clone());
                    j += 1;
                }
                let list_text = bullets.join("\n");

                if section_heading.is_some() {
                    section_parts.push(DocumentElement {
                        content_type: ContentType::BulletNumberedList,
                        text: list_text,
                    });
                } else {
                    flush_outside_shorts(
                        &mut chunks,
                        &mut outside_short_parts,
                        &mut pending_notes,
                        doc_metadata,
                    );
                    chunks.push(ChunkRecordInput {
                        content_type: ContentType::BulletNumberedList,
                        content: list_text,
                        metadata: base_chunk_metadata(None, &pending_notes, doc_metadata),
                    });
                    pending_notes.clear();
                }
                i = j - 1;
            }
            ContentType::Table => {
                flush_outside_shorts(
                    &mut chunks,
                    &mut outside_short_parts,
                    &mut pending_notes,
                    doc_metadata,
                );
                flush_section(
                    &mut chunks,
                    &mut section_heading,
                    &mut section_parts,
                    &mut pending_notes,
                    doc_metadata,
                );
                chunks.push(ChunkRecordInput {
                    content_type: ContentType::Table,
                    content: element.text.clone(),
                    metadata: base_chunk_metadata(None, &pending_notes, doc_metadata),
                });
                pending_notes.clear();
            }
            ContentType::CodeBlock => {
                flush_outside_shorts(
                    &mut chunks,
                    &mut outside_short_parts,
                    &mut pending_notes,
                    doc_metadata,
                );
                flush_section(
                    &mut chunks,
                    &mut section_heading,
                    &mut section_parts,
                    &mut pending_notes,
                    doc_metadata,
                );
                chunks.push(ChunkRecordInput {
                    content_type: ContentType::CodeBlock,
                    content: element.text.clone(),
                    metadata: base_chunk_metadata(None, &pending_notes, doc_metadata),
                });
                pending_notes.clear();
            }
            ContentType::ShortDisconnectedParagraph => {
                if section_heading.is_some() {
                    section_parts.push(element.clone());
                } else {
                    outside_short_parts.push(element.text.clone());
                }
            }
            ContentType::PlainParagraph | ContentType::LongSingleParagraph | ContentType::Image => {
                if section_heading.is_some() {
                    section_parts.push(element.clone());
                } else {
                    flush_outside_shorts(
                        &mut chunks,
                        &mut outside_short_parts,
                        &mut pending_notes,
                        doc_metadata,
                    );
                    let split = semantic_chunks(&element.text, 900);
                    for s in split {
                        chunks.push(ChunkRecordInput {
                            content_type: element.content_type,
                            content: s,
                            metadata: base_chunk_metadata(None, &pending_notes, doc_metadata),
                        });
                        pending_notes.clear();
                    }
                }
            }
            ContentType::MixedContent => {}
        }

        i += 1;
    }

    flush_outside_shorts(
        &mut chunks,
        &mut outside_short_parts,
        &mut pending_notes,
        doc_metadata,
    );
    flush_section(
        &mut chunks,
        &mut section_heading,
        &mut section_parts,
        &mut pending_notes,
        doc_metadata,
    );

    chunks
}

fn flush_outside_shorts(
    chunks: &mut Vec<ChunkRecordInput>,
    outside_short_parts: &mut Vec<String>,
    pending_notes: &mut Vec<String>,
    doc_metadata: &Value,
) {
    if outside_short_parts.is_empty() {
        return;
    }

    let merged = outside_short_parts.join(" ").trim().to_string();
    outside_short_parts.clear();
    if merged.is_empty() {
        return;
    }

    for item in recursive_char_chunks(&merged, 700, 100) {
        chunks.push(ChunkRecordInput {
            content_type: ContentType::ShortDisconnectedParagraph,
            content: item,
            metadata: base_chunk_metadata(None, pending_notes, doc_metadata),
        });
        pending_notes.clear();
    }
}

fn flush_section(
    chunks: &mut Vec<ChunkRecordInput>,
    section_heading: &mut Option<String>,
    section_parts: &mut Vec<DocumentElement>,
    pending_notes: &mut Vec<String>,
    doc_metadata: &Value,
) {
    if section_parts.is_empty() {
        return;
    }

    let heading = section_heading.clone();
    let mut has_paragraph = false;
    let mut has_bullets = false;
    let mut has_image = false;
    let mut lines = Vec::new();
    let mut shorts = Vec::new();

    for part in section_parts.iter() {
        match part.content_type {
            ContentType::BulletNumberedList => {
                has_bullets = true;
                lines.push(part.text.clone());
            }
            ContentType::HeadingSection => {
                lines.push(part.text.clone());
            }
            ContentType::ShortDisconnectedParagraph => {
                has_paragraph = true;
                shorts.push(part.text.clone());
            }
            ContentType::Image => {
                has_image = true;
                lines.push(part.text.clone());
            }
            _ => {
                has_paragraph = true;
                lines.push(part.text.clone());
            }
        }
    }

    if !shorts.is_empty() {
        lines.push(shorts.join(" "));
    }

    let combined = lines.join("\n").trim().to_string();
    if !combined.is_empty() {
        let content_type = if heading.is_some() && (has_paragraph || has_bullets || has_image) {
            ContentType::MixedContent
        } else {
            ContentType::HeadingSection
        };

        chunks.push(ChunkRecordInput {
            content_type,
            content: combined,
            metadata: base_chunk_metadata(heading, pending_notes, doc_metadata),
        });
        pending_notes.clear();
    }

    section_parts.clear();
    *section_heading = None;
}

fn base_chunk_metadata(
    section_heading: Option<String>,
    notes: &[String],
    doc_metadata: &Value,
) -> Value {
    json!({
        "footnotes_captions": notes,
        "page_number": Value::Null,
        "section_heading": section_heading,
        "document_metadata": doc_metadata,
    })
}

fn extract_text_from_xml(xml: &str) -> Result<String, String> {
    let mut out = String::new();
    let mut cursor = 0usize;

    while let Some(start) = find_next_wt_tag_start(xml, cursor) {
        let open_end = xml[start..]
            .find('>')
            .map(|i| start + i + 1)
            .ok_or_else(|| "Malformed text tag in DOCX XML".to_string())?;

        if open_end >= 2 && xml[open_end - 2..open_end].starts_with("/>") {
            cursor = open_end;
            continue;
        }

        let close = xml[open_end..]
            .find("</w:t>")
            .map(|i| open_end + i)
            .ok_or_else(|| {
                let start_snippet = start.saturating_sub(30);
                let end_snippet = (open_end + 60).min(xml.len());
                let snippet = &xml[start_snippet..end_snippet];
                format!("Unclosed text tag in DOCX XML near: {}", snippet)
            })?;

        let raw = &xml[open_end..close];
        let decoded = unescape(raw)
            .map_err(|e| format!("Failed to decode XML entities: {e}"))?
            .into_owned();

        if !decoded.trim().is_empty() {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(decoded.trim());
        }

        cursor = close + "</w:t>".len();
    }

    Ok(out)
}

fn find_next_wt_tag_start(xml: &str, from: usize) -> Option<usize> {
    let mut cursor = from;
    while let Some(rel_start) = xml[cursor..].find("<w:t") {
        let start = cursor + rel_start;
        let next_index = start + 4;
        let next = xml.as_bytes().get(next_index).copied();

        if matches!(
            next,
            Some(b'>') | Some(b'/') | Some(b' ') | Some(b'\t') | Some(b'\r') | Some(b'\n')
        ) {
            return Some(start);
        }

        cursor = next_index;
    }
    None
}

fn extract_footnotes(xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cursor = 0usize;

    while let Some(rel_start) = xml[cursor..].find("<w:footnote") {
        let start = cursor + rel_start;
        let end = match xml[start..].find("</w:footnote>") {
            Some(i) => start + i + "</w:footnote>".len(),
            None => break,
        };

        let block = &xml[start..end];
        if let Ok(text) = extract_text_from_xml(block) {
            let clean = text.trim().to_string();
            if !clean.is_empty() {
                out.push(clean);
            }
        }

        cursor = end;
    }

    out
}

fn read_zip_entry<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
    max_bytes: u64,
) -> Result<Option<String>, String> {
    match archive.by_name(name) {
        Ok(mut file) => {
            let size = file.size();
            if size > max_bytes {
                return Err(format!(
                    "{name} is too large after decompression: {} bytes (limit: {} bytes)",
                    size, max_bytes
                ));
            }
            let mut content = String::new();
            file.read_to_string(&mut content)
                .map_err(|e| format!("Failed to read {name}: {e}"))?;
            Ok(Some(content))
        }
        Err(zip::result::ZipError::FileNotFound) => Ok(None),
        Err(e) => Err(format!("Failed to open {name}: {e}")),
    }
}

fn read_first_prefixed_entry<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    prefix: &str,
    max_bytes: u64,
) -> Result<Option<String>, String> {
    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| format!("Failed to inspect zip entry at index {i}: {e}"))?;
        if file.name().starts_with(prefix) {
            let size = file.size();
            if size > max_bytes {
                return Err(format!(
                    "{} is too large after decompression: {} bytes (limit: {} bytes)",
                    file.name(),
                    size,
                    max_bytes
                ));
            }
            let mut content = String::new();
            file.read_to_string(&mut content)
                .map_err(|e| format!("Failed to read {}: {e}", file.name()))?;
            return Ok(Some(content));
        }
    }
    Ok(None)
}

fn count_prefixed_entries<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    prefix: &str,
) -> Result<usize, String> {
    let mut count = 0usize;
    for i in 0..archive.len() {
        let file = archive
            .by_index(i)
            .map_err(|e| format!("Failed to inspect zip entry at index {i}: {e}"))?;
        if file.name().starts_with(prefix) {
            count += 1;
        }
    }
    Ok(count)
}

fn semantic_chunks(text: &str, max_chars: usize) -> Vec<String> {
    if text.len() <= max_chars {
        return vec![text.trim().to_string()];
    }

    let sentences = split_sentences(text);
    let mut out = Vec::new();
    let mut current = String::new();

    for s in sentences {
        let candidate = if current.is_empty() {
            s.clone()
        } else {
            format!("{} {}", current, s)
        };

        if candidate.len() <= max_chars {
            current = candidate;
        } else {
            if !current.is_empty() {
                out.push(current.trim().to_string());
            }
            current = s;
        }
    }

    if !current.trim().is_empty() {
        out.push(current.trim().to_string());
    }

    if out.is_empty() {
        vec![text.trim().to_string()]
    } else {
        out
    }
}

fn split_sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();

    for ch in text.chars() {
        current.push(ch);
        if matches!(ch, '.' | '!' | '?') {
            let trimmed = current.trim();
            if !trimmed.is_empty() {
                out.push(trimmed.to_string());
            }
            current.clear();
        }
    }

    let tail = current.trim();
    if !tail.is_empty() {
        out.push(tail.to_string());
    }

    out
}

fn recursive_char_chunks(text: &str, max_chars: usize, overlap: usize) -> Vec<String> {
    if text.len() <= max_chars {
        return vec![text.to_string()];
    }

    let split_at = max_chars.min(text.len());
    let head = text[..split_at].trim().to_string();
    let tail_start = split_at.saturating_sub(overlap);
    let tail = text[tail_start..].trim().to_string();

    let mut out = vec![head];
    if !tail.is_empty() && tail.len() < text.len() {
        out.extend(recursive_char_chunks(&tail, max_chars, overlap));
    }
    out
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(chunk_docx, m)?)?;
    Ok(())
}
