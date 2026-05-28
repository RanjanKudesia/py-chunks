// src/extensions/html/with_images.rs

use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyModule};
use pyo3::wrap_pyfunction;
use pythonize::pythonize;
use serde_json::json;

use super::images::collect_html_images;
use super::page_aware::build_page_aware_chunks;
use super::section::build_section_chunks;
use super::semantic::build_semantic_chunks;
use super::sentence::build_sentence_chunks;
use super::sliding_window::build_sliding_window_chunks;
use super::structural::build_chunks_from_html_bytes;

#[pyfunction]
#[pyo3(signature = (file_path, mode, window_size, overlap, sentences_per_chunk, paragraphs_per_page))]
pub fn chunk_html_with_images(
    py: Python<'_>,
    file_path: &str,
    mode: &str,
    window_size: usize,
    overlap: usize,
    sentences_per_chunk: usize,
    paragraphs_per_page: usize,
) -> PyResult<(Vec<PyObject>, Vec<(String, Py<PyBytes>)>)> {
    let lower = file_path.to_ascii_lowercase();
    if !lower.ends_with(".html") && !lower.ends_with(".htm") {
        return Err(PyValueError::new_err(format!(
            "Expected .html or .htm file path, got: {file_path}"
        )));
    }

    let bytes = std::fs::read(file_path)
        .map_err(|e| PyIOError::new_err(format!("Failed to read HTML file: {e}")))?;

    let html = std::str::from_utf8(&bytes)
        .map(|s| s.to_string())
        .unwrap_or_else(|_| String::from_utf8_lossy(&bytes).to_string());

    // Build text chunks via the mode-specific internal function.
    let normalized = if mode == "default" { "structural" } else { mode };

    let text_records = match normalized {
        "structural" => build_chunks_from_html_bytes(&bytes),
        "semantic" => build_semantic_chunks(&bytes),
        "section" => build_section_chunks(&bytes),
        "sliding_window" => build_sliding_window_chunks(&bytes, window_size, overlap),
        "sentence" => build_sentence_chunks(&bytes, sentences_per_chunk),
        "page_aware" => build_page_aware_chunks(&bytes, paragraphs_per_page),
        _ => return Err(PyValueError::new_err(format!("Unknown HTML mode: {mode}"))),
    }
    .map_err(|e| PyRuntimeError::new_err(format!("HTML chunking failed: {e}")))?;

    // Extract images (mode-independent).
    let mut image_out: Vec<(String, Vec<u8>)> = Vec::new();
    let image_infos = collect_html_images(&html, file_path, &mut image_out);

    // Build image chunks first (prepend), then text chunks.
    let mut chunk_list: Vec<PyObject> = image_infos
        .into_iter()
        .map(|info| {
            let dict = PyDict::new_bound(py);
            dict.set_item("content", &info.hash_name)?;
            dict.set_item("content_type", "image")?;
            dict.set_item(
                "metadata",
                pythonize(
                    py,
                    &json!({
                        "image_name": info.hash_name,
                        "alt_text": info.alt_text,
                        "document_metadata": { "source_type": "html" }
                    }),
                )?,
            )?;
            Ok(dict.into_any().unbind())
        })
        .collect::<PyResult<_>>()?;

    for rec in text_records {
        let dict = PyDict::new_bound(py);
        dict.set_item("content", &rec.content)?;
        dict.set_item("content_type", rec.content_type.as_str())?;
        dict.set_item("metadata", pythonize(py, &rec.metadata)?)?;
        chunk_list.push(dict.into_any().unbind());
    }

    let image_out_py: Vec<(String, Py<PyBytes>)> = image_out
        .into_iter()
        .map(|(name, data)| (name, PyBytes::new_bound(py, &data).unbind()))
        .collect();

    Ok((chunk_list, image_out_py))
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(chunk_html_with_images, m)?)?;
    Ok(())
}
