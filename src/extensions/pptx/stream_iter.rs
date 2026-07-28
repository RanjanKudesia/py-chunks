/// Streaming iterator for all PPTX chunking strategies.
///
/// PPTX requires reading the entire ZIP archive before any slide can be
/// processed, so all strategies use batch-drain: the batch function runs
/// once at construction and yields one chunk per __next__ call.

use std::collections::VecDeque;

use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyModule;
use pyo3::wrap_pyfunction;
use pythonize::pythonize;
use std::fs;

use super::common::ChunkRecordInput;
use super::page_aware::build_page_aware_chunks;
use super::section::build_section_chunks;
use super::semantic::build_semantic_chunks;
use super::sentence::build_sentence_chunks;
use super::sliding_window::build_sliding_window_chunks;
use super::structural::build_structural_chunks;

fn chunk_to_pydict(py: Python<'_>, c: &ChunkRecordInput) -> PyResult<PyObject> {
    let dict = pyo3::types::PyDict::new_bound(py);
    dict.set_item("content", &c.content)?;
    dict.set_item("content_type", c.content_type.as_str())?;
    dict.set_item("metadata", pythonize(py, &c.metadata)?)?;
    Ok(dict.into_any().unbind())
}

#[pyclass]
pub struct PptxStreamIterator {
    chunks: VecDeque<ChunkRecordInput>,
}

#[pymethods]
impl PptxStreamIterator {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> { slf }

    fn __next__(&mut self, py: Python<'_>) -> PyResult<Option<PyObject>> {
        match self.chunks.pop_front() {
            Some(chunk) => chunk_to_pydict(py, &chunk).map(Some),
            None => Ok(None),
        }
    }
}

#[pyfunction]
pub fn stream_pptx_chunks(
    py: Python<'_>,
    file_path: &str,
    mode: &str,
    window_size: usize,
    overlap: usize,
    sentences_per_chunk: usize,
    slides_per_chunk: usize,
) -> PyResult<Py<PptxStreamIterator>> {
    if !crate::extensions::pptx::common::is_pptx_ooxml(file_path) {
        return Err(PyValueError::new_err(format!(
            "Expected a PowerPoint OOXML file ({}), got: {file_path}",
            crate::extensions::pptx::common::pptx_exts_display()
        )));
    }
    match mode {
        "default" | "structural" | "semantic" | "section"
        | "sliding_window" | "sentence" | "page_aware" => {}
        _ => return Err(PyValueError::new_err(format!("Unknown PPTX streaming mode: {mode}"))),
    }
    if mode == "sliding_window" {
        if window_size == 0 { return Err(PyValueError::new_err("window_size must be > 0")); }
        if overlap >= window_size { return Err(PyValueError::new_err("overlap must be < window_size")); }
    }
    if mode == "sentence" && sentences_per_chunk == 0 {
        return Err(PyValueError::new_err("sentences_per_chunk must be > 0"));
    }
    if mode == "page_aware" && slides_per_chunk == 0 {
        return Err(PyValueError::new_err("slides_per_chunk must be > 0"));
    }

    let bytes = fs::read(file_path)
        .map_err(|e| PyIOError::new_err(format!("Failed to read PPTX file: {e}")))?;

    let chunks = match mode {
        "default" | "structural" => build_structural_chunks(&bytes),
        "semantic"               => build_semantic_chunks(&bytes),
        "section"                => build_section_chunks(&bytes),
        "sliding_window"         => build_sliding_window_chunks(&bytes, window_size, overlap),
        "sentence"               => build_sentence_chunks(&bytes, sentences_per_chunk),
        "page_aware"             => build_page_aware_chunks(&bytes, slides_per_chunk),
        _                        => unreachable!(),
    }
    .map_err(PyRuntimeError::new_err)?;

    Py::new(
        py,
        PptxStreamIterator {
            chunks: chunks.into_iter().collect(),
        },
    )
}

pub fn register(m: &pyo3::Bound<'_, PyModule>) -> pyo3::PyResult<()> {
    m.add_class::<PptxStreamIterator>()?;
    m.add_function(wrap_pyfunction!(stream_pptx_chunks, m)?)?;
    Ok(())
}
