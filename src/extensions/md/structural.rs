use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};
use pyo3::wrap_pyfunction;
use pythonize::pythonize;
use serde_json::{json, Value};
use std::fs;
use std::time::Instant;

use super::common::{
    classify_prose, extract_heading_text, heading_level, parse_markdown_blocks, split_at_sentences,
    strip_block_content, ChunkRecordInput, ContentType, MdBlockType, MAX_CHUNK_CHARS,
    MIN_CHUNK_CHARS,
};

// ── Prose helpers ─────────────────────────────────────────────────────────────

fn flush_prose(
    chunks: &mut Vec<ChunkRecordInput>,
    heading: &Option<String>,
    section_level: u8,
    parts: &mut Vec<String>,
    len: &mut usize,
) {
    if parts.is_empty() {
        return;
    }
    let content = parts.join("\n").trim().to_string();
    parts.clear();
    *len = 0;
    if content.is_empty() {
        return;
    }
    chunks.push(ChunkRecordInput {
        content_type: classify_prose(&content),
        content,
        metadata: md_metadata(heading.clone(), section_level),
    });
}

fn merge_short_prose(chunks: Vec<ChunkRecordInput>, min_chars: usize) -> Vec<ChunkRecordInput> {
    let soft_max = MAX_CHUNK_CHARS + min_chars;
    let mut result: Vec<ChunkRecordInput> = Vec::new();
    for chunk in chunks {
        let is_prose = matches!(
            chunk.content_type,
            ContentType::PlainParagraph
                | ContentType::ShortDisconnectedParagraph
                | ContentType::LongSingleParagraph
        );
        if is_prose && chunk.content.len() < min_chars {
            if let Some(prev) = result.last_mut() {
                let prev_is_prose = matches!(
                    prev.content_type,
                    ContentType::PlainParagraph
                        | ContentType::ShortDisconnectedParagraph
                        | ContentType::LongSingleParagraph
                );
                if prev_is_prose && prev.content.len() + chunk.content.len() + 1 <= soft_max {
                    prev.content = format!("{}\n{}", prev.content, chunk.content)
                        .trim()
                        .to_string();
                    prev.content_type = classify_prose(&prev.content);
                    continue;
                }
            }
        }
        result.push(chunk);
    }
    result
}

fn md_metadata(section_heading: Option<String>, section_level: u8) -> Value {
    json!({
        "section_heading": section_heading,
        "section_level":   section_level,
        "document_metadata": { "source_type": "md" }
    })
}

// ── Core build function ───────────────────────────────────────────────────────

pub fn build_chunks_from_md_bytes(bytes: &[u8]) -> Result<Vec<ChunkRecordInput>, String> {
    let text = std::str::from_utf8(bytes)
        .map(|v| v.to_string())
        .unwrap_or_else(|_| String::from_utf8_lossy(bytes).to_string());

    if text.trim().is_empty() {
        return Err("Markdown file is empty after decoding".to_string());
    }

    let blocks = parse_markdown_blocks(&text);

    let mut chunks: Vec<ChunkRecordInput> = Vec::new();
    let mut current_heading: Option<String> = None;
    let mut cur_section_level: u8 = 0;
    let mut prose_parts: Vec<String> = Vec::new();
    let mut prose_len: usize = 0;

    for block in blocks {
        match block.block_type {
            MdBlockType::Heading => {
                flush_prose(
                    &mut chunks,
                    &current_heading,
                    cur_section_level,
                    &mut prose_parts,
                    &mut prose_len,
                );
                let heading_text = extract_heading_text(&block.content);
                let level = heading_level(&block.content);
                current_heading = Some(heading_text.clone());
                cur_section_level = level;
                chunks.push(ChunkRecordInput {
                    content_type: ContentType::HeadingSection,
                    content: heading_text,
                    metadata: json!({
                        "section_heading": serde_json::Value::Null,
                        "section_level": level,
                        "document_metadata": { "source_type": "md" }
                    }),
                });
            }
            MdBlockType::Code => {
                flush_prose(
                    &mut chunks,
                    &current_heading,
                    cur_section_level,
                    &mut prose_parts,
                    &mut prose_len,
                );
                chunks.push(ChunkRecordInput {
                    content_type: ContentType::CodeBlock,
                    content: block.content,
                    metadata: md_metadata(current_heading.clone(), cur_section_level),
                });
            }
            MdBlockType::Table => {
                flush_prose(
                    &mut chunks,
                    &current_heading,
                    cur_section_level,
                    &mut prose_parts,
                    &mut prose_len,
                );
                chunks.push(ChunkRecordInput {
                    content_type: ContentType::Table,
                    content: block.content,
                    metadata: md_metadata(current_heading.clone(), cur_section_level),
                });
            }
            MdBlockType::List => {
                flush_prose(
                    &mut chunks,
                    &current_heading,
                    cur_section_level,
                    &mut prose_parts,
                    &mut prose_len,
                );
                let clean = strip_block_content(&block.content, true);
                if !clean.is_empty() {
                    chunks.push(ChunkRecordInput {
                        content_type: ContentType::BulletNumberedList,
                        content: clean,
                        metadata: md_metadata(current_heading.clone(), cur_section_level),
                    });
                }
            }
            MdBlockType::Paragraph => {
                let clean = strip_block_content(&block.content, false);
                if clean.is_empty() {
                    continue;
                }
                let sub_blocks = if clean.len() > MAX_CHUNK_CHARS {
                    split_at_sentences(&clean, MAX_CHUNK_CHARS)
                } else {
                    vec![clean]
                };
                for sub in sub_blocks {
                    let add = sub.len() + 1;
                    if prose_len + add > MAX_CHUNK_CHARS && !prose_parts.is_empty() {
                        flush_prose(
                            &mut chunks,
                            &current_heading,
                            cur_section_level,
                            &mut prose_parts,
                            &mut prose_len,
                        );
                    }
                    prose_len += add;
                    prose_parts.push(sub);
                }
            }
        }
    }

    flush_prose(
        &mut chunks,
        &current_heading,
        cur_section_level,
        &mut prose_parts,
        &mut prose_len,
    );
    let chunks = merge_short_prose(chunks, MIN_CHUNK_CHARS);

    if chunks.is_empty() {
        return Err("No chunks generated from Markdown document".to_string());
    }
    Ok(chunks)
}

// ── PyO3 entry point ──────────────────────────────────────────────────────────

#[pyfunction]
pub fn chunk_md(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    if !file_path.to_ascii_lowercase().ends_with(".md") {
        return Err(PyValueError::new_err(format!(
            "Expected .md file path, got: {file_path}"
        )));
    }

    let bytes = fs::read(file_path)
        .map_err(|e| PyIOError::new_err(format!("Failed to read MD file: {e}")))?;

    let rust_start = Instant::now();
    let chunks_raw = build_chunks_from_md_bytes(&bytes)
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to parse MD: {e}")))?;
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

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(chunk_md, m)?)?;
    Ok(())
}
