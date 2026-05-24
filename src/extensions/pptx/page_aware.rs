/// Page-aware chunker for PPTX.
/// Groups N consecutive slides into one chunk.  Slides are already discrete
/// pages, so `paragraphs_per_page` is interpreted as `slides_per_chunk`.

use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};
use pyo3::wrap_pyfunction;
use pythonize::pythonize;
use serde_json::json;
use std::fs;
use std::time::Instant;

use super::common::{collect_slide_names, open_pptx, read_all_slides, ChunkRecordInput, ContentType};

pub fn build_page_aware_chunks(bytes: &[u8], slides_per_chunk: usize) -> Result<Vec<ChunkRecordInput>, String> {
    if slides_per_chunk == 0 { return Err("slides_per_chunk must be > 0".to_string()); }
    let mut archive = open_pptx(bytes)?;
    let slide_names = collect_slide_names(&archive);
    if slide_names.is_empty() { return Err("No slides found".to_string()); }
    let total_slides = slide_names.len();

    let mut units: Vec<(usize, String, Option<String>)> = Vec::new();
    for (slide_num, slide) in read_all_slides(&mut archive, &slide_names)? {
        let text = slide.all_text();
        if text.is_empty() { continue; }
        units.push((slide_num, text, slide.title));
    }

    if units.is_empty() { return Err("No text content found".to_string()); }

    let mut result: Vec<ChunkRecordInput> = Vec::new();
    let mut chunk_index = 0usize;
    let mut i = 0usize;
    while i < units.len() {
        let end = (i + slides_per_chunk).min(units.len());
        let window = &units[i..end];
        let content = window.iter().map(|(_, t, _)| t.as_str()).collect::<Vec<_>>().join("\n\n");
        if !content.is_empty() {
            result.push(ChunkRecordInput {
                content_type: ContentType::PageAware,
                content,
                metadata: json!({
                    "slides_per_chunk": slides_per_chunk,
                    "slide_count":      window.len(),
                    "slide_range":      [window[0].0, window.last().unwrap().0],
                    "page_break_type":  "slide_boundary",
                    "chunk_index":      chunk_index,
                    "document_metadata": { "source_type": "pptx", "total_slides": total_slides }
                }),
            });
            chunk_index += 1;
        }
        i = end;
    }

    if result.is_empty() { return Err("No page-aware chunks generated".to_string()); }
    Ok(result)
}

#[pyfunction]
pub fn chunk_pptx_page_aware(py: Python<'_>, file_path: &str, slides_per_chunk: usize) -> PyResult<PyObject> {
    if !file_path.to_ascii_lowercase().ends_with(".pptx") {
        return Err(PyValueError::new_err(format!("Expected .pptx file path, got: {file_path}")));
    }
    if slides_per_chunk == 0 { return Err(PyValueError::new_err("slides_per_chunk must be > 0")); }
    let bytes = fs::read(file_path).map_err(|e| PyIOError::new_err(format!("{e}")))?;
    let rust_start = Instant::now();
    let chunks_raw = build_page_aware_chunks(&bytes, slides_per_chunk).map_err(PyRuntimeError::new_err)?;
    let rust_ms = rust_start.elapsed().as_secs_f64() * 1000.0;
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

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(chunk_pptx_page_aware, m)?)?;
    Ok(())
}
