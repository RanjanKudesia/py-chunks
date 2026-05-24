/// Default and structural chunker for PPTX.
///
/// Both `chunk_pptx` (default) and `chunk_pptx_structural` use the same
/// algorithm: one chunk per slide, with short slides merged when they fall
/// below MIN_CHUNK_CHARS.  Output follows the standard schema:
///   { content, content_type, metadata: { slide_number, slide_title, ... } }
use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};
use pyo3::wrap_pyfunction;
use pythonize::pythonize;
use std::fs;
use std::time::Instant;

use super::common::{
    classify_chunk, collect_slide_names, open_pptx, pptx_metadata, read_all_slides,
    split_large_text, ChunkRecordInput, MAX_CHUNK_CHARS, MIN_CHUNK_CHARS,
};

// ── Core build function ───────────────────────────────────────────────────────

pub fn build_structural_chunks(bytes: &[u8]) -> Result<Vec<ChunkRecordInput>, String> {
    let mut archive = open_pptx(bytes)?;
    let slide_names = collect_slide_names(&archive);
    if slide_names.is_empty() {
        return Err("No slides found in PPTX archive".to_string());
    }
    let total_slides = slide_names.len();

    // (content, slide_num, slide_title)
    let mut raw: Vec<(String, usize, Option<String>)> = Vec::new();
    for (slide_num, slide) in read_all_slides(&mut archive, &slide_names)? {
        let full_text = slide.all_text();
        if full_text.is_empty() {
            continue;
        }
        let title = slide.title;
        if full_text.len() <= MAX_CHUNK_CHARS {
            raw.push((full_text, slide_num, title));
        } else {
            for part in split_large_text(&full_text, MAX_CHUNK_CHARS) {
                raw.push((part, slide_num, title.clone()));
            }
        }
    }

    if raw.is_empty() {
        return Err("No text content found in PPTX document".to_string());
    }

    // Merge short consecutive same-slide chunks.
    // Hard cap at MAX_CHUNK_CHARS to honour the documented per-chunk limit.
    let mut merged: Vec<(String, usize, Option<String>)> = Vec::new();
    for (text, slide_num, title) in raw {
        if let Some((prev_text, prev_slide, _)) = merged.last_mut() {
            if *prev_slide == slide_num && text.len() < MIN_CHUNK_CHARS {
                let candidate = format!("{prev_text}\n{text}").trim().to_string();
                if candidate.len() <= MAX_CHUNK_CHARS {
                    *prev_text = candidate;
                    continue;
                }
            }
        }
        merged.push((text, slide_num, title));
    }

    let result = merged
        .into_iter()
        .map(|(text, slide_num, title)| ChunkRecordInput {
            content_type: classify_chunk(&text),
            content: text,
            metadata: pptx_metadata(slide_num, title, None, total_slides),
        })
        .collect();

    Ok(result)
}

// ── PyO3 helpers ──────────────────────────────────────────────────────────────

fn emit_chunks(
    py: Python<'_>,
    chunks_raw: Vec<ChunkRecordInput>,
    rust_ms: f64,
) -> PyResult<PyObject> {
    let chunk_list: Vec<PyObject> = chunks_raw
        .into_iter()
        .map(|c| {
            let dict = PyDict::new_bound(py);
            dict.set_item("content", &c.content)?;
            dict.set_item("content_type", c.content_type.as_str())?;
            dict.set_item("metadata", pythonize(py, &c.metadata)?)?;
            Ok(dict.into_any().unbind())
        })
        .collect::<PyResult<_>>()?;
    let result = PyDict::new_bound(py);
    result.set_item("chunks", chunk_list)?;
    result.set_item("rust_ms", (rust_ms * 1000.0).round() / 1000.0)?;
    Ok(result.into_any().unbind())
}

// ── PyO3 entry points ─────────────────────────────────────────────────────────

#[pyfunction]
pub fn chunk_pptx(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    if !file_path.to_ascii_lowercase().ends_with(".pptx") {
        return Err(PyValueError::new_err(format!(
            "Expected .pptx file path, got: {file_path}"
        )));
    }
    let bytes = fs::read(file_path)
        .map_err(|e| PyIOError::new_err(format!("Failed to read PPTX file: {e}")))?;
    let rust_start = Instant::now();
    let chunks_raw = build_structural_chunks(&bytes)
        .map_err(|e| PyRuntimeError::new_err(format!("PPTX chunking failed: {e}")))?;
    let rust_ms = rust_start.elapsed().as_secs_f64() * 1000.0;
    emit_chunks(py, chunks_raw, rust_ms)
}

#[pyfunction]
pub fn chunk_pptx_structural(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    if !file_path.to_ascii_lowercase().ends_with(".pptx") {
        return Err(PyValueError::new_err(format!(
            "Expected .pptx file path, got: {file_path}"
        )));
    }
    let bytes = fs::read(file_path)
        .map_err(|e| PyIOError::new_err(format!("Failed to read PPTX file: {e}")))?;
    let rust_start = Instant::now();
    let chunks_raw = build_structural_chunks(&bytes)
        .map_err(|e| PyRuntimeError::new_err(format!("PPTX chunking failed: {e}")))?;
    let rust_ms = rust_start.elapsed().as_secs_f64() * 1000.0;
    emit_chunks(py, chunks_raw, rust_ms)
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(chunk_pptx, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_pptx_structural, m)?)?;
    Ok(())
}
