use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};
use pyo3::wrap_pyfunction;
use pythonize::pythonize;
use std::fs;
use std::time::Instant;

use super::common::{
    classify_prose, html_metadata, parse_html_blocks, remove_comments, split_at_sentences,
    ChunkRecordInput, ContentType, HtmlBlockType, MAX_CHUNK_CHARS, MIN_CHUNK_CHARS,
};

// ── Prose helpers ─────────────────────────────────────────────────────────────

fn flush_prose(
    chunks: &mut Vec<ChunkRecordInput>,
    heading: &Option<String>,
    parts: &mut Vec<String>,
    len: &mut usize,
) {
    if parts.is_empty() { return; }
    let content = parts.join("\n").trim().to_string();
    parts.clear(); *len = 0;
    if content.is_empty() { return; }
    chunks.push(ChunkRecordInput {
        content_type: classify_prose(&content),
        content,
        metadata: html_metadata(heading.clone()),
    });
}

fn merge_short_prose(chunks: Vec<ChunkRecordInput>, min_chars: usize) -> Vec<ChunkRecordInput> {
    let soft_max = MAX_CHUNK_CHARS + min_chars;
    let mut result: Vec<ChunkRecordInput> = Vec::new();
    for chunk in chunks {
        let is_prose = matches!(
            chunk.content_type,
            ContentType::PlainParagraph | ContentType::ShortDisconnectedParagraph | ContentType::LongSingleParagraph
        );
        if is_prose && chunk.content.len() < min_chars {
            if let Some(prev) = result.last_mut() {
                let prev_prose = matches!(
                    prev.content_type,
                    ContentType::PlainParagraph | ContentType::ShortDisconnectedParagraph | ContentType::LongSingleParagraph
                );
                if prev_prose && prev.content.len() + chunk.content.len() + 1 <= soft_max {
                    prev.content = format!("{}\n{}", prev.content, chunk.content).trim().to_string();
                    prev.content_type = classify_prose(&prev.content);
                    continue;
                }
            }
        }
        result.push(chunk);
    }
    result
}

// ── Core build function ───────────────────────────────────────────────────────

pub fn build_chunks_from_html_bytes(bytes: &[u8]) -> Result<Vec<ChunkRecordInput>, String> {
    let text = std::str::from_utf8(bytes)
        .map(|v| v.to_string())
        .unwrap_or_else(|_| String::from_utf8_lossy(bytes).to_string());
    if text.trim().is_empty() { return Err("HTML file is empty after decoding".to_string()); }

    let blocks = parse_html_blocks(&remove_comments(&text));
    let mut chunks = Vec::new();
    let mut current_heading: Option<String> = None;
    let mut prose_parts: Vec<String> = Vec::new();
    let mut prose_len = 0usize;

    for block in blocks {
        match block.block_type {
            HtmlBlockType::Heading => {
                flush_prose(&mut chunks, &current_heading, &mut prose_parts, &mut prose_len);
                current_heading = Some(block.content.clone());
                chunks.push(ChunkRecordInput {
                    content_type: ContentType::HeadingSection,
                    content: block.content,
                    metadata: html_metadata(None),
                });
            }
            HtmlBlockType::Code => {
                flush_prose(&mut chunks, &current_heading, &mut prose_parts, &mut prose_len);
                chunks.push(ChunkRecordInput {
                    content_type: ContentType::CodeBlock,
                    content: block.content,
                    metadata: html_metadata(current_heading.clone()),
                });
            }
            HtmlBlockType::Table => {
                flush_prose(&mut chunks, &current_heading, &mut prose_parts, &mut prose_len);
                chunks.push(ChunkRecordInput {
                    content_type: ContentType::Table,
                    content: block.content,
                    metadata: html_metadata(current_heading.clone()),
                });
            }
            HtmlBlockType::List => {
                flush_prose(&mut chunks, &current_heading, &mut prose_parts, &mut prose_len);
                if !block.content.is_empty() {
                    chunks.push(ChunkRecordInput {
                        content_type: ContentType::BulletNumberedList,
                        content: block.content,
                        metadata: html_metadata(current_heading.clone()),
                    });
                }
            }
            HtmlBlockType::Paragraph => {
                if block.content.is_empty() { continue; }
                let parts = if block.content.len() > MAX_CHUNK_CHARS {
                    split_at_sentences(&block.content, MAX_CHUNK_CHARS)
                } else {
                    vec![block.content]
                };
                for part in parts {
                    let add = part.len() + 1;
                    if prose_len + add > MAX_CHUNK_CHARS && !prose_parts.is_empty() {
                        flush_prose(&mut chunks, &current_heading, &mut prose_parts, &mut prose_len);
                    }
                    prose_len += add;
                    prose_parts.push(part);
                }
            }
        }
    }
    flush_prose(&mut chunks, &current_heading, &mut prose_parts, &mut prose_len);
    let chunks = merge_short_prose(chunks, MIN_CHUNK_CHARS);
    if chunks.is_empty() { return Err("No chunks generated from HTML document".to_string()); }
    Ok(chunks)
}

// ── PyO3 entry points ─────────────────────────────────────────────────────────

fn emit(py: Python<'_>, chunks_raw: Vec<ChunkRecordInput>, rust_ms: f64) -> PyResult<PyObject> {
    let chunk_list: Vec<PyObject> = chunks_raw.into_iter().map(|c| {
        let dict = PyDict::new_bound(py);
        dict.set_item("content", &c.content)?;
        dict.set_item("content_type", c.content_type.as_str())?;
        dict.set_item("metadata", pythonize(py, &c.metadata)?)?;
        Ok(dict.into_any().unbind())
    }).collect::<PyResult<_>>()?;
    let result = PyDict::new_bound(py);
    result.set_item("chunks", chunk_list)?;
    result.set_item("rust_ms", (rust_ms * 1000.0).round() / 1000.0)?;
    Ok(result.into_any().unbind())
}

#[pyfunction]
pub fn chunk_html(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    if !file_path.to_ascii_lowercase().ends_with(".html")
        && !file_path.to_ascii_lowercase().ends_with(".htm")
    {
        return Err(PyValueError::new_err(format!("Expected .html/.htm file path, got: {file_path}")));
    }
    let bytes = fs::read(file_path).map_err(|e| PyIOError::new_err(format!("Failed to read HTML: {e}")))?;
    let rust_start = Instant::now();
    let chunks_raw = build_chunks_from_html_bytes(&bytes)
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to parse HTML: {e}")))?;
    let rust_ms = rust_start.elapsed().as_secs_f64() * 1000.0;
    emit(py, chunks_raw, rust_ms)
}

#[pyfunction]
pub fn chunk_html_structural(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    chunk_html(py, file_path)
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(chunk_html, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_html_structural, m)?)?;
    Ok(())
}
