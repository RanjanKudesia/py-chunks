//! `.json` / `.jsonl` / `.ndjson` chunking + markdown pyfunctions.
//! Pipeline mirrors `.eml`/`.odt`: JSON → record markdown → Markdown chunker.

use std::time::Instant;

use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};
use pyo3::wrap_pyfunction;
use pythonize::pythonize;
use serde_json::json;

use super::extract::{parse_document, parse_lines, JsonDoc, JsonKind};
use crate::extensions::md::common::ChunkRecordInput;
use crate::extensions::md::page_aware::build_page_aware_chunks;
use crate::extensions::md::section::build_section_chunks;
use crate::extensions::md::semantic::build_semantic_chunks;
use crate::extensions::md::sentence::build_sentence_chunks;
use crate::extensions::md::sliding_window::build_sliding_window_chunks;
use crate::extensions::md::structural::build_chunks_from_md_bytes;

fn kind_for(file_path: &str) -> PyResult<JsonKind> {
    let lower = file_path.to_ascii_lowercase();
    if lower.ends_with(".jsonl") || lower.ends_with(".ndjson") {
        Ok(JsonKind::Lines)
    } else if lower.ends_with(".json") {
        Ok(JsonKind::Document)
    } else {
        Err(PyValueError::new_err(format!(
            "Expected .json, .jsonl or .ndjson file path, got: {file_path}"
        )))
    }
}

fn source_type(file_path: &str) -> &'static str {
    let lower = file_path.to_ascii_lowercase();
    if lower.ends_with(".ndjson") {
        "ndjson"
    } else if lower.ends_with(".jsonl") {
        "jsonl"
    } else {
        "json"
    }
}

struct Loaded {
    markdown: String,
    metadata: serde_json::Value,
}

fn load_json(file_path: &str) -> PyResult<Loaded> {
    let kind = kind_for(file_path)?;
    let raw = std::fs::read(file_path)
        .map_err(|e| PyIOError::new_err(format!("Failed to read JSON file: {e}")))?;
    let doc: JsonDoc = match kind {
        JsonKind::Lines => parse_lines(&raw),
        JsonKind::Document => parse_document(&raw).map_err(PyRuntimeError::new_err)?,
    };
    let metadata = json!({
        "source_type": source_type(file_path),
        "record_count": doc.record_count,
        "top_level": doc.top_level,
        "envelope_key": doc.envelope_key,
    });
    Ok(Loaded { markdown: doc.markdown, metadata })
}

fn chunks_for_mode(
    markdown: &[u8],
    mode: &str,
    window_size: usize,
    overlap: usize,
    sentences_per_chunk: usize,
    paragraphs_per_page: usize,
) -> Result<Vec<ChunkRecordInput>, String> {
    match mode {
        "default" | "structural" => build_chunks_from_md_bytes(markdown),
        "section" => build_section_chunks(markdown),
        "semantic" => build_semantic_chunks(markdown),
        "sentence" => build_sentence_chunks(markdown, sentences_per_chunk),
        "page_aware" => build_page_aware_chunks(markdown, paragraphs_per_page),
        "sliding_window" => build_sliding_window_chunks(markdown, window_size, overlap),
        other => Err(format!("Unknown JSON mode: {other}")),
    }
}

fn inject_metadata(records: &mut [ChunkRecordInput], meta: &serde_json::Value) {
    for rec in records.iter_mut() {
        if let serde_json::Value::Object(map) = &mut rec.metadata {
            map.insert("document_metadata".to_string(), meta.clone());
        }
    }
}

fn record_to_pydict(py: Python<'_>, rec: &ChunkRecordInput) -> PyResult<PyObject> {
    let dict = PyDict::new_bound(py);
    dict.set_item("content", &rec.content)?;
    dict.set_item("content_type", rec.content_type.as_str())?;
    dict.set_item("metadata", pythonize(py, &rec.metadata)?)?;
    Ok(dict.into_any().unbind())
}

fn build_records(
    loaded: &Loaded,
    mode: &str,
    window_size: usize,
    overlap: usize,
    sentences_per_chunk: usize,
    paragraphs_per_page: usize,
) -> PyResult<Vec<ChunkRecordInput>> {
    if loaded.markdown.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut records = chunks_for_mode(
        loaded.markdown.as_bytes(),
        mode,
        window_size,
        overlap,
        sentences_per_chunk,
        paragraphs_per_page,
    )
    .map_err(PyRuntimeError::new_err)?;
    inject_metadata(&mut records, &loaded.metadata);
    Ok(records)
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
    let start = Instant::now();
    let loaded = load_json(file_path)?;
    let records = build_records(
        &loaded, mode, window_size, overlap, sentences_per_chunk, paragraphs_per_page,
    )?;
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
fn chunk_json(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    run_mode(py, file_path, "default", 3, 1, 3, 15)
}
#[pyfunction]
fn chunk_json_section(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    run_mode(py, file_path, "section", 3, 1, 3, 15)
}
#[pyfunction]
fn chunk_json_semantic(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    run_mode(py, file_path, "semantic", 3, 1, 3, 15)
}
#[pyfunction]
fn chunk_json_sentence(py: Python<'_>, file_path: &str, sentences_per_chunk: usize) -> PyResult<PyObject> {
    run_mode(py, file_path, "sentence", 3, 1, sentences_per_chunk, 15)
}
#[pyfunction]
fn chunk_json_page_aware(py: Python<'_>, file_path: &str, paragraphs_per_page: usize) -> PyResult<PyObject> {
    run_mode(py, file_path, "page_aware", 3, 1, 3, paragraphs_per_page)
}
#[pyfunction]
fn chunk_json_sliding_window(py: Python<'_>, file_path: &str, window_size: usize, overlap: usize) -> PyResult<PyObject> {
    run_mode(py, file_path, "sliding_window", window_size, overlap, 3, 15)
}

#[pyfunction]
fn json_to_markdown(file_path: &str) -> PyResult<String> {
    Ok(load_json(file_path)?.markdown)
}

#[pyclass]
pub struct JsonStreamIterator {
    chunks: std::vec::IntoIter<PyObject>,
}

#[pymethods]
impl JsonStreamIterator {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }
    fn __next__(&mut self, _py: Python<'_>) -> Option<PyObject> {
        self.chunks.next()
    }
}

#[pyfunction]
#[pyo3(signature = (file_path, mode, window_size, overlap, sentences_per_chunk, paragraphs_per_page))]
fn stream_json_chunks(
    py: Python<'_>,
    file_path: &str,
    mode: &str,
    window_size: usize,
    overlap: usize,
    sentences_per_chunk: usize,
    paragraphs_per_page: usize,
) -> PyResult<Py<JsonStreamIterator>> {
    let loaded = load_json(file_path)?;
    let records = build_records(
        &loaded, mode, window_size, overlap, sentences_per_chunk, paragraphs_per_page,
    )?;
    let chunk_vec: Vec<PyObject> = records
        .iter()
        .map(|r| record_to_pydict(py, r))
        .collect::<PyResult<_>>()?;
    Py::new(py, JsonStreamIterator { chunks: chunk_vec.into_iter() })
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(chunk_json, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_json_section, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_json_semantic, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_json_sentence, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_json_page_aware, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_json_sliding_window, m)?)?;
    m.add_function(wrap_pyfunction!(json_to_markdown, m)?)?;
    m.add_function(wrap_pyfunction!(stream_json_chunks, m)?)?;
    Ok(())
}
