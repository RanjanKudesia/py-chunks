use std::path::Path;
use std::time::Instant;

use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};
use pyo3::wrap_pyfunction;
use pythonize::pythonize;
use serde_json::json;

use crate::extensions::doc::structural::{
    build_page_aware_chunks, build_section_chunks, build_semantic_chunks, build_sentence_chunks,
    build_sliding_window_chunks, build_structural_chunks, chunks_to_py, ChunkRecord,
};
use crate::extensions::doc::text_extractor::DocParagraph;

use super::cfb_reader;
use super::text_extractor;

pub(crate) fn validate_ppt_path(file_path: &str) -> PyResult<()> {
    if !file_path.to_ascii_lowercase().ends_with(".ppt") {
        return Err(PyValueError::new_err(format!(
            "Expected .ppt file path, got: {file_path}"
        )));
    }
    if !Path::new(file_path).exists() {
        return Err(PyIOError::new_err(format!("File not found: {file_path}")));
    }
    Ok(())
}

pub(crate) fn load_ppt_paragraphs(file_path: &str) -> Result<Vec<DocParagraph>, String> {
    let stream = cfb_reader::read_powerpoint_document_stream(file_path)?;
    Ok(text_extractor::extract_paragraphs(&stream))
}

#[pyfunction]
fn chunk_ppt(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    validate_ppt_path(file_path)?;
    let start = Instant::now();
    let paragraphs = load_ppt_paragraphs(file_path).map_err(PyRuntimeError::new_err)?;
    let chunks = build_structural_chunks(paragraphs);
    let rust_ms = start.elapsed().as_secs_f64() * 1000.0;
    chunks_to_py(py, file_path, chunks, rust_ms)
}

#[pyfunction]
fn chunk_ppt_section(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    validate_ppt_path(file_path)?;
    let start = Instant::now();
    let paragraphs = load_ppt_paragraphs(file_path).map_err(PyRuntimeError::new_err)?;
    let chunks = build_section_chunks(paragraphs);
    let rust_ms = start.elapsed().as_secs_f64() * 1000.0;
    chunks_to_py(py, file_path, chunks, rust_ms)
}

#[pyfunction]
fn chunk_ppt_semantic(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    validate_ppt_path(file_path)?;
    let start = Instant::now();
    let paragraphs = load_ppt_paragraphs(file_path).map_err(PyRuntimeError::new_err)?;
    let chunks = build_semantic_chunks(paragraphs);
    let rust_ms = start.elapsed().as_secs_f64() * 1000.0;
    chunks_to_py(py, file_path, chunks, rust_ms)
}

#[pyfunction]
fn chunk_ppt_sliding_window(
    py: Python<'_>,
    file_path: &str,
    window_size: usize,
    overlap: usize,
) -> PyResult<PyObject> {
    validate_ppt_path(file_path)?;
    if window_size == 0 {
        return Err(PyValueError::new_err("window_size must be greater than 0"));
    }
    if overlap >= window_size {
        return Err(PyValueError::new_err("overlap must be less than window_size"));
    }
    let start = Instant::now();
    let paragraphs = load_ppt_paragraphs(file_path).map_err(PyRuntimeError::new_err)?;
    let chunks = build_sliding_window_chunks(paragraphs, window_size, overlap);
    let rust_ms = start.elapsed().as_secs_f64() * 1000.0;
    chunks_to_py(py, file_path, chunks, rust_ms)
}

#[pyfunction]
fn chunk_ppt_sentence(
    py: Python<'_>,
    file_path: &str,
    sentences_per_chunk: usize,
) -> PyResult<PyObject> {
    validate_ppt_path(file_path)?;
    if sentences_per_chunk == 0 {
        return Err(PyValueError::new_err(
            "sentences_per_chunk must be greater than 0",
        ));
    }
    let start = Instant::now();
    let paragraphs = load_ppt_paragraphs(file_path).map_err(PyRuntimeError::new_err)?;
    let chunks = build_sentence_chunks(paragraphs, sentences_per_chunk);
    let rust_ms = start.elapsed().as_secs_f64() * 1000.0;
    chunks_to_py(py, file_path, chunks, rust_ms)
}

#[pyfunction]
fn chunk_ppt_page_aware(
    py: Python<'_>,
    file_path: &str,
    paragraphs_per_page: usize,
) -> PyResult<PyObject> {
    validate_ppt_path(file_path)?;
    if paragraphs_per_page == 0 {
        return Err(PyValueError::new_err(
            "paragraphs_per_page must be greater than 0",
        ));
    }
    let start = Instant::now();
    let paragraphs = load_ppt_paragraphs(file_path).map_err(PyRuntimeError::new_err)?;
    let chunks = build_page_aware_chunks(paragraphs, paragraphs_per_page);
    let rust_ms = start.elapsed().as_secs_f64() * 1000.0;
    chunks_to_py(py, file_path, chunks, rust_ms)
}

#[pyclass]
pub struct PptStructuralIterator {
    chunks: Vec<ChunkRecord>,
    index: usize,
    source: String,
    total_chunks: usize,
}

#[pymethods]
impl PptStructuralIterator {
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
        dict.set_item("content_type", chunk.content_type)?;
        dict.set_item(
            "metadata",
            pythonize(
                py,
                &json!({
                    "source": self.source,
                    "chunk_index": chunk.chunk_index,
                    "total_chunks": self.total_chunks,
                    "paragraph_type": chunk.paragraph_type,
                    "heading_level": chunk.heading_level,
                    "page_number": serde_json::Value::Null,
                }),
            )?,
        )?;
        Ok(Some(dict.into_any().unbind()))
    }
}

#[pyfunction]
fn chunk_ppt_structural_stream(file_path: &str) -> PyResult<PptStructuralIterator> {
    validate_ppt_path(file_path)?;
    let paragraphs = load_ppt_paragraphs(file_path).map_err(PyRuntimeError::new_err)?;
    let chunks = build_structural_chunks(paragraphs);
    let total_chunks = chunks.len();
    Ok(PptStructuralIterator {
        chunks,
        index: 0,
        source: file_path.to_string(),
        total_chunks,
    })
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(chunk_ppt, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_ppt_section, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_ppt_semantic, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_ppt_sliding_window, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_ppt_sentence, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_ppt_page_aware, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_ppt_structural_stream, m)?)?;
    m.add_class::<PptStructuralIterator>()?;
    Ok(())
}
