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

#[derive(Debug, Clone)]
struct IndexedParagraph {
    index: usize,
    text: String,
}

#[derive(Debug, Clone)]
struct ChunkRecordInput {
    content: String,
    metadata: Value,
}

#[pyfunction]
fn chunk_docx_sliding_window(
    py: Python<'_>,
    file_path: &str,
    window_size: usize,
    overlap: usize,
) -> PyResult<PyObject> {
    if !file_path.to_ascii_lowercase().ends_with(".docx") {
        return Err(PyValueError::new_err(format!(
            "Expected .docx file path, got: {file_path}"
        )));
    }
    if window_size == 0 {
        return Err(PyValueError::new_err("window_size must be greater than 0"));
    }
    if overlap >= window_size {
        return Err(PyValueError::new_err(
            "overlap must be less than window_size",
        ));
    }

    let bytes = fs::read(file_path)
        .map_err(|e| PyIOError::new_err(format!("Failed to read DOCX file: {e}")))?;

    let rust_start = Instant::now();
    let paragraphs = parse_docx_paragraphs(&bytes)
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to parse DOCX: {e}")))?;
    let chunks_raw = build_sliding_window_chunks(paragraphs, window_size, overlap);
    let rust_ms = rust_start.elapsed().as_secs_f64() * 1000.0;

    let chunk_list: Vec<PyObject> = chunks_raw
        .into_iter()
        .map(|c| {
            let dict = PyDict::new_bound(py);
            dict.set_item("content", &c.content)?;
            dict.set_item("content_type", "sliding_window")?;
            dict.set_item("metadata", pythonize(py, &c.metadata)?)?;
            Ok(dict.into_any().unbind())
        })
        .collect::<PyResult<_>>()?;

    let result = PyDict::new_bound(py);
    result.set_item("chunks", chunk_list)?;
    result.set_item("rust_ms", (rust_ms * 1000.0).round() / 1000.0)?;
    Ok(result.into_any().unbind())
}

fn build_sliding_window_chunks(
    paragraphs: Vec<IndexedParagraph>,
    window_size: usize,
    overlap: usize,
) -> Vec<ChunkRecordInput> {
    if paragraphs.is_empty() {
        return Vec::new();
    }

    let step = window_size - overlap;
    let mut chunks = Vec::new();
    let mut start = 0usize;
    let mut window_index = 0usize;

    while start < paragraphs.len() {
        let end = (start + window_size).min(paragraphs.len());
        let window = &paragraphs[start..end];
        let content = window
            .iter()
            .map(|paragraph| paragraph.text.clone())
            .collect::<Vec<_>>()
            .join("\n\n");
        let paragraph_indices: Vec<usize> =
            window.iter().map(|paragraph| paragraph.index).collect();

        chunks.push(ChunkRecordInput {
            content,
            metadata: json!({
                "window_size": window_size,
                "overlap": overlap,
                "window_index": window_index,
                "paragraph_indices": paragraph_indices,
                "document_metadata": {
                    "source_type": "docx"
                }
            }),
        });

        if end == paragraphs.len() {
            break;
        }

        start += step;
        window_index += 1;
    }

    chunks
}

fn parse_docx_paragraphs(bytes: &[u8]) -> Result<Vec<IndexedParagraph>, String> {
    let cursor = Cursor::new(bytes);
    let mut archive =
        ZipArchive::new(cursor).map_err(|e| format!("DOCX is not a valid zip archive: {e}"))?;

    let mut document_xml_file = archive
        .by_name("word/document.xml")
        .map_err(|_| "word/document.xml not found in DOCX".to_string())?;

    parse_document_xml_paragraphs_streaming(&mut document_xml_file)
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

fn parse_document_xml_paragraphs_streaming<R: Read>(
    reader_src: R,
) -> Result<Vec<IndexedParagraph>, String> {
    let mut reader = Reader::from_reader(BufReader::new(reader_src));

    let mut buf = Vec::new();
    let mut paragraphs = Vec::new();

    let mut in_table = false;
    let mut in_cell = false;
    let mut in_text = false;
    let mut in_paragraph = false;

    let mut para_text = String::new();
    let mut para_has_drawing = false;

    let mut table_rows: Vec<String> = Vec::new();
    let mut row_cells: Vec<String> = Vec::new();
    let mut cell_text = String::new();
    let mut paragraph_index = 0usize;

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
                        para_has_drawing = false;
                    }
                } else if qname_eq(name, b"drawing") && in_paragraph {
                    para_has_drawing = true;
                } else if qname_eq(name, b"t") {
                    in_text = true;
                }
            }
            Ok(Event::Empty(e)) => {
                let name = e.name();
                if qname_eq(name, b"drawing") && in_paragraph {
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
                        paragraphs.push(IndexedParagraph {
                            index: paragraph_index,
                            text: table_text,
                        });
                        paragraph_index += 1;
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
                        paragraphs.push(IndexedParagraph {
                            index: paragraph_index,
                            text: normalized,
                        });
                        paragraph_index += 1;
                    }

                    in_paragraph = false;
                    para_text.clear();
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

    Ok(paragraphs)
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
    m.add_function(wrap_pyfunction!(chunk_docx_sliding_window, m)?)?;
    Ok(())
}
