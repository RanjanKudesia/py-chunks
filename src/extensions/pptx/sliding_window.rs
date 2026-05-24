use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};
use pyo3::wrap_pyfunction;
use pythonize::pythonize;
use serde_json::json;
use std::fs;
use std::time::Instant;

use super::common::{
    collect_slide_names, open_pptx, read_all_slides, ChunkRecordInput, ContentType, MAX_CHUNK_CHARS,
};

/// Safety cap: no window chunk may exceed this many chars regardless of window_size.
const MAX_WINDOW_CONTENT_CHARS: usize = MAX_CHUNK_CHARS * 5; // 6000

pub fn build_sliding_window_chunks(
    bytes: &[u8],
    window_size: usize,
    overlap: usize,
) -> Result<Vec<ChunkRecordInput>, String> {
    if window_size == 0 {
        return Err("window_size must be > 0".to_string());
    }
    if overlap >= window_size {
        return Err("overlap must be < window_size".to_string());
    }

    let mut archive = open_pptx(bytes)?;
    let slide_names = collect_slide_names(&archive);
    if slide_names.is_empty() {
        return Err("No slides found".to_string());
    }
    let total_slides = slide_names.len();

    let mut units: Vec<(usize, String, Option<String>)> = Vec::new();
    for (slide_num, slide) in read_all_slides(&mut archive, &slide_names)? {
        let text = slide.all_text();
        if text.is_empty() {
            continue;
        }
        units.push((slide_num, text, slide.title));
    }

    if units.is_empty() {
        return Err("No text content found".to_string());
    }

    let step = window_size - overlap;
    let mut result: Vec<ChunkRecordInput> = Vec::new();
    let mut start = 0usize;
    let mut window_index = 0usize;

    loop {
        let end = (start + window_size).min(units.len());
        let window = &units[start..end];
        let raw_content = window
            .iter()
            .map(|(_, t, _)| t.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        // Safety cap: prevent unbounded output for very large window_size values.
        let (content, truncated) = if raw_content.len() > MAX_WINDOW_CONTENT_CHARS {
            let truncate_at = raw_content[..MAX_WINDOW_CONTENT_CHARS]
                .rfind([' ', '\n'])
                .unwrap_or(MAX_WINDOW_CONTENT_CHARS);
            (raw_content[..truncate_at].trim_end().to_string(), true)
        } else {
            (raw_content, false)
        };
        if !content.is_empty() {
            result.push(ChunkRecordInput {
                content_type: ContentType::SlidingWindow,
                content,
                metadata: json!({
                    "window_size":   window_size,
                    "overlap":       overlap,
                    "window_index":  window_index,
                    "slide_range":   [window[0].0, window.last().unwrap().0],
                    "slide_count":   window.len(),
                    "chunk_index":   window_index,
                    "truncated":     truncated,
                    "document_metadata": { "source_type": "pptx", "total_slides": total_slides }
                }),
            });
            window_index += 1;
        }
        if end >= units.len() {
            break;
        }
        start += step;
    }

    if result.is_empty() {
        return Err("No sliding-window chunks generated".to_string());
    }
    Ok(result)
}

#[pyfunction]
pub fn chunk_pptx_sliding_window(
    py: Python<'_>,
    file_path: &str,
    window_size: usize,
    overlap: usize,
) -> PyResult<PyObject> {
    if !file_path.to_ascii_lowercase().ends_with(".pptx") {
        return Err(PyValueError::new_err(format!(
            "Expected .pptx file path, got: {file_path}"
        )));
    }
    if window_size == 0 {
        return Err(PyValueError::new_err("window_size must be > 0"));
    }
    if overlap >= window_size {
        return Err(PyValueError::new_err("overlap must be < window_size"));
    }
    let bytes = fs::read(file_path).map_err(|e| PyIOError::new_err(format!("{e}")))?;
    let rust_start = Instant::now();
    let chunks_raw = build_sliding_window_chunks(&bytes, window_size, overlap)
        .map_err(PyRuntimeError::new_err)?;
    let rust_ms = rust_start.elapsed().as_secs_f64() * 1000.0;
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

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(chunk_pptx_sliding_window, m)?)?;
    Ok(())
}
