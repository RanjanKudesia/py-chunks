use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};
use pyo3::wrap_pyfunction;
use pythonize::pythonize;
use quick_xml::events::Event;
use quick_xml::name::QName;
use quick_xml::Reader;
use serde_json::{json, Value};
use std::fs;
use std::io::{BufReader, Cursor, Read};
use std::time::Instant;
use zip::ZipArchive;

const MIN_PARAGRAPH_CHARS: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PageBreakSignal {
    Explicit,
    Section,
    None,
}

impl PageBreakSignal {
    fn as_str(self) -> &'static str {
        match self {
            PageBreakSignal::Explicit => "explicit",
            PageBreakSignal::Section => "section",
            PageBreakSignal::None => "estimated",
        }
    }
}

#[derive(Debug, Clone)]
struct ParagraphEvent {
    text: String,
    signal: PageBreakSignal,
}

#[derive(Debug, Clone)]
struct ChunkRecordInput {
    content: String,
    metadata: Value,
}

#[pyfunction]
fn chunk_docx_page_aware(
    py: Python<'_>,
    file_path: &str,
    paragraphs_per_page: usize,
) -> PyResult<PyObject> {
    if !file_path.to_ascii_lowercase().ends_with(".docx") {
        return Err(PyValueError::new_err(format!(
            "Expected .docx file path, got: {file_path}"
        )));
    }
    if paragraphs_per_page == 0 {
        return Err(PyValueError::new_err(
            "paragraphs_per_page must be greater than 0",
        ));
    }

    let bytes = fs::read(file_path)
        .map_err(|e| PyIOError::new_err(format!("Failed to read DOCX file: {e}")))?;

    let rust_start = Instant::now();
    let events = parse_docx_paragraph_events(&bytes)
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to parse DOCX: {e}")))?;
    let chunks_raw = build_page_aware_chunks(events, paragraphs_per_page);
    let rust_ms = rust_start.elapsed().as_secs_f64() * 1000.0;

    let chunk_list: Vec<PyObject> = chunks_raw
        .into_iter()
        .map(|c| {
            let dict = PyDict::new_bound(py);
            dict.set_item("content", &c.content)?;
            dict.set_item("content_type", "page_aware")?;
            dict.set_item("metadata", pythonize(py, &c.metadata)?)?;
            Ok(dict.into_any().unbind())
        })
        .collect::<PyResult<_>>()?;

    let result = PyDict::new_bound(py);
    result.set_item("chunks", chunk_list)?;
    result.set_item("rust_ms", (rust_ms * 1000.0).round() / 1000.0)?;
    Ok(result.into_any().unbind())
}

fn build_page_aware_chunks(
    events: Vec<ParagraphEvent>,
    paragraphs_per_page: usize,
) -> Vec<ChunkRecordInput> {
    if events.is_empty() {
        return Vec::new();
    }

    let mut chunks = Vec::new();
    let mut page_number = 1usize;
    let mut current_paragraphs: Vec<String> = Vec::new();
    let mut paragraph_count = 0usize;
    let mut current_break_type: Option<PageBreakSignal> = None;

    for event in events {
        current_paragraphs.push(event.text);
        paragraph_count += 1;

        let boundary_signal = if matches!(event.signal, PageBreakSignal::Explicit) {
            Some(PageBreakSignal::Explicit)
        } else if matches!(event.signal, PageBreakSignal::Section) {
            Some(PageBreakSignal::Section)
        } else if paragraph_count >= paragraphs_per_page {
            Some(PageBreakSignal::None)
        } else {
            None
        };

        if let Some(signal) = boundary_signal {
            current_break_type = Some(signal);
            chunks.push(build_page_chunk(
                &current_paragraphs,
                page_number,
                current_break_type.unwrap_or(PageBreakSignal::None),
                paragraph_count,
            ));
            page_number += 1;
            current_paragraphs.clear();
            paragraph_count = 0;
            current_break_type = None;
        }
    }

    if !current_paragraphs.is_empty() {
        chunks.push(build_page_chunk(
            &current_paragraphs,
            page_number,
            current_break_type.unwrap_or(PageBreakSignal::None),
            paragraph_count,
        ));
    }

    chunks
}

fn build_page_chunk(
    paragraphs: &[String],
    page_number: usize,
    break_type: PageBreakSignal,
    paragraph_count: usize,
) -> ChunkRecordInput {
    ChunkRecordInput {
        content: paragraphs.join("\n\n"),
        metadata: json!({
            "page_number": page_number,
            "page_break_type": break_type.as_str(),
            "paragraph_count": paragraph_count,
            "document_metadata": {
                "source_type": "docx"
            }
        }),
    }
}

fn parse_docx_paragraph_events(bytes: &[u8]) -> Result<Vec<ParagraphEvent>, String> {
    let cursor = Cursor::new(bytes);
    let mut archive =
        ZipArchive::new(cursor).map_err(|e| format!("DOCX is not a valid zip archive: {e}"))?;

    let mut document_xml_file = archive
        .by_name("word/document.xml")
        .map_err(|_| "word/document.xml not found in DOCX".to_string())?;

    parse_document_xml_paragraph_events_streaming(&mut document_xml_file)
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

fn parse_document_xml_paragraph_events_streaming<R: Read>(
    reader_src: R,
) -> Result<Vec<ParagraphEvent>, String> {
    let mut reader = Reader::from_reader(BufReader::new(reader_src));

    let mut buf = Vec::new();
    let mut events = Vec::new();

    let mut in_table = false;
    let mut in_cell = false;
    let mut in_text = false;
    let mut in_paragraph = false;

    let mut para_text = String::new();
    let mut para_has_page_break = false;
    let mut para_has_section_break = false;
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
                        para_has_page_break = false;
                        para_has_section_break = false;
                        para_has_drawing = false;
                    }
                } else if qname_eq(name, b"br") && in_paragraph {
                    for attr in e.attributes().flatten() {
                        if qname_eq(attr.key, b"type") {
                            let value = String::from_utf8_lossy(attr.value.as_ref())
                                .trim()
                                .to_ascii_lowercase();
                            if value == "page" {
                                para_has_page_break = true;
                                break;
                            }
                        }
                    }
                } else if qname_eq(name, b"sectPr") && in_paragraph {
                    para_has_section_break = true;
                } else if qname_eq(name, b"drawing") && in_paragraph {
                    para_has_drawing = true;
                } else if qname_eq(name, b"t") {
                    in_text = true;
                }
            }
            Ok(Event::Empty(e)) => {
                let name = e.name();
                if qname_eq(name, b"br") && in_paragraph {
                    for attr in e.attributes().flatten() {
                        if qname_eq(attr.key, b"type") {
                            let value = String::from_utf8_lossy(attr.value.as_ref())
                                .trim()
                                .to_ascii_lowercase();
                            if value == "page" {
                                para_has_page_break = true;
                                break;
                            }
                        }
                    }
                } else if qname_eq(name, b"sectPr") && in_paragraph {
                    para_has_section_break = true;
                } else if qname_eq(name, b"drawing") && in_paragraph {
                    para_has_drawing = true;
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
                    let table_text = collapse_whitespace(&table_rows.join("\n"))
                        .trim()
                        .to_string();
                    if table_text.len() >= MIN_PARAGRAPH_CHARS {
                        events.push(ParagraphEvent {
                            text: table_text,
                            signal: PageBreakSignal::None,
                        });
                    }
                    in_table = false;
                    in_cell = false;
                    table_rows.clear();
                    row_cells.clear();
                    cell_text.clear();
                } else if qname_eq(name, b"p") && in_paragraph {
                    let text = collapse_whitespace(&para_text);
                    let normalized = if !text.is_empty() {
                        text
                    } else if para_has_drawing {
                        "[Image]".to_string()
                    } else {
                        String::new()
                    };

                    if normalized.len() >= MIN_PARAGRAPH_CHARS {
                        let signal = if para_has_page_break {
                            PageBreakSignal::Explicit
                        } else if para_has_section_break {
                            PageBreakSignal::Section
                        } else {
                            PageBreakSignal::None
                        };
                        events.push(ParagraphEvent {
                            text: normalized,
                            signal,
                        });
                    }

                    in_paragraph = false;
                    para_text.clear();
                    para_has_page_break = false;
                    para_has_section_break = false;
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

    Ok(events)
}

fn collapse_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_space = false;

    for ch in text.chars() {
        if ch.is_whitespace() {
            if !in_space {
                out.push(' ');
                in_space = true;
            }
        } else {
            out.push(ch);
            in_space = false;
        }
    }

    out.trim().to_string()
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(chunk_docx_page_aware, m)?)?;
    Ok(())
}
