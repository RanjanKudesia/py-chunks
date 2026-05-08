use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};
use pyo3::wrap_pyfunction;
use pythonize::pythonize;
use quick_xml::events::Event;
use quick_xml::name::QName;
use quick_xml::Reader;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::fs;
use std::io::{BufReader, Cursor, Read};
use std::time::Instant;
use zip::ZipArchive;

const MAX_CHUNK_CHARS: usize = 1500;
const SHORT_PARAGRAPH_CHARS: usize = 80;
const MIN_SHORT_MERGE_OUTPUT_CHARS: usize = 60;

const REFERENCE_STARTS: [&str; 8] = [
    "this", "it", "they", "these", "that", "those", "its", "their",
];

const TRANSITION_STARTS: [&str; 12] = [
    "however",
    "nevertheless",
    "in contrast",
    "on the other hand",
    "meanwhile",
    "conversely",
    "that said",
    "in summary",
    "to conclude",
    "therefore",
    "thus",
    "hence",
];

const STOPWORDS: [&str; 24] = [
    "the", "and", "for", "are", "was", "were", "this", "that", "with", "from", "have", "been",
    "will", "they", "their", "which", "about", "into", "more", "also", "when", "than", "those",
    "these",
];

#[derive(Debug, Clone)]
struct SemanticChunk {
    paragraphs: Vec<String>,
    merge_reason: &'static str,
}

#[derive(Debug, Clone)]
struct ChunkRecordInput {
    content: String,
    metadata: Value,
}

#[pyfunction]
fn chunk_docx_semantic(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    if !file_path.to_ascii_lowercase().ends_with(".docx") {
        return Err(PyValueError::new_err(format!(
            "Expected .docx file path, got: {file_path}"
        )));
    }

    let bytes = fs::read(file_path)
        .map_err(|e| PyIOError::new_err(format!("Failed to read DOCX file: {e}")))?;

    let rust_start = Instant::now();
    let paragraphs = parse_docx_paragraphs(&bytes)
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to parse DOCX: {e}")))?;
    let chunks_raw = build_semantic_chunks(paragraphs);
    let rust_ms = rust_start.elapsed().as_secs_f64() * 1000.0;

    let chunk_list: Vec<PyObject> = chunks_raw
        .into_iter()
        .map(|c| {
            let dict = PyDict::new_bound(py);
            dict.set_item("content", &c.content)?;
            dict.set_item("content_type", "semantic")?;
            dict.set_item("metadata", pythonize(py, &c.metadata)?)?;
            Ok(dict.into_any().unbind())
        })
        .collect::<PyResult<_>>()?;

    let result = PyDict::new_bound(py);
    result.set_item("chunks", chunk_list)?;
    result.set_item("rust_ms", (rust_ms * 1000.0).round() / 1000.0)?;
    Ok(result.into_any().unbind())
}

fn parse_docx_paragraphs(bytes: &[u8]) -> Result<Vec<String>, String> {
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

fn parse_document_xml_paragraphs_streaming<R: Read>(reader_src: R) -> Result<Vec<String>, String> {
    let mut reader = Reader::from_reader(BufReader::new(reader_src));

    let mut buf = Vec::new();
    let mut paragraphs = Vec::new();

    let mut in_table = false;
    let mut in_cell = false;
    let mut in_text = false;
    let mut in_paragraph = false;

    let mut para_text = String::new();
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
                        para_is_list = false;
                        para_has_drawing = false;
                    }
                } else if qname_eq(name, b"numPr") && in_paragraph {
                    para_is_list = true;
                } else if qname_eq(name, b"drawing") && in_paragraph {
                    para_has_drawing = true;
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
                        paragraphs.push(table_text);
                    }
                    in_table = false;
                    in_cell = false;
                    table_rows.clear();
                    row_cells.clear();
                    cell_text.clear();
                } else if qname_eq(name, b"p") && in_paragraph {
                    let text = para_text.trim().to_string();
                    if !text.is_empty() || para_has_drawing {
                        let normalized = if !text.is_empty() {
                            if para_is_list {
                                format!("- {text}")
                            } else {
                                text
                            }
                        } else {
                            "[Image]".to_string()
                        };
                        paragraphs.push(normalized);
                    }

                    in_paragraph = false;
                    para_text.clear();
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

    Ok(paragraphs)
}

fn build_semantic_chunks(paragraphs: Vec<String>) -> Vec<ChunkRecordInput> {
    let cleaned: Vec<String> = paragraphs
        .into_iter()
        .map(|p| collapse_whitespace(&p))
        .filter(|p| !p.is_empty())
        .collect();

    if cleaned.is_empty() {
        return Vec::new();
    }

    let semantic_chunks =
        prune_small_short_chunks(merge_heading_singletons(group_semantic_chunks(cleaned)));
    semantic_chunks
        .into_iter()
        .map(|chunk| {
            let content = chunk.paragraphs.join("\n\n");
            ChunkRecordInput {
                content,
                metadata: json!({
                    "section_heading": Value::Null,
                    "paragraph_count": chunk.paragraphs.len(),
                    "merge_reason": chunk.merge_reason,
                    "document_metadata": {
                        "source_type": "docx"
                    }
                }),
            }
        })
        .collect()
}

fn merge_heading_singletons(chunks: Vec<SemanticChunk>) -> Vec<SemanticChunk> {
    let mut merged: Vec<SemanticChunk> = Vec::new();
    let mut pending_heading: Option<String> = None;

    for mut chunk in chunks {
        if is_heading_singleton(&chunk) {
            pending_heading = chunk.paragraphs.into_iter().next();
            continue;
        }

        if let Some(heading) = pending_heading.take() {
            if has_actual_body_content(&chunk) {
                let mut paragraphs = vec![heading];
                paragraphs.extend(chunk.paragraphs);
                chunk.paragraphs = paragraphs;
                chunk.merge_reason = "heading_merge";
            }
        }

        merged.push(chunk);
    }

    merged
}

fn prune_small_short_chunks(chunks: Vec<SemanticChunk>) -> Vec<SemanticChunk> {
    chunks
        .into_iter()
        .filter(|chunk| {
            !(chunk.merge_reason == "short_paragraph"
                && chunk.paragraphs.join("\n\n").len() < MIN_SHORT_MERGE_OUTPUT_CHARS)
        })
        .collect()
}

fn is_heading_singleton(chunk: &SemanticChunk) -> bool {
    chunk.paragraphs.len() == 1 && chunk.paragraphs[0].len() < 30
}

fn has_actual_body_content(chunk: &SemanticChunk) -> bool {
    if is_heading_singleton(chunk) {
        return false;
    }

    if chunk.paragraphs.len() == 1 {
        return chunk.paragraphs[0].len() > 50;
    }

    let first = &chunk.paragraphs[0];
    if first.len() < 30 {
        let body_len = chunk.paragraphs[1..].join("\n\n").len();
        return body_len > 50;
    }

    chunk.paragraphs.join("\n\n").len() > 50
}

fn group_semantic_chunks(paragraphs: Vec<String>) -> Vec<SemanticChunk> {
    let mut chunks = Vec::new();
    let mut current = SemanticChunk {
        paragraphs: vec![paragraphs[0].clone()],
        merge_reason: "keyword_overlap",
    };

    let mut force_merge_next = false;

    for para in paragraphs.iter().skip(1) {
        let para_is_short = is_short_paragraph(para);
        let mut merge = false;
        let mut merge_reason = current.merge_reason;
        let mut pending_break_reason: Option<&'static str> = None;

        if starts_with_reference_pronoun(para) {
            merge = true;
            merge_reason = "reference_continuity";
        } else if starts_with_transition_keyword(para) {
            pending_break_reason = Some("transition_break");
        } else if keyword_overlap_count(&current.paragraphs, para) >= 2 {
            merge = true;
            merge_reason = "keyword_overlap";
        } else if force_merge_next && can_short_merge(&current.paragraphs, para, para_is_short) {
            merge = true;
            merge_reason = "short_paragraph";
        }

        if merge {
            let merged_len = chunk_content_len(&current.paragraphs) + 2 + para.len();
            if merged_len > MAX_CHUNK_CHARS {
                current.merge_reason = "size_limit";
                chunks.push(current);
                current = SemanticChunk {
                    paragraphs: vec![para.clone()],
                    merge_reason: "size_limit",
                };
                force_merge_next = para_is_short && para.contains(' ');
                continue;
            }

            current.paragraphs.push(para.clone());
            current.merge_reason = merge_reason;
            force_merge_next = para_is_short && para.contains(' ');
            continue;
        }

        if let Some(reason) = pending_break_reason {
            current.merge_reason = reason;
            chunks.push(current);
            current = SemanticChunk {
                paragraphs: vec![para.clone()],
                merge_reason: reason,
            };
            force_merge_next = para_is_short && para.contains(' ');
            continue;
        }

        if para_is_short {
            chunks.push(current);
            current = SemanticChunk {
                paragraphs: vec![para.clone()],
                merge_reason: "short_paragraph",
            };
            force_merge_next = para.contains(' ');
            continue;
        }

        chunks.push(current);
        current = SemanticChunk {
            paragraphs: vec![para.clone()],
            merge_reason: "keyword_overlap",
        };
        force_merge_next = false;
    }

    chunks.push(current);
    chunks
}

fn is_short_paragraph(text: &str) -> bool {
    text.len() < SHORT_PARAGRAPH_CHARS
}

fn can_short_merge(
    current_paragraphs: &[String],
    next_paragraph: &str,
    next_is_short: bool,
) -> bool {
    if !next_paragraph.contains(' ') {
        return false;
    }

    if current_paragraphs.len() >= 3 && next_is_short {
        return false;
    }

    if trailing_short_paragraphs(current_paragraphs) >= 3 {
        return false;
    }

    true
}

fn trailing_short_paragraphs(paragraphs: &[String]) -> usize {
    paragraphs
        .iter()
        .rev()
        .take_while(|paragraph| is_short_paragraph(paragraph))
        .count()
}

fn starts_with_reference_pronoun(text: &str) -> bool {
    let lower = text.trim_start().to_ascii_lowercase();
    REFERENCE_STARTS.iter().any(|prefix| {
        lower == *prefix
            || lower
                .strip_prefix(prefix)
                .map(|rest| rest.starts_with(' ') || rest.starts_with(',') || rest.starts_with(':'))
                .unwrap_or(false)
    })
}

fn starts_with_transition_keyword(text: &str) -> bool {
    let lower = text.trim_start().to_ascii_lowercase();
    TRANSITION_STARTS.iter().any(|prefix| {
        lower == *prefix
            || lower
                .strip_prefix(prefix)
                .map(|rest| rest.starts_with(' ') || rest.starts_with(',') || rest.starts_with(':'))
                .unwrap_or(false)
    })
}

fn keyword_overlap_count(current_chunk: &[String], next_paragraph: &str) -> usize {
    let current_words = extract_keywords(&current_chunk.join(" "));
    let next_words = extract_keywords(next_paragraph);
    current_words.intersection(&next_words).count()
}

fn extract_keywords(text: &str) -> HashSet<String> {
    text.split(|c: char| !c.is_ascii_alphabetic())
        .map(|word| word.to_ascii_lowercase())
        .filter(|word| word.len() > 4)
        .filter(|word| !STOPWORDS.contains(&word.as_str()))
        .collect()
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

fn chunk_content_len(paragraphs: &[String]) -> usize {
    if paragraphs.is_empty() {
        return 0;
    }
    paragraphs.iter().map(|p| p.len()).sum::<usize>() + ((paragraphs.len() - 1) * 2)
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(chunk_docx_semantic, m)?)?;
    Ok(())
}
