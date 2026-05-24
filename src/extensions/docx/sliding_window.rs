use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};
use pyo3::wrap_pyfunction;
use pythonize::pythonize;
use serde_json::{json, Value};

use super::common::{parse_docx_indexed_paragraphs, IndexedParagraph};
use std::fs;
use std::time::Instant;

#[derive(Debug, Clone)]
struct ChunkRecordInput {
    content: String,
    metadata: Value,
}

#[pyfunction]
fn chunk_docx_sliding_window(
    py: Python<'_>,
    file_path: &str,
    window_size: usize,
    overlap: usize,
) -> PyResult<PyObject> {
    if !file_path.to_ascii_lowercase().ends_with(".docx") {
        return Err(PyValueError::new_err(format!(
            "Expected .docx file path, got: {file_path}"
        )));
    }
    if window_size == 0 {
        return Err(PyValueError::new_err("window_size must be greater than 0"));
    }
    if overlap >= window_size {
        return Err(PyValueError::new_err(
            "overlap must be less than window_size",
        ));
    }

    let bytes = fs::read(file_path)
        .map_err(|e| PyIOError::new_err(format!("Failed to read DOCX file: {e}")))?;

    let rust_start = Instant::now();
    let paragraphs = parse_docx_indexed_paragraphs(&bytes)
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to parse DOCX: {e}")))?;
    let chunks_raw = build_sliding_window_chunks(paragraphs, window_size, overlap);
    let rust_ms = rust_start.elapsed().as_secs_f64() * 1000.0;

    let chunk_list: Vec<PyObject> = chunks_raw
        .into_iter()
        .map(|c| {
            let dict = PyDict::new_bound(py);
            dict.set_item("content", &c.content)?;
            dict.set_item("content_type", "sliding_window")?;
            dict.set_item("metadata", pythonize(py, &c.metadata)?)?;
            Ok(dict.into_any().unbind())
        })
        .collect::<PyResult<_>>()?;

    let result = PyDict::new_bound(py);
    result.set_item("chunks", chunk_list)?;
    result.set_item("rust_ms", (rust_ms * 1000.0).round() / 1000.0)?;
    Ok(result.into_any().unbind())
}

fn build_sliding_window_chunks(
    paragraphs: Vec<IndexedParagraph>,
    window_size: usize,
    overlap: usize,
) -> Vec<ChunkRecordInput> {
    if paragraphs.is_empty() {
        return Vec::new();
    }

    let step = window_size - overlap;
    let mut chunks = Vec::new();
    let mut start = 0usize;
    let mut window_index = 0usize;

    while start < paragraphs.len() {
        let end = (start + window_size).min(paragraphs.len());
        let window = &paragraphs[start..end];
        let content = window
            .iter()
            .map(|paragraph| paragraph.text.clone())
            .collect::<Vec<_>>()
            .join("\n\n");
        let paragraph_indices: Vec<usize> =
            window.iter().map(|paragraph| paragraph.index).collect();
        let paragraph_meta: Vec<Value> = window
            .iter()
            .map(|p| {
                json!({
                    "index": p.index,
                    "is_heading": p.is_heading,
                    "heading_level": p.heading_level,
                    "is_list": p.is_list,
                    "is_table": p.is_table,
                })
            })
            .collect();
        let heading_count = window.iter().filter(|p| p.is_heading).count();
        let list_count = window.iter().filter(|p| p.is_list).count();

        chunks.push(ChunkRecordInput {
            content,
            metadata: json!({
                "window_size": window_size,
                "overlap": overlap,
                "window_index": window_index,
                "paragraph_indices": paragraph_indices,
                "paragraph_meta": paragraph_meta,
                "heading_count": heading_count,
                "list_item_count": list_count,
                "document_metadata": {
                    "source_type": "docx"
                }
            }),
        });

        if end == paragraphs.len() {
            break;
        }

        start += step;
        window_index += 1;
    }

    chunks
}

#[pyclass]
pub struct DocxSlidingWindowIterator {
    chunks: Vec<ChunkRecordInput>,
    index: usize,
}

#[pymethods]
impl DocxSlidingWindowIterator {
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
        dict.set_item("content_type", "sliding_window")?;
        dict.set_item("metadata", pythonize(py, &chunk.metadata)?)?;
        Ok(Some(dict.into_any().unbind()))
    }
}

#[pyfunction]
fn chunk_docx_sliding_window_stream(
    file_path: &str,
    window_size: usize,
    overlap: usize,
) -> PyResult<DocxSlidingWindowIterator> {
    if !file_path.to_ascii_lowercase().ends_with(".docx") {
        return Err(PyValueError::new_err(format!(
            "Expected .docx file path, got: {file_path}"
        )));
    }
    if window_size == 0 {
        return Err(PyValueError::new_err("window_size must be greater than 0"));
    }
    if overlap >= window_size {
        return Err(PyValueError::new_err(
            "overlap must be less than window_size",
        ));
    }

    let bytes = fs::read(file_path)
        .map_err(|e| PyIOError::new_err(format!("Failed to read DOCX file: {e}")))?;

    let paragraphs = parse_docx_indexed_paragraphs(&bytes)
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to parse DOCX: {e}")))?;
    let chunks = build_sliding_window_chunks(paragraphs, window_size, overlap);

    Ok(DocxSlidingWindowIterator { chunks, index: 0 })
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(chunk_docx_sliding_window, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_docx_sliding_window_stream, m)?)?;
    m.add_class::<DocxSlidingWindowIterator>()?;
    Ok(())
}
