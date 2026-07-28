//! `.ipynb` chunking + markdown + image pyfunctions.

use std::time::Instant;

use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyModule};
use pyo3::wrap_pyfunction;
use pythonize::pythonize;

use super::extract::{extract, to_markdown, IpynbDoc};
use crate::extensions::md::common::ChunkRecordInput;
use crate::extensions::md::page_aware::build_page_aware_chunks;
use crate::extensions::md::section::build_section_chunks;
use crate::extensions::md::semantic::build_semantic_chunks;
use crate::extensions::md::sentence::build_sentence_chunks;
use crate::extensions::md::sliding_window::build_sliding_window_chunks;
use crate::extensions::md::structural::build_chunks_from_md_bytes;

fn ensure_ipynb(file_path: &str) -> PyResult<()> {
    if !file_path.to_ascii_lowercase().ends_with(".ipynb") {
        return Err(PyValueError::new_err(format!(
            "Expected .ipynb file path, got: {file_path}"
        )));
    }
    Ok(())
}

fn load(file_path: &str) -> PyResult<IpynbDoc> {
    let bytes = std::fs::read(file_path)
        .map_err(|e| PyIOError::new_err(format!("Failed to read notebook file: {e}")))?;
    extract(&bytes).map_err(PyRuntimeError::new_err)
}

fn record_to_pydict(py: Python<'_>, rec: &ChunkRecordInput) -> PyResult<PyObject> {
    let dict = PyDict::new_bound(py);
    dict.set_item("content", &rec.content)?;
    dict.set_item("content_type", rec.content_type.as_str())?;
    dict.set_item("metadata", pythonize(py, &rec.metadata)?)?;
    Ok(dict.into_any().unbind())
}

fn inject_metadata(records: &mut [ChunkRecordInput], doc: &IpynbDoc) {
    let meta = serde_json::json!({
        "source_type": "ipynb",
        "language": doc.language,
        "kernel": doc.kernel,
        "nbformat": doc.nbformat,
        "cell_count": doc.cell_count,
        "code_cell_count": doc.code_cell_count,
        "markdown_cell_count": doc.markdown_cell_count,
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
    if markdown.iter().all(|b| b.is_ascii_whitespace()) {
        return Ok(Vec::new());
    }
    let result = match mode {
        "default" | "structural" => build_chunks_from_md_bytes(markdown),
        "section" => build_section_chunks(markdown),
        "semantic" => build_semantic_chunks(markdown),
        "sentence" => build_sentence_chunks(markdown, sentences_per_chunk),
        "page_aware" => build_page_aware_chunks(markdown, paragraphs_per_page),
        "sliding_window" => build_sliding_window_chunks(markdown, window_size, overlap),
        other => return Err(format!("Unknown notebook mode: {other}")),
    };
    // An image-only notebook (only image refs, no prose) yields no text chunks —
    // treat the MD chunker's empty/no-chunks signal as an empty result.
    match result {
        Err(e) if e.contains("empty") || e.contains("No chunks") => Ok(Vec::new()),
        other => other,
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
    ensure_ipynb(file_path)?;
    let start = Instant::now();
    let doc = load(file_path)?;
    let mut records = chunks_for_mode(
        doc.markdown.as_bytes(),
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
fn chunk_ipynb(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    run_mode(py, file_path, "default", 3, 1, 3, 15)
}
#[pyfunction]
fn chunk_ipynb_section(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    run_mode(py, file_path, "section", 3, 1, 3, 15)
}
#[pyfunction]
fn chunk_ipynb_semantic(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    run_mode(py, file_path, "semantic", 3, 1, 3, 15)
}
#[pyfunction]
fn chunk_ipynb_sentence(py: Python<'_>, file_path: &str, sentences_per_chunk: usize) -> PyResult<PyObject> {
    run_mode(py, file_path, "sentence", 3, 1, sentences_per_chunk, 15)
}
#[pyfunction]
fn chunk_ipynb_page_aware(py: Python<'_>, file_path: &str, paragraphs_per_page: usize) -> PyResult<PyObject> {
    run_mode(py, file_path, "page_aware", 3, 1, 3, paragraphs_per_page)
}
#[pyfunction]
fn chunk_ipynb_sliding_window(py: Python<'_>, file_path: &str, window_size: usize, overlap: usize) -> PyResult<PyObject> {
    run_mode(py, file_path, "sliding_window", window_size, overlap, 3, 15)
}

#[pyfunction]
fn ipynb_to_markdown(file_path: &str) -> PyResult<String> {
    ensure_ipynb(file_path)?;
    Ok(to_markdown(&load(file_path)?))
}

#[pyfunction]
fn ipynb_to_markdown_with_images(
    py: Python<'_>,
    file_path: &str,
) -> PyResult<(String, Vec<(String, Py<PyBytes>)>)> {
    ensure_ipynb(file_path)?;
    let doc = load(file_path)?;
    let images = doc
        .images
        .iter()
        .map(|(n, b)| (n.clone(), PyBytes::new_bound(py, b).unbind()))
        .collect();
    Ok((to_markdown(&doc), images))
}

#[pyfunction]
#[pyo3(signature = (file_path, mode, rows_per_chunk=1, window_size=3, overlap=1, sentences_per_chunk=3, paragraphs_per_page=15, max_chunk_chars=2000))]
#[allow(clippy::too_many_arguments)]
fn chunk_ipynb_with_images(
    py: Python<'_>,
    file_path: &str,
    mode: &str,
    rows_per_chunk: usize,
    window_size: usize,
    overlap: usize,
    sentences_per_chunk: usize,
    paragraphs_per_page: usize,
    max_chunk_chars: usize,
) -> PyResult<(Vec<PyObject>, Vec<(String, Py<PyBytes>)>)> {
    ensure_ipynb(file_path)?;
    let _ = (rows_per_chunk, max_chunk_chars);
    let normalized_mode = if mode == "default" { "default" } else { mode };
    let doc = load(file_path)?;
    let records = chunks_for_mode(
        doc.markdown.as_bytes(),
        normalized_mode,
        window_size,
        overlap,
        sentences_per_chunk,
        paragraphs_per_page,
    )
    .map_err(PyRuntimeError::new_err)?;

    let mut chunk_list: Vec<PyObject> = doc
        .images
        .iter()
        .map(|(name, _)| {
            let dict = PyDict::new_bound(py);
            dict.set_item("content", name)?;
            dict.set_item("content_type", "image")?;
            let meta = PyDict::new_bound(py);
            meta.set_item("image_name", name)?;
            dict.set_item("metadata", meta)?;
            Ok(dict.into_any().unbind())
        })
        .collect::<PyResult<_>>()?;
    let mut text_records = records;
    inject_metadata(&mut text_records, &doc);
    for rec in &text_records {
        chunk_list.push(record_to_pydict(py, rec)?);
    }
    let images = doc
        .images
        .into_iter()
        .map(|(n, b)| (n, PyBytes::new_bound(py, &b).unbind()))
        .collect();
    Ok((chunk_list, images))
}

#[pyclass]
pub struct IpynbStreamIterator {
    chunks: std::vec::IntoIter<PyObject>,
}

#[pymethods]
impl IpynbStreamIterator {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }
    fn __next__(&mut self, _py: Python<'_>) -> Option<PyObject> {
        self.chunks.next()
    }
}

#[pyfunction]
#[pyo3(signature = (file_path, mode, window_size, overlap, sentences_per_chunk, paragraphs_per_page))]
fn stream_ipynb_chunks(
    py: Python<'_>,
    file_path: &str,
    mode: &str,
    window_size: usize,
    overlap: usize,
    sentences_per_chunk: usize,
    paragraphs_per_page: usize,
) -> PyResult<Py<IpynbStreamIterator>> {
    ensure_ipynb(file_path)?;
    let doc = load(file_path)?;
    let mut records = chunks_for_mode(
        doc.markdown.as_bytes(),
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
    Py::new(py, IpynbStreamIterator { chunks: chunk_vec.into_iter() })
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(chunk_ipynb, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_ipynb_section, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_ipynb_semantic, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_ipynb_sentence, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_ipynb_page_aware, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_ipynb_sliding_window, m)?)?;
    m.add_function(wrap_pyfunction!(ipynb_to_markdown, m)?)?;
    m.add_function(wrap_pyfunction!(ipynb_to_markdown_with_images, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_ipynb_with_images, m)?)?;
    m.add_function(wrap_pyfunction!(stream_ipynb_chunks, m)?)?;
    Ok(())
}
