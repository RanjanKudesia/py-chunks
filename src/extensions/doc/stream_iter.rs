use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};
use pyo3::wrap_pyfunction;
use pythonize::pythonize;
use serde_json::json;

use super::structural::{
    build_page_aware_chunks, build_section_chunks, build_semantic_chunks, build_sentence_chunks,
    build_sliding_window_chunks, load_doc_paragraphs, validate_doc_path, ChunkRecord,
};

fn chunk_to_py(
    py: Python<'_>,
    chunk: &ChunkRecord,
    source: &str,
    total_chunks: usize,
) -> PyResult<PyObject> {
    let dict = PyDict::new_bound(py);
    dict.set_item("content", &chunk.content)?;
    dict.set_item("content_type", chunk.content_type)?;
    dict.set_item(
        "metadata",
        pythonize(
            py,
            &json!({
                "source": source,
                "chunk_index": chunk.chunk_index,
                "total_chunks": total_chunks,
                "paragraph_type": chunk.paragraph_type,
                "heading_level": chunk.heading_level,
                "page_number": serde_json::Value::Null,
            }),
        )?,
    )?;
    Ok(dict.into_any().unbind())
}

#[pyclass]
pub struct DocSemanticIterator {
    chunks: Vec<ChunkRecord>,
    index: usize,
    source: String,
    total_chunks: usize,
}

#[pymethods]
impl DocSemanticIterator {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(&mut self, py: Python<'_>) -> PyResult<Option<PyObject>> {
        if self.index >= self.chunks.len() {
            return Ok(None);
        }
        let chunk = &self.chunks[self.index];
        self.index += 1;
        Ok(Some(chunk_to_py(py, chunk, &self.source, self.total_chunks)?))
    }
}

#[pyclass]
pub struct DocSectionIterator {
    chunks: Vec<ChunkRecord>,
    index: usize,
    source: String,
    total_chunks: usize,
}

#[pymethods]
impl DocSectionIterator {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(&mut self, py: Python<'_>) -> PyResult<Option<PyObject>> {
        if self.index >= self.chunks.len() {
            return Ok(None);
        }
        let chunk = &self.chunks[self.index];
        self.index += 1;
        Ok(Some(chunk_to_py(py, chunk, &self.source, self.total_chunks)?))
    }
}

#[pyclass]
pub struct DocSlidingWindowIterator {
    chunks: Vec<ChunkRecord>,
    index: usize,
    source: String,
    total_chunks: usize,
}

#[pymethods]
impl DocSlidingWindowIterator {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(&mut self, py: Python<'_>) -> PyResult<Option<PyObject>> {
        if self.index >= self.chunks.len() {
            return Ok(None);
        }
        let chunk = &self.chunks[self.index];
        self.index += 1;
        Ok(Some(chunk_to_py(py, chunk, &self.source, self.total_chunks)?))
    }
}

#[pyclass]
pub struct DocSentenceIterator {
    chunks: Vec<ChunkRecord>,
    index: usize,
    source: String,
    total_chunks: usize,
}

#[pymethods]
impl DocSentenceIterator {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(&mut self, py: Python<'_>) -> PyResult<Option<PyObject>> {
        if self.index >= self.chunks.len() {
            return Ok(None);
        }
        let chunk = &self.chunks[self.index];
        self.index += 1;
        Ok(Some(chunk_to_py(py, chunk, &self.source, self.total_chunks)?))
    }
}

#[pyclass]
pub struct DocPageAwareIterator {
    chunks: Vec<ChunkRecord>,
    index: usize,
    source: String,
    total_chunks: usize,
}

#[pymethods]
impl DocPageAwareIterator {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(&mut self, py: Python<'_>) -> PyResult<Option<PyObject>> {
        if self.index >= self.chunks.len() {
            return Ok(None);
        }
        let chunk = &self.chunks[self.index];
        self.index += 1;
        Ok(Some(chunk_to_py(py, chunk, &self.source, self.total_chunks)?))
    }
}

#[pyfunction]
fn chunk_doc_semantic_stream(file_path: &str) -> PyResult<DocSemanticIterator> {
    validate_doc_path(file_path)?;
    let paragraphs = load_doc_paragraphs(file_path).map_err(PyRuntimeError::new_err)?;
    let chunks = build_semantic_chunks(paragraphs);
    let total_chunks = chunks.len();
    Ok(DocSemanticIterator {
        chunks,
        index: 0,
        source: file_path.to_string(),
        total_chunks,
    })
}

#[pyfunction]
fn chunk_doc_section_stream(file_path: &str) -> PyResult<DocSectionIterator> {
    validate_doc_path(file_path)?;
    let paragraphs = load_doc_paragraphs(file_path).map_err(PyRuntimeError::new_err)?;
    let chunks = build_section_chunks(paragraphs);
    let total_chunks = chunks.len();
    Ok(DocSectionIterator {
        chunks,
        index: 0,
        source: file_path.to_string(),
        total_chunks,
    })
}

#[pyfunction]
fn chunk_doc_sliding_window_stream(
    file_path: &str,
    window_size: usize,
    overlap: usize,
) -> PyResult<DocSlidingWindowIterator> {
    validate_doc_path(file_path)?;
    if window_size == 0 {
        return Err(PyValueError::new_err("window_size must be greater than 0"));
    }
    if overlap >= window_size {
        return Err(PyValueError::new_err("overlap must be less than window_size"));
    }

    let paragraphs = load_doc_paragraphs(file_path).map_err(PyRuntimeError::new_err)?;
    let chunks = build_sliding_window_chunks(paragraphs, window_size, overlap);
    let total_chunks = chunks.len();
    Ok(DocSlidingWindowIterator {
        chunks,
        index: 0,
        source: file_path.to_string(),
        total_chunks,
    })
}

#[pyfunction]
fn chunk_doc_sentence_stream(
    file_path: &str,
    sentences_per_chunk: usize,
) -> PyResult<DocSentenceIterator> {
    validate_doc_path(file_path)?;
    if sentences_per_chunk == 0 {
        return Err(PyValueError::new_err(
            "sentences_per_chunk must be greater than 0",
        ));
    }

    let paragraphs = load_doc_paragraphs(file_path).map_err(PyRuntimeError::new_err)?;
    let chunks = build_sentence_chunks(paragraphs, sentences_per_chunk);
    let total_chunks = chunks.len();
    Ok(DocSentenceIterator {
        chunks,
        index: 0,
        source: file_path.to_string(),
        total_chunks,
    })
}

#[pyfunction]
fn chunk_doc_page_aware_stream(
    file_path: &str,
    paragraphs_per_page: usize,
) -> PyResult<DocPageAwareIterator> {
    validate_doc_path(file_path)?;
    if paragraphs_per_page == 0 {
        return Err(PyValueError::new_err(
            "paragraphs_per_page must be greater than 0",
        ));
    }

    let paragraphs = load_doc_paragraphs(file_path).map_err(PyRuntimeError::new_err)?;
    let chunks = build_page_aware_chunks(paragraphs, paragraphs_per_page);
    let total_chunks = chunks.len();
    Ok(DocPageAwareIterator {
        chunks,
        index: 0,
        source: file_path.to_string(),
        total_chunks,
    })
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(chunk_doc_semantic_stream, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_doc_section_stream, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_doc_sliding_window_stream, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_doc_sentence_stream, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_doc_page_aware_stream, m)?)?;
    m.add_class::<DocSemanticIterator>()?;
    m.add_class::<DocSectionIterator>()?;
    m.add_class::<DocSlidingWindowIterator>()?;
    m.add_class::<DocSentenceIterator>()?;
    m.add_class::<DocPageAwareIterator>()?;
    Ok(())
}
