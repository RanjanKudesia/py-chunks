use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};
use pyo3::wrap_pyfunction;
use pythonize::pythonize;
use serde_json::json;
use std::fs;
use std::time::Instant;

use super::common::{
    current_section_heading, heading_path_strings, parse_html_blocks, remove_comments,
    split_sentences, update_heading_stack, ChunkRecordInput, ContentType, HtmlBlockType,
};

struct IndexedSentence {
    text: String, para_index: usize,
    section_heading: Option<String>, heading_path: Vec<String>,
}

pub fn build_sentence_chunks(bytes: &[u8], sentences_per_chunk: usize) -> Result<Vec<ChunkRecordInput>, String> {
    if sentences_per_chunk == 0 { return Err("sentences_per_chunk must be > 0".to_string()); }
    let text = std::str::from_utf8(bytes).map(|s| s.to_string()).unwrap_or_else(|_| String::from_utf8_lossy(bytes).to_string());
    if text.trim().is_empty() { return Err("HTML file is empty".to_string()); }
    let blocks = parse_html_blocks(&remove_comments(&text));
    let total = blocks.len();
    let mut result: Vec<ChunkRecordInput> = Vec::new();
    let mut heading_stack: Vec<(u8, String)> = Vec::new();
    let mut sentences: Vec<IndexedSentence> = Vec::new();
    let mut para_index = 0usize; let mut chunk_index = 0usize;

    let flush = |sentences: &mut Vec<IndexedSentence>, result: &mut Vec<ChunkRecordInput>, ci: &mut usize, spc: usize, total: usize| {
        let mut i = 0usize;
        while i < sentences.len() {
            let end = (i + spc).min(sentences.len());
            let window = &sentences[i..end];
            let content = window.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" ");
            if !content.is_empty() {
                result.push(ChunkRecordInput {
                    content_type: ContentType::Sentence, content,
                    metadata: json!({
                        "sentences_per_chunk": spc, "actual_sentence_count": window.len(),
                        "chunk_index": *ci, "source_paragraph_index": window[0].para_index,
                        "section_heading": window[0].section_heading, "heading_path": window[0].heading_path,
                        "document_metadata": { "source_type": "html", "total_input_blocks": total }
                    }),
                });
                *ci += 1;
            }
            i = end;
        }
        sentences.clear();
    };

    for block in &blocks {
        match block.block_type {
            HtmlBlockType::Heading => {
                flush(&mut sentences, &mut result, &mut chunk_index, sentences_per_chunk, total);
                update_heading_stack(&mut heading_stack, block.heading_level, block.content.clone());
                result.push(ChunkRecordInput {
                    content_type: ContentType::HeadingSection, content: block.content.clone(),
                    metadata: json!({
                        "section_heading": block.content, "section_level": block.heading_level,
                        "heading_path": heading_path_strings(&heading_stack), "chunk_index": chunk_index,
                        "document_metadata": { "source_type": "html", "total_input_blocks": total }
                    }),
                });
                chunk_index += 1;
            }
            HtmlBlockType::Code | HtmlBlockType::Table => {
                flush(&mut sentences, &mut result, &mut chunk_index, sentences_per_chunk, total);
                let ct = if block.block_type == HtmlBlockType::Code { ContentType::CodeBlock } else { ContentType::Table };
                result.push(ChunkRecordInput {
                    content_type: ct, content: block.content.clone(),
                    metadata: json!({ "section_heading": current_section_heading(&heading_stack), "heading_path": heading_path_strings(&heading_stack), "chunk_index": chunk_index, "document_metadata": { "source_type": "html", "total_input_blocks": total } }),
                });
                chunk_index += 1; para_index += 1;
            }
            HtmlBlockType::List => {
                let sh = current_section_heading(&heading_stack);
                let hp = heading_path_strings(&heading_stack);
                let pi = para_index;
                for line in block.content.lines() {
                    let t = line.trim().to_string();
                    if !t.is_empty() { sentences.push(IndexedSentence { text: t, para_index: pi, section_heading: sh.clone(), heading_path: hp.clone() }); }
                }
                para_index += 1;
            }
            HtmlBlockType::Paragraph => {
                let clean = block.content.trim().to_string();
                if clean.is_empty() { continue; }
                let sh = current_section_heading(&heading_stack);
                let hp = heading_path_strings(&heading_stack);
                let pi = para_index;
                for s in split_sentences(&clean) {
                    sentences.push(IndexedSentence { text: s, para_index: pi, section_heading: sh.clone(), heading_path: hp.clone() });
                }
                para_index += 1;
            }
        }
    }
    flush(&mut sentences, &mut result, &mut chunk_index, sentences_per_chunk, total);
    if result.is_empty() { return Err("No sentence chunks generated".to_string()); }
    Ok(result)
}

#[pyfunction]
pub fn chunk_html_sentence(py: Python<'_>, file_path: &str, sentences_per_chunk: usize) -> PyResult<PyObject> {
    if !file_path.to_ascii_lowercase().ends_with(".html") && !file_path.to_ascii_lowercase().ends_with(".htm") {
        return Err(PyValueError::new_err(format!("Expected .html/.htm, got: {file_path}")));
    }
    if sentences_per_chunk == 0 { return Err(PyValueError::new_err("sentences_per_chunk must be > 0")); }
    let bytes = fs::read(file_path).map_err(|e| PyIOError::new_err(format!("{e}")))?;
    let rust_start = Instant::now();
    let chunks_raw = build_sentence_chunks(&bytes, sentences_per_chunk).map_err(PyRuntimeError::new_err)?;
    let rust_ms = rust_start.elapsed().as_secs_f64() * 1000.0;
    let chunk_list: Vec<PyObject> = chunks_raw.into_iter().map(|c| {
        let dict = PyDict::new_bound(py); dict.set_item("content", &c.content)?;
        dict.set_item("content_type", c.content_type.as_str())?;
        dict.set_item("metadata", pythonize(py, &c.metadata)?)?;
        Ok(dict.into_any().unbind())
    }).collect::<PyResult<_>>()?;
    let result = PyDict::new_bound(py);
    result.set_item("chunks", chunk_list)?; result.set_item("rust_ms", (rust_ms*1000.0).round()/1000.0)?;
    Ok(result.into_any().unbind())
}
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(chunk_html_sentence, m)?)?; Ok(())
}
