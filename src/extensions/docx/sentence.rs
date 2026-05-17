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

const TITLE_ABBREVIATIONS: [&str; 8] = ["mr.", "mrs.", "dr.", "prof.", "sr.", "jr.", "vs.", "etc."];

#[derive(Debug, Clone)]
struct IndexedParagraph {
    index: usize,
    text: String,
}

#[derive(Debug, Clone)]
struct IndexedSentence {
    paragraph_index: usize,
    text: String,
}

#[derive(Debug, Clone)]
struct ChunkRecordInput {
    content: String,
    metadata: Value,
}

#[pyfunction]
fn chunk_docx_sentence(
    py: Python<'_>,
    file_path: &str,
    sentences_per_chunk: usize,
) -> PyResult<PyObject> {
    if !file_path.to_ascii_lowercase().ends_with(".docx") {
        return Err(PyValueError::new_err(format!(
            "Expected .docx file path, got: {file_path}"
        )));
    }
    if sentences_per_chunk == 0 {
        return Err(PyValueError::new_err(
            "sentences_per_chunk must be greater than 0",
        ));
    }

    let bytes = fs::read(file_path)
        .map_err(|e| PyIOError::new_err(format!("Failed to read DOCX file: {e}")))?;

    let rust_start = Instant::now();
    let paragraphs = parse_docx_paragraphs(&bytes)
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to parse DOCX: {e}")))?;
    let sentences = paragraphs
        .iter()
        .flat_map(|paragraph| split_paragraph_sentences(paragraph))
        .collect::<Vec<_>>();
    let chunks_raw = build_sentence_chunks(sentences, sentences_per_chunk);
    let rust_ms = rust_start.elapsed().as_secs_f64() * 1000.0;

    let chunk_list: Vec<PyObject> = chunks_raw
        .into_iter()
        .map(|c| {
            let dict = PyDict::new_bound(py);
            dict.set_item("content", &c.content)?;
            dict.set_item("content_type", "sentence")?;
            dict.set_item("metadata", pythonize(py, &c.metadata)?)?;
            Ok(dict.into_any().unbind())
        })
        .collect::<PyResult<_>>()?;

    let result = PyDict::new_bound(py);
    result.set_item("chunks", chunk_list)?;
    result.set_item("rust_ms", (rust_ms * 1000.0).round() / 1000.0)?;
    Ok(result.into_any().unbind())
}

fn build_sentence_chunks(
    sentences: Vec<IndexedSentence>,
    sentences_per_chunk: usize,
) -> Vec<ChunkRecordInput> {
    if sentences.is_empty() {
        return Vec::new();
    }

    let mut chunks = Vec::new();
    let mut start = 0usize;
    let mut chunk_index = 0usize;

    while start < sentences.len() {
        let end = (start + sentences_per_chunk).min(sentences.len());
        let window = &sentences[start..end];
        let content = window
            .iter()
            .map(|sentence| sentence.text.clone())
            .collect::<Vec<_>>()
            .join(" ");

        chunks.push(ChunkRecordInput {
            content,
            metadata: json!({
                "sentences_per_chunk": sentences_per_chunk,
                "actual_sentence_count": window.len(),
                "chunk_index": chunk_index,
                "source_paragraph_index": window[0].paragraph_index,
                "document_metadata": {
                    "source_type": "docx"
                }
            }),
        });

        start = end;
        chunk_index += 1;
    }

    chunks
}

fn split_paragraph_sentences(paragraph: &IndexedParagraph) -> Vec<IndexedSentence> {
    let chars: Vec<char> = paragraph.text.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut start = 0usize;
    let len = chars.len();
    let mut i = 0usize;

    while i < len {
        let ch = chars[i];
        if matches!(ch, '.' | '?' | '!') && should_split_at(&chars, i, ch) {
            let sentence = chars[start..=i].iter().collect::<String>();
            let sentence = collapse_whitespace(&sentence);
            if !sentence.is_empty() {
                out.push(IndexedSentence {
                    paragraph_index: paragraph.index,
                    text: sentence,
                });
            }

            let mut next_start = i + 1;
            while next_start < len && chars[next_start].is_whitespace() {
                next_start += 1;
            }
            start = next_start;
            i = next_start;
            continue;
        }
        i += 1;
    }

    if start < len {
        let tail = chars[start..].iter().collect::<String>();
        let tail = collapse_whitespace(&tail);
        if !tail.is_empty() {
            out.push(IndexedSentence {
                paragraph_index: paragraph.index,
                text: tail,
            });
        }
    }

    out
}

fn should_split_at(chars: &[char], punct_idx: usize, punct: char) -> bool {
    if punct_idx + 2 >= chars.len() {
        return false;
    }

    if chars[punct_idx + 1] != ' ' || !chars[punct_idx + 2].is_uppercase() {
        return false;
    }

    if punct == '.' {
        let prefix = chars[..=punct_idx].iter().collect::<String>();
        if ends_with_title_abbreviation(&prefix)
            || ends_with_numeric_marker(&prefix)
            || ends_with_initials_abbreviation(&prefix)
        {
            return false;
        }
    }

    true
}

fn ends_with_title_abbreviation(prefix: &str) -> bool {
    let lower = prefix.trim_end().to_ascii_lowercase();
    TITLE_ABBREVIATIONS.iter().any(|abbr| lower.ends_with(abbr))
}

fn ends_with_numeric_marker(prefix: &str) -> bool {
    let trimmed = prefix.trim_end();
    let without_dot = trimmed.strip_suffix('.').unwrap_or(trimmed);
    let token = without_dot
        .split_whitespace()
        .last()
        .unwrap_or_default()
        .trim();
    !token.is_empty() && token.chars().all(|c| c.is_ascii_digit())
}

fn ends_with_initials_abbreviation(prefix: &str) -> bool {
    let trimmed = prefix.trim_end();
    let bytes = trimmed.as_bytes();
    if bytes.len() < 4 || *bytes.last().unwrap_or(&b' ') != b'.' {
        return false;
    }

    // Match patterns like U.S. / U.K. / U.S.A.
    let mut i = bytes.len();
    let mut groups = 0usize;

    while i >= 2 {
        if bytes[i - 1] != b'.' {
            break;
        }
        if !bytes[i - 2].is_ascii_uppercase() {
            break;
        }
        groups += 1;
        if i < 3 {
            break;
        }
        i -= 2;
        if i > 0 && bytes[i - 1] == b'.' {
            continue;
        }
    }

    groups >= 2
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
    m.add_function(wrap_pyfunction!(chunk_docx_sentence, m)?)?;
    Ok(())
}
