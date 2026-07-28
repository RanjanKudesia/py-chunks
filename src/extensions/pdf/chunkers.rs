//! PDF chunking + markdown pyfunctions, backed by liteparse.
//!
//! Pipeline: PDF --liteparse--> markdown --existing Markdown chunker--> chunks.
//! This reuses the mature Markdown chunking modes on liteparse's high-quality
//! markdown, so PDF chunks share the Markdown chunk schema and quality.

use std::time::Instant;

use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyModule};
use pyo3::wrap_pyfunction;
use pythonize::pythonize;

use super::liteparse_backend::pdf_to_markdown as backend_to_markdown;
use crate::extensions::md::common::ChunkRecordInput;
use crate::extensions::md::page_aware::build_page_aware_chunks;
use crate::extensions::md::section::build_section_chunks;
use crate::extensions::md::semantic::build_semantic_chunks;
use crate::extensions::md::sentence::build_sentence_chunks;
use crate::extensions::md::sliding_window::build_sliding_window_chunks;
use crate::extensions::md::structural::build_chunks_from_md_bytes;

fn ensure_pdf(file_path: &str) -> PyResult<()> {
    if !file_path.to_ascii_lowercase().ends_with(".pdf") {
        return Err(PyValueError::new_err(format!(
            "Expected .pdf file path, got: {file_path}"
        )));
    }
    Ok(())
}

/// Convert one `ChunkRecordInput` into a Python `{content, content_type, metadata}` dict.
fn record_to_pydict(py: Python<'_>, rec: &ChunkRecordInput) -> PyResult<PyObject> {
    let dict = PyDict::new_bound(py);
    dict.set_item("content", &rec.content)?;
    dict.set_item("content_type", rec.content_type.as_str())?;
    dict.set_item("metadata", pythonize(py, &rec.metadata)?)?;
    Ok(dict.into_any().unbind())
}

/// Inject the PDF document-level metadata every chunk carries, preserving the
/// `document_metadata: {source_type, total_pages}` contract of the old pipeline.
fn inject_pdf_metadata(records: &mut [ChunkRecordInput], total_pages: usize) {
    for rec in records.iter_mut() {
        if let serde_json::Value::Object(map) = &mut rec.metadata {
            map.insert(
                "document_metadata".to_string(),
                serde_json::json!({
                    "source_type": "pdf",
                    "total_pages": total_pages,
                }),
            );
        }
    }
}

/// Build Markdown chunks for the given mode from raw markdown bytes.
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
        other => Err(format!("Unknown PDF mode: {other}")),
    }
}

/// Shared implementation: parse the PDF once, chunk with `mode`, return
/// `{chunks, rust_ms}` exactly like the other chunkers.
fn run_mode(
    py: Python<'_>,
    file_path: &str,
    mode: &str,
    window_size: usize,
    overlap: usize,
    sentences_per_chunk: usize,
    paragraphs_per_page: usize,
) -> PyResult<PyObject> {
    ensure_pdf(file_path)?;
    let start = Instant::now();
    let conv = backend_to_markdown(file_path, false).map_err(PyRuntimeError::new_err)?;
    let mut records = chunks_for_mode(
        conv.markdown.as_bytes(),
        mode,
        window_size,
        overlap,
        sentences_per_chunk,
        paragraphs_per_page,
    )
    .map_err(PyRuntimeError::new_err)?;
    inject_pdf_metadata(&mut records, conv.total_pages);
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
fn chunk_pdf(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    run_mode(py, file_path, "default", 3, 1, 3, 15)
}

#[pyfunction]
fn chunk_pdf_fast(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    run_mode(py, file_path, "default", 3, 1, 3, 15)
}

#[pyfunction]
fn chunk_pdf_section(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    run_mode(py, file_path, "section", 3, 1, 3, 15)
}

#[pyfunction]
fn chunk_pdf_semantic(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    run_mode(py, file_path, "semantic", 3, 1, 3, 15)
}

#[pyfunction]
fn chunk_pdf_sentence(py: Python<'_>, file_path: &str, sentences_per_chunk: usize) -> PyResult<PyObject> {
    run_mode(py, file_path, "sentence", 3, 1, sentences_per_chunk, 15)
}

#[pyfunction]
fn chunk_pdf_page_aware(py: Python<'_>, file_path: &str, paragraphs_per_page: usize) -> PyResult<PyObject> {
    run_mode(py, file_path, "page_aware", 3, 1, 3, paragraphs_per_page)
}

#[pyfunction]
fn chunk_pdf_sliding_window(py: Python<'_>, file_path: &str, window_size: usize, overlap: usize) -> PyResult<PyObject> {
    run_mode(py, file_path, "sliding_window", window_size, overlap, 3, 15)
}

#[pyfunction]
fn pdf_to_markdown(file_path: &str) -> PyResult<String> {
    ensure_pdf(file_path)?;
    let (markdown, _) = pdf_to_markdown_impl(file_path, false)?;
    Ok(markdown)
}

#[pyfunction]
fn pdf_to_markdown_with_images(
    py: Python<'_>,
    file_path: &str,
) -> PyResult<(String, Vec<(String, Py<PyBytes>)>)> {
    ensure_pdf(file_path)?;
    let (markdown, images) = pdf_to_markdown_impl(file_path, true)?;
    let images_py = images
        .into_iter()
        .map(|(name, bytes)| (name, PyBytes::new_bound(py, &bytes).unbind()))
        .collect();
    Ok((markdown, images_py))
}

/// Internal: markdown + (name, bytes) images.
fn pdf_to_markdown_impl(
    file_path: &str,
    embed_images: bool,
) -> PyResult<(String, Vec<(String, Vec<u8>)>)> {
    let conv = backend_to_markdown(file_path, embed_images).map_err(PyIOError::new_err)?;
    let images = conv.images.into_iter().map(|i| (i.name, i.bytes)).collect();
    Ok((conv.markdown, images))
}

#[pyfunction]
#[pyo3(signature = (file_path, mode, rows_per_chunk=1, window_size=3, overlap=1, sentences_per_chunk=3, paragraphs_per_page=15, max_chunk_chars=2000))]
#[allow(clippy::too_many_arguments)]
fn chunk_pdf_with_images(
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
    ensure_pdf(file_path)?;
    let _ = (rows_per_chunk, max_chunk_chars); // accepted for API symmetry
    let normalized_mode = if mode == "default" { "default" } else { mode };

    let conv = backend_to_markdown(file_path, true).map_err(PyRuntimeError::new_err)?;
    let images = conv.images;
    let mut records = chunks_for_mode(
        conv.markdown.as_bytes(),
        normalized_mode,
        window_size,
        overlap,
        sentences_per_chunk,
        paragraphs_per_page,
    )
    .map_err(PyRuntimeError::new_err)?;
    inject_pdf_metadata(&mut records, conv.total_pages);

    // Image chunks first (content_type="image"), then the text chunks — matching
    // the other formats' image-aware output.
    let mut chunk_list: Vec<PyObject> = images
        .iter()
        .map(|img| {
            let dict = PyDict::new_bound(py);
            dict.set_item("content", &img.name)?;
            dict.set_item("content_type", "image")?;
            let meta = PyDict::new_bound(py);
            meta.set_item("image_name", &img.name)?;
            dict.set_item("metadata", meta)?;
            Ok(dict.into_any().unbind())
        })
        .collect::<PyResult<_>>()?;

    for rec in &records {
        chunk_list.push(record_to_pydict(py, rec)?);
    }

    let images_py = images
        .into_iter()
        .map(|img| (img.name, PyBytes::new_bound(py, &img.bytes).unbind()))
        .collect();
    Ok((chunk_list, images_py))
}

#[pyclass]
pub struct PdfStreamIterator {
    chunks: std::vec::IntoIter<PyObject>,
}

#[pymethods]
impl PdfStreamIterator {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }
    fn __next__(&mut self, _py: Python<'_>) -> Option<PyObject> {
        self.chunks.next()
    }
}

#[pyfunction]
#[pyo3(signature = (file_path, mode, window_size, overlap, sentences_per_chunk, paragraphs_per_page))]
fn stream_pdf_chunks(
    py: Python<'_>,
    file_path: &str,
    mode: &str,
    window_size: usize,
    overlap: usize,
    sentences_per_chunk: usize,
    paragraphs_per_page: usize,
) -> PyResult<Py<PdfStreamIterator>> {
    ensure_pdf(file_path)?;
    let conv = backend_to_markdown(file_path, false).map_err(PyRuntimeError::new_err)?;
    let mut records = chunks_for_mode(
        conv.markdown.as_bytes(),
        mode,
        window_size,
        overlap,
        sentences_per_chunk,
        paragraphs_per_page,
    )
    .map_err(PyRuntimeError::new_err)?;
    inject_pdf_metadata(&mut records, conv.total_pages);
    let chunk_vec: Vec<PyObject> = records
        .iter()
        .map(|r| record_to_pydict(py, r))
        .collect::<PyResult<_>>()?;
    Py::new(
        py,
        PdfStreamIterator {
            chunks: chunk_vec.into_iter(),
        },
    )
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(chunk_pdf, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_pdf_fast, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_pdf_section, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_pdf_semantic, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_pdf_sentence, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_pdf_page_aware, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_pdf_sliding_window, m)?)?;
    m.add_function(wrap_pyfunction!(pdf_to_markdown, m)?)?;
    m.add_function(wrap_pyfunction!(pdf_to_markdown_with_images, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_pdf_with_images, m)?)?;
    m.add_function(wrap_pyfunction!(stream_pdf_chunks, m)?)?;
    Ok(())
}
