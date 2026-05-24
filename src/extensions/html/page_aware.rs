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

struct PageAccum { parts: Vec<String>, section_heading: Option<String>, heading_path: Vec<String>, break_type: &'static str }
impl PageAccum {
    fn new(sh: Option<String>, hp: Vec<String>, bt: &'static str) -> Self { PageAccum { parts: Vec::new(), section_heading: sh, heading_path: hp, break_type: bt } }
    fn push(&mut self, s: String) { self.parts.push(s); }
    fn len(&self) -> usize { self.parts.len() }
    fn is_empty(&self) -> bool { self.parts.is_empty() }
    fn into_content(self) -> String { self.parts.join("\n\n") }
}

pub fn build_page_aware_chunks(bytes: &[u8], paragraphs_per_page: usize) -> Result<Vec<ChunkRecordInput>, String> {
    if paragraphs_per_page == 0 { return Err("paragraphs_per_page must be > 0".to_string()); }
    let text = std::str::from_utf8(bytes).map(|s| s.to_string()).unwrap_or_else(|_| String::from_utf8_lossy(bytes).to_string());
    if text.trim().is_empty() { return Err("HTML file is empty".to_string()); }
    let blocks = parse_html_blocks(&remove_comments(&text));
    let total = blocks.len();
    let mut heading_stack: Vec<(u8, String)> = Vec::new();
    let mut result: Vec<ChunkRecordInput> = Vec::new();
    let mut accum: Option<PageAccum> = None;
    let mut chunk_index = 0usize;

    let flush = |accum: &mut Option<PageAccum>, result: &mut Vec<ChunkRecordInput>, ci: &mut usize, total: usize| {
        if let Some(a) = accum.take() {
            if !a.is_empty() {
                let pc = a.len(); let bt = a.break_type; let sh = a.section_heading.clone(); let hp = a.heading_path.clone();
                let content = a.into_content();
                result.push(ChunkRecordInput {
                    content_type: ContentType::PageAware, content,
                    metadata: json!({ "page_break_type": bt, "paragraph_count": pc, "section_heading": sh, "heading_path": hp, "chunk_index": *ci, "document_metadata": { "source_type": "html", "total_input_blocks": total } }),
                });
                *ci += 1;
            }
        }
    };

    for block in &blocks {
        if block.block_type == HtmlBlockType::Heading {
            flush(&mut accum, &mut result, &mut chunk_index, total);
            update_heading_stack(&mut heading_stack, block.heading_level, block.content.clone());
            result.push(ChunkRecordInput {
                content_type: ContentType::HeadingSection, content: block.content.clone(),
                metadata: json!({ "page_break_type": "heading_boundary", "paragraph_count": 0, "section_heading": block.content, "section_level": block.heading_level, "heading_path": heading_path_strings(&heading_stack), "chunk_index": chunk_index, "document_metadata": { "source_type": "html", "total_input_blocks": total } }),
            });
            chunk_index += 1;
            accum = Some(PageAccum::new(current_section_heading(&heading_stack), heading_path_strings(&heading_stack), "heading_boundary"));
        } else {
            let a = accum.get_or_insert_with(|| PageAccum::new(current_section_heading(&heading_stack), heading_path_strings(&heading_stack), "estimated"));
            a.push(block.content.clone());
            if a.len() >= paragraphs_per_page { flush(&mut accum, &mut result, &mut chunk_index, total); }
        }
    }
    flush(&mut accum, &mut result, &mut chunk_index, total);
    if result.is_empty() { return Err("No page-aware chunks generated".to_string()); }
    Ok(result)
}

#[pyfunction]
pub fn chunk_html_page_aware(py: Python<'_>, file_path: &str, paragraphs_per_page: usize) -> PyResult<PyObject> {
    if !file_path.to_ascii_lowercase().ends_with(".html") && !file_path.to_ascii_lowercase().ends_with(".htm") {
        return Err(PyValueError::new_err(format!("Expected .html/.htm, got: {file_path}")));
    }
    if paragraphs_per_page == 0 { return Err(PyValueError::new_err("paragraphs_per_page must be > 0")); }
    let bytes = fs::read(file_path).map_err(|e| PyIOError::new_err(format!("{e}")))?;
    let rust_start = Instant::now();
    let chunks_raw = build_page_aware_chunks(&bytes, paragraphs_per_page).map_err(PyRuntimeError::new_err)?;
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
    m.add_function(wrap_pyfunction!(chunk_html_page_aware, m)?)?; Ok(())
}
