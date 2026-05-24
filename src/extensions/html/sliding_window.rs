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
    update_heading_stack, ChunkRecordInput, ContentType, HtmlBlockType,
};

struct BlockUnit { content: String, section_heading: Option<String>, heading_path: Vec<String> }

pub fn build_sliding_window_chunks(bytes: &[u8], window_size: usize, overlap: usize) -> Result<Vec<ChunkRecordInput>, String> {
    if window_size == 0 { return Err("window_size must be > 0".to_string()); }
    if overlap >= window_size { return Err("overlap must be < window_size".to_string()); }
    let text = std::str::from_utf8(bytes).map(|s| s.to_string()).unwrap_or_else(|_| String::from_utf8_lossy(bytes).to_string());
    if text.trim().is_empty() { return Err("HTML file is empty".to_string()); }
    let blocks = parse_html_blocks(&remove_comments(&text));
    let total = blocks.len();
    let mut heading_stack: Vec<(u8, String)> = Vec::new();
    let mut units: Vec<BlockUnit> = Vec::new();
    for block in &blocks {
        if block.block_type == HtmlBlockType::Heading {
            update_heading_stack(&mut heading_stack, block.heading_level, block.content.clone());
        }
        let content = block.content.trim().to_string();
        if content.is_empty() { continue; }
        units.push(BlockUnit { content, section_heading: current_section_heading(&heading_stack), heading_path: heading_path_strings(&heading_stack) });
    }
    if units.is_empty() { return Err("No content blocks found".to_string()); }
    let step = window_size - overlap;
    let mut result: Vec<ChunkRecordInput> = Vec::new();
    let mut start = 0usize; let mut window_index = 0usize;
    loop {
        let end = (start + window_size).min(units.len());
        let window = &units[start..end];
        let content = window.iter().map(|u| u.content.as_str()).collect::<Vec<_>>().join("\n\n");
        if !content.is_empty() {
            result.push(ChunkRecordInput {
                content_type: ContentType::SlidingWindow, content,
                metadata: json!({
                    "window_size": window_size, "overlap": overlap, "window_index": window_index,
                    "paragraph_range": [start, end.saturating_sub(1)], "paragraph_count": window.len(),
                    "section_heading": window[0].section_heading, "heading_path": window[0].heading_path,
                    "chunk_index": window_index, "document_metadata": { "source_type": "html", "total_input_blocks": total }
                }),
            });
            window_index += 1;
        }
        if end >= units.len() { break; }
        start += step;
    }
    if result.is_empty() { return Err("No sliding-window chunks generated".to_string()); }
    Ok(result)
}

#[pyfunction]
pub fn chunk_html_sliding_window(py: Python<'_>, file_path: &str, window_size: usize, overlap: usize) -> PyResult<PyObject> {
    if !file_path.to_ascii_lowercase().ends_with(".html") && !file_path.to_ascii_lowercase().ends_with(".htm") {
        return Err(PyValueError::new_err(format!("Expected .html/.htm, got: {file_path}")));
    }
    if window_size == 0 { return Err(PyValueError::new_err("window_size must be > 0")); }
    if overlap >= window_size { return Err(PyValueError::new_err("overlap must be < window_size")); }
    let bytes = fs::read(file_path).map_err(|e| PyIOError::new_err(format!("{e}")))?;
    let rust_start = Instant::now();
    let chunks_raw = build_sliding_window_chunks(&bytes, window_size, overlap).map_err(PyRuntimeError::new_err)?;
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
    m.add_function(wrap_pyfunction!(chunk_html_sliding_window, m)?)?; Ok(())
}
