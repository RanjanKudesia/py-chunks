use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};
use pyo3::wrap_pyfunction;
use pythonize::pythonize;
use serde_json::{json, Value};

use super::common::{collapse_whitespace, parse_docx_indexed_paragraphs, IndexedParagraph};
use std::fs;
use std::time::Instant;

const TITLE_ABBREVIATIONS: [&str; 22] = [
    "mr.", "mrs.", "ms.", "dr.", "prof.", "sr.", "jr.", "vs.", "etc.", "st.", "capt.", "lt.",
    "sgt.", "gen.", "col.", "rev.", "hon.", "gov.", "sen.", "rep.", "fig.", "vol.",
];

/// Characters allowed to trail a terminal `.` / `?` / `!` and still count as
/// part of the same sentence (closing quotes, parentheses, brackets).
const SENTENCE_CLOSERS: &[char] = &['"', '\u{201D}', '\'', '\u{2019}', ')', ']'];

#[derive(Debug, Clone)]
struct IndexedSentence {
    paragraph_index: usize,
    text: String,
    paragraph_is_heading: bool,
    paragraph_heading_level: Option<u32>,
    paragraph_is_list: bool,
    paragraph_is_table: bool,
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
    let paragraphs = parse_docx_indexed_paragraphs(&bytes)
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
                "source_paragraph_is_heading": window[0].paragraph_is_heading,
                "source_paragraph_heading_level": window[0].paragraph_heading_level,
                "source_paragraph_is_list": window[0].paragraph_is_list,
                "source_paragraph_is_table": window[0].paragraph_is_table,
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
        if matches!(ch, '.' | '?' | '!') {
            if let Some(end_inclusive) = sentence_end_at(&chars, i, ch) {
                let sentence = chars[start..=end_inclusive].iter().collect::<String>();
                let sentence = collapse_whitespace(&sentence);
                if !sentence.is_empty() {
                    out.push(IndexedSentence {
                        paragraph_index: paragraph.index,
                        text: sentence,
                        paragraph_is_heading: paragraph.is_heading,
                        paragraph_heading_level: paragraph.heading_level,
                        paragraph_is_list: paragraph.is_list,
                        paragraph_is_table: paragraph.is_table,
                    });
                }

                let mut next_start = end_inclusive + 1;
                while next_start < len && chars[next_start].is_whitespace() {
                    next_start += 1;
                }
                start = next_start;
                i = next_start;
                continue;
            }
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
                paragraph_is_heading: paragraph.is_heading,
                paragraph_heading_level: paragraph.heading_level,
                paragraph_is_list: paragraph.is_list,
                paragraph_is_table: paragraph.is_table,
            });
        }
    }

    out
}

/// Determine whether a `.` / `?` / `!` at `punct_idx` ends a sentence, and if
/// so, return the inclusive end index (which may be past `punct_idx` when
/// trailing closing quotes/parens belong to the current sentence).
fn sentence_end_at(chars: &[char], punct_idx: usize, punct: char) -> Option<usize> {
    // Walk over any trailing closing quotes/parens that should stay with the
    // current sentence (e.g. `He said "Hello." Then left.`).
    let mut end = punct_idx;
    let mut after = punct_idx + 1;
    while after < chars.len() && SENTENCE_CLOSERS.contains(&chars[after]) {
        end = after;
        after += 1;
    }

    // Need at least one whitespace + uppercase letter after the (possibly
    // extended) sentence end to count as a boundary.
    if after + 1 >= chars.len() {
        return None;
    }
    if chars[after] != ' ' || !chars[after + 1].is_uppercase() {
        return None;
    }

    if punct == '.' {
        let prefix = chars[..=punct_idx].iter().collect::<String>();
        if ends_with_title_abbreviation(&prefix)
            || ends_with_list_marker(&prefix)
            || ends_with_initials_abbreviation(&prefix)
        {
            return None;
        }
    }

    Some(end)
}

fn ends_with_title_abbreviation(prefix: &str) -> bool {
    let lower = prefix.trim_end().to_ascii_lowercase();
    TITLE_ABBREVIATIONS.iter().any(|abbr| lower.ends_with(abbr))
}

/// True when `prefix` ends with a short numeric list/ordinal marker such as
/// `1.`, `12.`, or `999.`. Restricted to 1–3 digit tokens so that years
/// (`1985.`) and longer numbers still terminate sentences correctly.
fn ends_with_list_marker(prefix: &str) -> bool {
    let trimmed = prefix.trim_end();
    let without_dot = match trimmed.strip_suffix('.') {
        Some(s) => s,
        None => return false,
    };
    let token = without_dot
        .split_whitespace()
        .last()
        .unwrap_or_default()
        .trim();
    if token.is_empty() || token.len() > 3 {
        return false;
    }
    token.chars().all(|c| c.is_ascii_digit())
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

#[pyclass]
pub struct DocxSentenceIterator {
    chunks: Vec<ChunkRecordInput>,
    index: usize,
}

#[pymethods]
impl DocxSentenceIterator {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(&mut self, py: Python<'_>) -> PyResult<Option<PyObject>> {
        if self.index >= self.chunks.len() {
            return Ok(None);
        }
        let chunk = &self.chunks[self.index];
        self.index += 1;
        let dict = PyDict::new_bound(py);
        dict.set_item("content", &chunk.content)?;
        dict.set_item("content_type", "sentence")?;
        dict.set_item("metadata", pythonize(py, &chunk.metadata)?)?;
        Ok(Some(dict.into_any().unbind()))
    }
}

#[pyfunction]
fn chunk_docx_sentence_stream(
    file_path: &str,
    sentences_per_chunk: usize,
) -> PyResult<DocxSentenceIterator> {
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

    let paragraphs = parse_docx_indexed_paragraphs(&bytes)
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to parse DOCX: {e}")))?;
    let sentences = paragraphs
        .iter()
        .flat_map(|paragraph| split_paragraph_sentences(paragraph))
        .collect::<Vec<_>>();
    let chunks = build_sentence_chunks(sentences, sentences_per_chunk);

    Ok(DocxSentenceIterator { chunks, index: 0 })
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(chunk_docx_sentence, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_docx_sentence_stream, m)?)?;
    m.add_class::<DocxSentenceIterator>()?;
    Ok(())
}
