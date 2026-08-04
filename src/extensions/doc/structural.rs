use std::path::Path;
use std::time::Instant;

use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};
use pyo3::wrap_pyfunction;
use pythonize::pythonize;
use serde_json::json;

use super::cfb_reader;
use super::fib;
use super::paragraph_props;
use super::piece_table;
use super::stylesheet;
use super::text_extractor::{self, DocParagraph, ParagraphType};
use crate::extensions::shared::{floor_char_boundary, split_at_sentences, split_sentences, STOPWORDS};

const MAX_CHUNK_CHARS: usize = 1200;
const MAX_SECTION_CHARS: usize = 2000;

#[derive(Debug, Clone)]
pub(crate) struct ChunkRecord {
    pub(crate) content: String,
    pub(crate) content_type: &'static str,
    pub(crate) chunk_index: usize,
    pub(crate) heading_level: Option<u8>,
    pub(crate) paragraph_type: &'static str,
}

fn paragraph_type_str(t: &ParagraphType) -> &'static str {
    match t {
        ParagraphType::Heading(_) => "heading",
        ParagraphType::Normal => "normal",
        ParagraphType::Table => "table",
        ParagraphType::ListItem => "list_item",
        ParagraphType::PageBreak => "page_break",
    }
}

fn content_type_for_paragraph(t: &ParagraphType, short: bool) -> &'static str {
    if short {
        return "short_disconnected_paragraph";
    }
    match t {
        ParagraphType::Heading(_) => "heading",
        ParagraphType::Normal => "plain_paragraph",
        ParagraphType::Table => "table",
        ParagraphType::ListItem => "bullet_list",
        ParagraphType::PageBreak => "plain_paragraph",
    }
}

pub(crate) fn validate_doc_path(file_path: &str) -> PyResult<()> {
    if !file_path.to_ascii_lowercase().ends_with(".doc") {
        return Err(PyValueError::new_err(format!(
            "Expected .doc file path, got: {file_path}"
        )));
    }
    if !Path::new(file_path).exists() {
        return Err(PyIOError::new_err(format!("File not found: {file_path}")));
    }
    Ok(())
}

/// Append the stories that follow the main text — footnotes, headers/footers,
/// annotations, endnotes and text boxes — as labelled paragraphs.
///
/// A `.doc`'s CP space holds all of these after `ccpText`, each with its own
/// length in the FIB, and none of them were ever extracted (#70). On
/// sample.doc that is 2,761 characters: a footnote block, a header, and a text
/// box holding the "Units for Magnetic Properties" table.
///
/// They are appended rather than interleaved because the binary format gives no
/// reliable way to place a footnote back at its reference point, and a labelled
/// block at the end is honest about that.
fn append_side_stories(
    paragraphs: &mut Vec<DocParagraph>,
    word_doc: &[u8],
    table: &[u8],
    fib: &fib::Fib,
) {
    let mut cp = fib.ccp_text.max(0) as u32;
    let stories: [(&str, i32); 6] = [
        ("Footnotes", fib.ccp_ftn),
        ("Headers and footers", fib.ccp_hdd),
        ("Macros", fib.ccp_mcr),
        ("Comments", fib.ccp_atn),
        ("Endnotes", fib.ccp_edn),
        ("Text boxes", fib.ccp_txbx),
    ];
    for (label, len) in stories {
        let len = len.max(0) as u32;
        if len == 0 {
            continue;
        }
        let (from, to) = (cp, cp + len);
        cp = to;
        let Some(text) = piece_table::reconstruct_story(
            word_doc,
            table,
            fib.fc_clx,
            fib.lcb_clx,
            from,
            to,
        ) else {
            continue;
        };
        paragraphs.push(DocParagraph {
            content: format!("[{label}]"),
            paragraph_type: text_extractor::ParagraphType::Heading(2),
            heading_level: Some(2),
        });
        for line in text.split('\r') {
            let line = text_extractor::collapse_whitespace(
                &line.replace('\x07', " "),
            );
            if !line.is_empty() {
                paragraphs.push(DocParagraph {
                    content: line,
                    paragraph_type: text_extractor::ParagraphType::Normal,
                    heading_level: None,
                });
            }
        }
    }
}

pub(crate) fn load_doc_paragraphs(file_path: &str) -> Result<Vec<DocParagraph>, String> {
    let word_doc = cfb_reader::read_word_document_stream(file_path)?;
    let fib = fib::parse_fib(&word_doc)?;
    let table = cfb_reader::read_table_stream(file_path, fib.f_which_tbl_stm)?;
    let reconstructed = piece_table::parse_piece_table(
        &word_doc,
        &table,
        fib.fc_clx,
        fib.lcb_clx,
        fib.ccp_text,
    )?;
    let stylesheet = stylesheet::parse_stylesheet(&table, fib.fc_stshf, fib.lcb_stshf)?;
    let props = paragraph_props::parse_paragraph_props(
        &word_doc,
        &table,
        fib.fc_plcf_papx,
        fib.lcb_plcf_papx,
    )?;
    let mut paragraphs = text_extractor::extract_paragraphs(&reconstructed, &props, &stylesheet);
    append_side_stories(&mut paragraphs, &word_doc, &table, &fib);
    Ok(paragraphs)
}

/// Like [`load_doc_paragraphs`] but keeps each paragraph's raw ordinal, so
/// callers can interleave content anchored by raw paragraph index (used by
/// the image-aware markdown converter).
pub(crate) fn load_doc_paragraphs_indexed(
    file_path: &str,
) -> Result<Vec<(usize, DocParagraph)>, String> {
    let word_doc = cfb_reader::read_word_document_stream(file_path)?;
    let fib = fib::parse_fib(&word_doc)?;
    let table = cfb_reader::read_table_stream(file_path, fib.f_which_tbl_stm)?;
    let reconstructed = piece_table::parse_piece_table(
        &word_doc,
        &table,
        fib.fc_clx,
        fib.lcb_clx,
        fib.ccp_text,
    )?;
    let stylesheet = stylesheet::parse_stylesheet(&table, fib.fc_stshf, fib.lcb_stshf)?;
    let props = paragraph_props::parse_paragraph_props(
        &word_doc,
        &table,
        fib.fc_plcf_papx,
        fib.lcb_plcf_papx,
    )?;
    Ok(text_extractor::extract_paragraphs_indexed(
        &reconstructed,
        &props,
        &stylesheet,
    ))
}

/// Split an oversized paragraph so no chunk exceeds `max_chars`. Prefers
/// sentence boundaries; any residual piece still over the limit (text with no
/// detectable sentence boundaries) is hard-split at UTF-8 char boundaries so the
/// cap is always honoured.
fn split_oversized(text: &str, max_chars: usize) -> Vec<String> {
    if text.len() <= max_chars {
        return vec![text.to_string()];
    }
    let mut out = Vec::new();
    for piece in split_at_sentences(text, max_chars) {
        if piece.len() <= max_chars {
            if !piece.is_empty() {
                out.push(piece);
            }
            continue;
        }
        let bytes = piece.as_bytes();
        let mut start = 0usize;
        while start < bytes.len() {
            let mut end = (start + max_chars).min(piece.len());
            end = floor_char_boundary(&piece, end);
            if end <= start {
                // max_chars smaller than a single char; advance to next boundary.
                end = floor_char_boundary(&piece, start + 4).max(start + 1).min(piece.len());
            }
            let slice = piece[start..end].trim();
            if !slice.is_empty() {
                out.push(slice.to_string());
            }
            start = end;
        }
    }
    if out.is_empty() {
        vec![text.trim().to_string()]
    } else {
        out
    }
}

pub(crate) fn build_structural_chunks(paragraphs: Vec<DocParagraph>) -> Vec<ChunkRecord> {
    let mut out = Vec::new();
    let mut short_buf = String::new();

    for p in paragraphs {
        if matches!(p.paragraph_type, ParagraphType::PageBreak) {
            if !short_buf.is_empty() {
                let content = short_buf.trim().to_string();
                if !content.is_empty() {
                    out.push(ChunkRecord {
                        content,
                        content_type: "short_disconnected_paragraph",
                        chunk_index: 0,
                        heading_level: None,
                        paragraph_type: "normal",
                    });
                }
                short_buf.clear();
            }
            continue;
        }

        let trimmed = p.content.trim();
        if trimmed.is_empty() {
            continue;
        }

        let is_short_normal = matches!(p.paragraph_type, ParagraphType::Normal) && trimmed.len() < 80;
        if is_short_normal {
            let candidate = if short_buf.is_empty() {
                trimmed.to_string()
            } else {
                format!("{}\n{}", short_buf, trimmed)
            };
            if candidate.len() > MAX_CHUNK_CHARS && !short_buf.is_empty() {
                out.push(ChunkRecord {
                    content: short_buf.trim().to_string(),
                    content_type: "short_disconnected_paragraph",
                    chunk_index: 0,
                    heading_level: None,
                    paragraph_type: "normal",
                });
                short_buf = trimmed.to_string();
            } else {
                short_buf = candidate;
            }
            continue;
        }

        if !short_buf.is_empty() {
            out.push(ChunkRecord {
                content: short_buf.trim().to_string(),
                content_type: "short_disconnected_paragraph",
                chunk_index: 0,
                heading_level: None,
                paragraph_type: "normal",
            });
            short_buf.clear();
        }

        let content_type = content_type_for_paragraph(&p.paragraph_type, false);
        let paragraph_type = paragraph_type_str(&p.paragraph_type);
        // A single paragraph can exceed MAX_CHUNK_CHARS (e.g. a document with no
        // paragraph breaks). Split it so we never emit an unsplittable mega-chunk,
        // mirroring how the docx chunker recursively splits oversized paragraphs.
        for piece in split_oversized(trimmed, MAX_CHUNK_CHARS) {
            out.push(ChunkRecord {
                content: piece,
                content_type,
                chunk_index: 0,
                heading_level: p.heading_level,
                paragraph_type,
            });
        }
    }

    if !short_buf.is_empty() {
        out.push(ChunkRecord {
            content: short_buf.trim().to_string(),
            content_type: "short_disconnected_paragraph",
            chunk_index: 0,
            heading_level: None,
            paragraph_type: "normal",
        });
    }

    for (i, ch) in out.iter_mut().enumerate() {
        ch.chunk_index = i;
    }
    out
}

pub(crate) fn build_section_chunks(paragraphs: Vec<DocParagraph>) -> Vec<ChunkRecord> {
    let mut out = Vec::new();
    let mut current_heading = "Preamble".to_string();
    let mut current_level: Option<u8> = None;
    let mut lines: Vec<String> = Vec::new();

    let flush = |out: &mut Vec<ChunkRecord>, heading: &str, level: Option<u8>, lines: &mut Vec<String>| {
        if lines.is_empty() {
            return;
        }
        let joined = lines.join("\n").trim().to_string();
        lines.clear();
        if joined.is_empty() {
            return;
        }

        if joined.len() <= MAX_SECTION_CHARS {
            out.push(ChunkRecord {
                content: joined,
                content_type: "section",
                chunk_index: 0,
                heading_level: level,
                paragraph_type: if heading == "Preamble" { "normal" } else { "heading" },
            });
            return;
        }

        let mut start = 0usize;
        while start < joined.len() {
            // Snap to a UTF-8 char boundary so multi-byte content (e.g. '÷', CJK)
            // never triggers a mid-character slice panic on very large sections.
            let raw_end = (start + MAX_SECTION_CHARS).min(joined.len());
            let mut end = floor_char_boundary(&joined, raw_end);
            if end <= start {
                end = joined.len();
            }
            let part = joined[start..end].trim().to_string();
            if !part.is_empty() {
                out.push(ChunkRecord {
                    content: part,
                    content_type: "section",
                    chunk_index: 0,
                    heading_level: level,
                    paragraph_type: if heading == "Preamble" { "normal" } else { "heading" },
                });
            }
            start = end;
        }
    };

    for p in paragraphs {
        if matches!(p.paragraph_type, ParagraphType::PageBreak) {
            flush(&mut out, &current_heading, current_level, &mut lines);
            continue;
        }

        let content = p.content.trim();
        if content.is_empty() {
            continue;
        }

        if let ParagraphType::Heading(level) = p.paragraph_type {
            flush(&mut out, &current_heading, current_level, &mut lines);
            current_heading = content.to_string();
            current_level = Some(level);
            lines.push(content.to_string());
        } else {
            lines.push(content.to_string());
        }
    }

    flush(&mut out, &current_heading, current_level, &mut lines);

    for (i, ch) in out.iter_mut().enumerate() {
        ch.chunk_index = i;
    }
    out
}

fn keyword_set(text: &str) -> std::collections::HashSet<String> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .map(|w| w.to_ascii_lowercase())
        .filter(|w| w.len() >= 4)
        .filter(|w| !STOPWORDS.contains(&w.as_str()))
        .collect()
}

pub(crate) fn build_semantic_chunks(paragraphs: Vec<DocParagraph>) -> Vec<ChunkRecord> {
    let filtered: Vec<DocParagraph> = paragraphs
        .into_iter()
        .filter(|p| !matches!(p.paragraph_type, ParagraphType::PageBreak))
        .filter(|p| !p.content.trim().is_empty())
        .collect();

    if filtered.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut current = String::new();
    let mut current_keywords = std::collections::HashSet::new();
    let mut current_heading_level = None;
    let mut current_para_type = "normal";

    let flush = |out: &mut Vec<ChunkRecord>, content: &mut String, heading_level: Option<u8>, para_type: &'static str| {
        let val = content.trim().to_string();
        if val.is_empty() {
            return;
        }
        out.push(ChunkRecord {
            content: val,
            content_type: "semantic",
            chunk_index: 0,
            heading_level,
            paragraph_type: para_type,
        });
        content.clear();
    };

    for p in filtered {
        let text = p.content.trim();
        let keys = keyword_set(text);

        let is_heading = matches!(p.paragraph_type, ParagraphType::Heading(_));
        let overlaps = !current_keywords.is_empty() && current_keywords.intersection(&keys).next().is_some();
        let starts_reference = {
            let lower = text.to_ascii_lowercase();
            ["this ", "it ", "they ", "these ", "that ", "those "]
                .iter()
                .any(|x| lower.starts_with(x))
        };

        let should_split = is_heading
            || (!current.is_empty()
                && !overlaps
                && !starts_reference
                && current.len() + 2 + text.len() > MAX_CHUNK_CHARS);

        if should_split {
            flush(&mut out, &mut current, current_heading_level, current_para_type);
            current_keywords.clear();
        }

        if !current.is_empty() {
            current.push_str("\n\n");
        }
        current.push_str(text);
        current_keywords.extend(keys);
        current_heading_level = p.heading_level;
        current_para_type = paragraph_type_str(&p.paragraph_type);
    }

    flush(&mut out, &mut current, current_heading_level, current_para_type);

    for (i, ch) in out.iter_mut().enumerate() {
        ch.chunk_index = i;
    }
    out
}

pub(crate) fn build_sliding_window_chunks(
    paragraphs: Vec<DocParagraph>,
    window_size: usize,
    overlap: usize,
) -> Vec<ChunkRecord> {
    if window_size == 0 || overlap >= window_size {
        return Vec::new();
    }

    let items: Vec<DocParagraph> = paragraphs
        .into_iter()
        .filter(|p| !matches!(p.paragraph_type, ParagraphType::PageBreak))
        .filter(|p| !p.content.trim().is_empty())
        .collect();
    if items.is_empty() {
        return Vec::new();
    }

    let step = window_size - overlap;
    let mut out = Vec::new();
    let mut start = 0usize;

    while start < items.len() {
        let end = (start + window_size).min(items.len());
        let window = &items[start..end];
        let content = window
            .iter()
            .map(|p| p.content.trim().to_string())
            .collect::<Vec<_>>()
            .join("\n\n");
        out.push(ChunkRecord {
            content,
            content_type: "sliding_window",
            chunk_index: 0,
            heading_level: window.iter().find_map(|p| p.heading_level),
            paragraph_type: paragraph_type_str(&window[0].paragraph_type),
        });

        if end == items.len() {
            break;
        }
        start += step;
    }

    for (i, ch) in out.iter_mut().enumerate() {
        ch.chunk_index = i;
    }
    out
}

pub(crate) fn build_sentence_chunks(
    paragraphs: Vec<DocParagraph>,
    sentences_per_chunk: usize,
) -> Vec<ChunkRecord> {
    if sentences_per_chunk == 0 {
        return Vec::new();
    }

    let mut indexed_sentences: Vec<(String, Option<u8>, &'static str)> = Vec::new();
    for p in paragraphs {
        if matches!(p.paragraph_type, ParagraphType::PageBreak) {
            continue;
        }
        let text = p.content.trim();
        if text.is_empty() {
            continue;
        }
        let mut sentences = split_sentences(text);
        if sentences.is_empty() {
            sentences.push(text.to_string());
        }
        for s in sentences {
            indexed_sentences.push((s, p.heading_level, paragraph_type_str(&p.paragraph_type)));
        }
    }

    let mut out = Vec::new();
    let mut idx = 0usize;
    while idx < indexed_sentences.len() {
        let end = (idx + sentences_per_chunk).min(indexed_sentences.len());
        let window = &indexed_sentences[idx..end];
        let content = window
            .iter()
            .map(|(s, _, _)| s.clone())
            .collect::<Vec<_>>()
            .join(" ");
        out.push(ChunkRecord {
            content,
            content_type: "sentence",
            chunk_index: 0,
            heading_level: window[0].1,
            paragraph_type: window[0].2,
        });
        idx = end;
    }

    for (i, ch) in out.iter_mut().enumerate() {
        ch.chunk_index = i;
    }
    out
}

pub(crate) fn build_page_aware_chunks(
    paragraphs: Vec<DocParagraph>,
    paragraphs_per_page: usize,
) -> Vec<ChunkRecord> {
    if paragraphs_per_page == 0 {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut acc: Vec<DocParagraph> = Vec::new();

    let flush = |out: &mut Vec<ChunkRecord>, acc: &mut Vec<DocParagraph>| {
        if acc.is_empty() {
            return;
        }
        let content = acc
            .iter()
            .map(|p| p.content.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");
        if !content.is_empty() {
            out.push(ChunkRecord {
                content,
                content_type: "page_aware",
                chunk_index: 0,
                heading_level: acc.iter().find_map(|p| p.heading_level),
                paragraph_type: paragraph_type_str(&acc[0].paragraph_type),
            });
        }
        acc.clear();
    };

    for p in paragraphs {
        if matches!(p.paragraph_type, ParagraphType::PageBreak) {
            flush(&mut out, &mut acc);
            continue;
        }
        if p.content.trim().is_empty() {
            continue;
        }
        acc.push(p);
        let normal_count = acc
            .iter()
            .filter(|x| matches!(x.paragraph_type, ParagraphType::Normal))
            .count();
        if normal_count >= paragraphs_per_page {
            flush(&mut out, &mut acc);
        }
    }
    flush(&mut out, &mut acc);

    for (i, ch) in out.iter_mut().enumerate() {
        ch.chunk_index = i;
    }
    out
}

pub(crate) fn chunks_to_py(
    py: Python<'_>,
    file_path: &str,
    chunks: Vec<ChunkRecord>,
    rust_ms: f64,
) -> PyResult<PyObject> {
    let total_chunks = chunks.len();
    let py_chunks: Vec<PyObject> = chunks
        .iter()
        .map(|chunk| {
            let dict = PyDict::new_bound(py);
            dict.set_item("content", &chunk.content)?;
            dict.set_item("content_type", chunk.content_type)?;
            dict.set_item(
                "metadata",
                pythonize(
                    py,
                    &json!({
                        "source": file_path,
                        "chunk_index": chunk.chunk_index,
                        "total_chunks": total_chunks,
                        "paragraph_type": chunk.paragraph_type,
                        "heading_level": chunk.heading_level,
                        "page_number": serde_json::Value::Null,
                    }),
                )?,
            )?;
            Ok(dict.into_any().unbind())
        })
        .collect::<PyResult<_>>()?;

    let result = PyDict::new_bound(py);
    result.set_item("chunks", py_chunks)?;
    result.set_item("rust_ms", (rust_ms * 1000.0).round() / 1000.0)?;
    Ok(result.into_any().unbind())
}

#[pyfunction]
fn chunk_doc(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    validate_doc_path(file_path)?;
    let start = Instant::now();
    let paragraphs = load_doc_paragraphs(file_path).map_err(PyRuntimeError::new_err)?;
    let chunks = build_structural_chunks(paragraphs);
    let rust_ms = start.elapsed().as_secs_f64() * 1000.0;
    chunks_to_py(py, file_path, chunks, rust_ms)
}

#[pyfunction]
fn chunk_doc_section(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    validate_doc_path(file_path)?;
    let start = Instant::now();
    let paragraphs = load_doc_paragraphs(file_path).map_err(PyRuntimeError::new_err)?;
    let chunks = build_section_chunks(paragraphs);
    let rust_ms = start.elapsed().as_secs_f64() * 1000.0;
    chunks_to_py(py, file_path, chunks, rust_ms)
}

#[pyfunction]
fn chunk_doc_semantic(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    validate_doc_path(file_path)?;
    let start = Instant::now();
    let paragraphs = load_doc_paragraphs(file_path).map_err(PyRuntimeError::new_err)?;
    let chunks = build_semantic_chunks(paragraphs);
    let rust_ms = start.elapsed().as_secs_f64() * 1000.0;
    chunks_to_py(py, file_path, chunks, rust_ms)
}

#[pyfunction]
fn chunk_doc_sliding_window(
    py: Python<'_>,
    file_path: &str,
    window_size: usize,
    overlap: usize,
) -> PyResult<PyObject> {
    validate_doc_path(file_path)?;
    if window_size == 0 {
        return Err(PyValueError::new_err("window_size must be greater than 0"));
    }
    if overlap >= window_size {
        return Err(PyValueError::new_err("overlap must be less than window_size"));
    }
    let start = Instant::now();
    let paragraphs = load_doc_paragraphs(file_path).map_err(PyRuntimeError::new_err)?;
    let chunks = build_sliding_window_chunks(paragraphs, window_size, overlap);
    let rust_ms = start.elapsed().as_secs_f64() * 1000.0;
    chunks_to_py(py, file_path, chunks, rust_ms)
}

#[pyfunction]
fn chunk_doc_sentence(
    py: Python<'_>,
    file_path: &str,
    sentences_per_chunk: usize,
) -> PyResult<PyObject> {
    validate_doc_path(file_path)?;
    if sentences_per_chunk == 0 {
        return Err(PyValueError::new_err(
            "sentences_per_chunk must be greater than 0",
        ));
    }
    let start = Instant::now();
    let paragraphs = load_doc_paragraphs(file_path).map_err(PyRuntimeError::new_err)?;
    let chunks = build_sentence_chunks(paragraphs, sentences_per_chunk);
    let rust_ms = start.elapsed().as_secs_f64() * 1000.0;
    chunks_to_py(py, file_path, chunks, rust_ms)
}

#[pyfunction]
fn chunk_doc_page_aware(
    py: Python<'_>,
    file_path: &str,
    paragraphs_per_page: usize,
) -> PyResult<PyObject> {
    validate_doc_path(file_path)?;
    if paragraphs_per_page == 0 {
        return Err(PyValueError::new_err(
            "paragraphs_per_page must be greater than 0",
        ));
    }
    let start = Instant::now();
    let paragraphs = load_doc_paragraphs(file_path).map_err(PyRuntimeError::new_err)?;
    let chunks = build_page_aware_chunks(paragraphs, paragraphs_per_page);
    let rust_ms = start.elapsed().as_secs_f64() * 1000.0;
    chunks_to_py(py, file_path, chunks, rust_ms)
}

#[pyclass]
pub struct DocStructuralIterator {
    chunks: Vec<ChunkRecord>,
    index: usize,
    source: String,
    total_chunks: usize,
}

#[pymethods]
impl DocStructuralIterator {
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
        dict.set_item("content_type", chunk.content_type)?;
        dict.set_item(
            "metadata",
            pythonize(
                py,
                &json!({
                    "source": self.source,
                    "chunk_index": chunk.chunk_index,
                    "total_chunks": self.total_chunks,
                    "paragraph_type": chunk.paragraph_type,
                    "heading_level": chunk.heading_level,
                    "page_number": serde_json::Value::Null,
                }),
            )?,
        )?;
        Ok(Some(dict.into_any().unbind()))
    }
}

#[pyfunction]
fn chunk_doc_structural_stream(file_path: &str) -> PyResult<DocStructuralIterator> {
    validate_doc_path(file_path)?;
    let paragraphs = load_doc_paragraphs(file_path).map_err(PyRuntimeError::new_err)?;
    let chunks = build_structural_chunks(paragraphs);
    let total_chunks = chunks.len();
    Ok(DocStructuralIterator {
        chunks,
        index: 0,
        source: file_path.to_string(),
        total_chunks,
    })
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(chunk_doc, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_doc_section, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_doc_semantic, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_doc_sliding_window, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_doc_sentence, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_doc_page_aware, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_doc_structural_stream, m)?)?;
    m.add_class::<DocStructuralIterator>()?;
    Ok(())
}
