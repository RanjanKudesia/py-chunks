//! `.msg` chunking + markdown pyfunctions.
//!
//! Pipeline: .msg --cfb extract--> markdown --existing Markdown chunker--> chunks.
//! Mirrors the PDF (liteparse→md) bridge so msg chunks share the Markdown schema.

use std::time::Instant;

use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};
use pyo3::wrap_pyfunction;
use pythonize::pythonize;

use super::extract::{document_to_markdown, extract_document, MsgDocument};
use crate::extensions::md::common::ChunkRecordInput;
use crate::extensions::md::page_aware::build_page_aware_chunks;
use crate::extensions::md::section::build_section_chunks;
use crate::extensions::md::semantic::build_semantic_chunks;
use crate::extensions::md::sentence::build_sentence_chunks;
use crate::extensions::md::sliding_window::build_sliding_window_chunks;
use crate::extensions::md::structural::build_chunks_from_md_bytes;

fn ensure_msg(file_path: &str) -> PyResult<()> {
    if !file_path.to_ascii_lowercase().ends_with(".msg") {
        return Err(PyValueError::new_err(format!(
            "Expected .msg file path, got: {file_path}"
        )));
    }
    Ok(())
}

fn record_to_pydict(py: Python<'_>, rec: &ChunkRecordInput) -> PyResult<PyObject> {
    let dict = PyDict::new_bound(py);
    dict.set_item("content", &rec.content)?;
    dict.set_item("content_type", rec.content_type.as_str())?;
    dict.set_item("metadata", pythonize(py, &rec.metadata)?)?;
    Ok(dict.into_any().unbind())
}

fn inject_msg_metadata(records: &mut [ChunkRecordInput], doc: &MsgDocument) {
    let meta = serde_json::json!({
        "source_type": "msg",
        "message_class": doc.message_class,
        "subject": doc.subject,
        "from": doc.from,
        "to": doc.to,
        "cc": doc.cc,
        "bcc": doc.bcc,
        "sent_date": doc.sent_date,
        "received_date": doc.received_date,
        "importance": doc.importance,
        "conversation_topic": doc.conversation_topic,
        "has_attachments": !doc.attachments.is_empty(),
        "attachment_count": doc.attachments.len(),
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
    match mode {
        "default" | "structural" => build_chunks_from_md_bytes(markdown),
        "section" => build_section_chunks(markdown),
        "semantic" => build_semantic_chunks(markdown),
        "sentence" => build_sentence_chunks(markdown, sentences_per_chunk),
        "page_aware" => build_page_aware_chunks(markdown, paragraphs_per_page),
        "sliding_window" => build_sliding_window_chunks(markdown, window_size, overlap),
        other => Err(format!("Unknown MSG mode: {other}")),
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
    ensure_msg(file_path)?;
    let start = Instant::now();
    let doc = extract_document(file_path).map_err(PyRuntimeError::new_err)?;
    let markdown = document_to_markdown(&doc);
    let mut records = chunks_for_mode(
        markdown.as_bytes(),
        mode,
        window_size,
        overlap,
        sentences_per_chunk,
        paragraphs_per_page,
    )
    .map_err(PyRuntimeError::new_err)?;
    inject_msg_metadata(&mut records, &doc);
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
fn chunk_msg(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    run_mode(py, file_path, "default", 3, 1, 3, 15)
}
#[pyfunction]
fn chunk_msg_section(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    run_mode(py, file_path, "section", 3, 1, 3, 15)
}
#[pyfunction]
fn chunk_msg_semantic(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    run_mode(py, file_path, "semantic", 3, 1, 3, 15)
}
#[pyfunction]
fn chunk_msg_sentence(py: Python<'_>, file_path: &str, sentences_per_chunk: usize) -> PyResult<PyObject> {
    run_mode(py, file_path, "sentence", 3, 1, sentences_per_chunk, 15)
}
#[pyfunction]
fn chunk_msg_page_aware(py: Python<'_>, file_path: &str, paragraphs_per_page: usize) -> PyResult<PyObject> {
    run_mode(py, file_path, "page_aware", 3, 1, 3, paragraphs_per_page)
}
#[pyfunction]
fn chunk_msg_sliding_window(py: Python<'_>, file_path: &str, window_size: usize, overlap: usize) -> PyResult<PyObject> {
    run_mode(py, file_path, "sliding_window", window_size, overlap, 3, 15)
}

#[pyfunction]
fn msg_to_markdown(file_path: &str) -> PyResult<String> {
    ensure_msg(file_path)?;
    let doc = extract_document(file_path).map_err(PyRuntimeError::new_err)?;
    Ok(document_to_markdown(&doc))
}

#[pyclass]
pub struct MsgStreamIterator {
    chunks: std::vec::IntoIter<PyObject>,
}

#[pymethods]
impl MsgStreamIterator {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }
    fn __next__(&mut self, _py: Python<'_>) -> Option<PyObject> {
        self.chunks.next()
    }
}

#[pyfunction]
#[pyo3(signature = (file_path, mode, window_size, overlap, sentences_per_chunk, paragraphs_per_page))]
fn stream_msg_chunks(
    py: Python<'_>,
    file_path: &str,
    mode: &str,
    window_size: usize,
    overlap: usize,
    sentences_per_chunk: usize,
    paragraphs_per_page: usize,
) -> PyResult<Py<MsgStreamIterator>> {
    ensure_msg(file_path)?;
    let doc = extract_document(file_path).map_err(PyRuntimeError::new_err)?;
    let markdown = document_to_markdown(&doc);
    let mut records = chunks_for_mode(
        markdown.as_bytes(),
        mode,
        window_size,
        overlap,
        sentences_per_chunk,
        paragraphs_per_page,
    )
    .map_err(PyRuntimeError::new_err)?;
    inject_msg_metadata(&mut records, &doc);
    let chunk_vec: Vec<PyObject> = records
        .iter()
        .map(|r| record_to_pydict(py, r))
        .collect::<PyResult<_>>()?;
    Py::new(
        py,
        MsgStreamIterator {
            chunks: chunk_vec.into_iter(),
        },
    )
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(chunk_msg, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_msg_section, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_msg_semantic, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_msg_sentence, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_msg_page_aware, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_msg_sliding_window, m)?)?;
    m.add_function(wrap_pyfunction!(msg_to_markdown, m)?)?;
    m.add_function(wrap_pyfunction!(stream_msg_chunks, m)?)?;
    Ok(())
}
