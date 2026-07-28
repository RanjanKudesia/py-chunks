//! `.rtf` chunking + markdown pyfunctions.
//!
//! Pipeline: .rtf --hand-rolled extractor--> markdown --Markdown chunker--> chunks.

use std::time::Instant;

use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};
use pyo3::wrap_pyfunction;
use pythonize::pythonize;

use super::extract::{extract, to_markdown, RtfDoc};
use crate::extensions::md::common::ChunkRecordInput;
use crate::extensions::md::page_aware::build_page_aware_chunks;
use crate::extensions::md::section::build_section_chunks;
use crate::extensions::md::semantic::build_semantic_chunks;
use crate::extensions::md::sentence::build_sentence_chunks;
use crate::extensions::md::sliding_window::build_sliding_window_chunks;
use crate::extensions::md::structural::build_chunks_from_md_bytes;

fn ensure_rtf(file_path: &str) -> PyResult<()> {
    if !file_path.to_ascii_lowercase().ends_with(".rtf") {
        return Err(PyValueError::new_err(format!(
            "Expected .rtf file path, got: {file_path}"
        )));
    }
    Ok(())
}

fn load(file_path: &str) -> PyResult<RtfDoc> {
    let bytes = std::fs::read(file_path)
        .map_err(|e| PyIOError::new_err(format!("Failed to read RTF file: {e}")))?;
    Ok(extract(&bytes))
}

fn record_to_pydict(py: Python<'_>, rec: &ChunkRecordInput) -> PyResult<PyObject> {
    let dict = PyDict::new_bound(py);
    dict.set_item("content", &rec.content)?;
    dict.set_item("content_type", rec.content_type.as_str())?;
    dict.set_item("metadata", pythonize(py, &rec.metadata)?)?;
    Ok(dict.into_any().unbind())
}

fn inject_metadata(records: &mut [ChunkRecordInput], doc: &RtfDoc) {
    let meta = serde_json::json!({
        "source_type": "rtf",
        "title": doc.title,
        "author": doc.author,
    });
    for rec in records.iter_mut() {
        if let serde_json::Value::Object(map) = &mut rec.metadata {
            map.insert("document_metadata".to_string(), meta.clone());
        }
    }
}

fn chunks_for_mode(
    markdown: &[u8],
    mode: &str,
    window_size: usize,
    overlap: usize,
    sentences_per_chunk: usize,
    paragraphs_per_page: usize,
) -> Result<Vec<ChunkRecordInput>, String> {
    // Empty document (e.g. an RTF that is only fonts/objects/metadata) → no chunks,
    // rather than erroring in the Markdown chunker.
    if markdown.iter().all(|b| b.is_ascii_whitespace()) {
        return Ok(Vec::new());
    }
    match mode {
        "default" | "structural" => build_chunks_from_md_bytes(markdown),
        "section" => build_section_chunks(markdown),
        "semantic" => build_semantic_chunks(markdown),
        "sentence" => build_sentence_chunks(markdown, sentences_per_chunk),
        "page_aware" => build_page_aware_chunks(markdown, paragraphs_per_page),
        "sliding_window" => build_sliding_window_chunks(markdown, window_size, overlap),
        other => Err(format!("Unknown RTF mode: {other}")),
    }
}

fn run_mode(
    py: Python<'_>,
    file_path: &str,
    mode: &str,
    window_size: usize,
    overlap: usize,
    sentences_per_chunk: usize,
    paragraphs_per_page: usize,
) -> PyResult<PyObject> {
    ensure_rtf(file_path)?;
    let start = Instant::now();
    let doc = load(file_path)?;
    let markdown = to_markdown(&doc);
    let mut records = chunks_for_mode(
        markdown.as_bytes(),
        mode,
        window_size,
        overlap,
        sentences_per_chunk,
        paragraphs_per_page,
    )
    .map_err(PyRuntimeError::new_err)?;
    inject_metadata(&mut records, &doc);
    let rust_ms = start.elapsed().as_secs_f64() * 1000.0;

    let chunk_list: Vec<PyObject> = records
        .iter()
        .map(|r| record_to_pydict(py, r))
        .collect::<PyResult<_>>()?;
    let result = PyDict::new_bound(py);
    result.set_item("chunks", chunk_list)?;
    result.set_item("rust_ms", (rust_ms * 1000.0).round() / 1000.0)?;
    Ok(result.into_any().unbind())
}

#[pyfunction]
fn chunk_rtf(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    run_mode(py, file_path, "default", 3, 1, 3, 15)
}
#[pyfunction]
fn chunk_rtf_section(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    run_mode(py, file_path, "section", 3, 1, 3, 15)
}
#[pyfunction]
fn chunk_rtf_semantic(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    run_mode(py, file_path, "semantic", 3, 1, 3, 15)
}
#[pyfunction]
fn chunk_rtf_sentence(py: Python<'_>, file_path: &str, sentences_per_chunk: usize) -> PyResult<PyObject> {
    run_mode(py, file_path, "sentence", 3, 1, sentences_per_chunk, 15)
}
#[pyfunction]
fn chunk_rtf_page_aware(py: Python<'_>, file_path: &str, paragraphs_per_page: usize) -> PyResult<PyObject> {
    run_mode(py, file_path, "page_aware", 3, 1, 3, paragraphs_per_page)
}
#[pyfunction]
fn chunk_rtf_sliding_window(py: Python<'_>, file_path: &str, window_size: usize, overlap: usize) -> PyResult<PyObject> {
    run_mode(py, file_path, "sliding_window", window_size, overlap, 3, 15)
}

#[pyfunction]
fn rtf_to_markdown(file_path: &str) -> PyResult<String> {
    ensure_rtf(file_path)?;
    let doc = load(file_path)?;
    Ok(to_markdown(&doc))
}

#[pyclass]
pub struct RtfStreamIterator {
    chunks: std::vec::IntoIter<PyObject>,
}

#[pymethods]
impl RtfStreamIterator {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }
    fn __next__(&mut self, _py: Python<'_>) -> Option<PyObject> {
        self.chunks.next()
    }
}

#[pyfunction]
#[pyo3(signature = (file_path, mode, window_size, overlap, sentences_per_chunk, paragraphs_per_page))]
fn stream_rtf_chunks(
    py: Python<'_>,
    file_path: &str,
    mode: &str,
    window_size: usize,
    overlap: usize,
    sentences_per_chunk: usize,
    paragraphs_per_page: usize,
) -> PyResult<Py<RtfStreamIterator>> {
    ensure_rtf(file_path)?;
    let doc = load(file_path)?;
    let markdown = to_markdown(&doc);
    let mut records = chunks_for_mode(
        markdown.as_bytes(),
        mode,
        window_size,
        overlap,
        sentences_per_chunk,
        paragraphs_per_page,
    )
    .map_err(PyRuntimeError::new_err)?;
    inject_metadata(&mut records, &doc);
    let chunk_vec: Vec<PyObject> = records
        .iter()
        .map(|r| record_to_pydict(py, r))
        .collect::<PyResult<_>>()?;
    Py::new(
        py,
        RtfStreamIterator {
            chunks: chunk_vec.into_iter(),
        },
    )
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(chunk_rtf, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_rtf_section, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_rtf_semantic, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_rtf_sentence, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_rtf_page_aware, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_rtf_sliding_window, m)?)?;
    m.add_function(wrap_pyfunction!(rtf_to_markdown, m)?)?;
    m.add_function(wrap_pyfunction!(stream_rtf_chunks, m)?)?;
    Ok(())
}
