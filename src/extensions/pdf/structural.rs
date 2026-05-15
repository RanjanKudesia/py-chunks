use pdfium_render::prelude::*;
use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use pythonize::pythonize;
use std::time::Instant;

use super::common::{
    absorb_heading_only_units, build_paragraph, build_units, classify_chunk, is_noise_line,
    is_standalone_number, is_toc_heading, looks_like_toc_entry, merge_short_chunks, normalize_line,
    pdf_metadata, split_raw_blocks, split_unit, Paragraph, MAX_CHUNK_CHARS, MIN_CHUNK_CHARS,
};

fn get_pdfium() -> Result<Pdfium, String> {
    // Try pypdfium2_raw location first (installed via pip install pypdfium2)
    let candidates = vec![
        // venv-relative location
        std::env::var("VIRTUAL_ENV")
            .map(|v| {
                format!(
                    "{}/lib/python3.13/site-packages/pypdfium2_raw/libpdfium.dylib",
                    v
                )
            })
            .unwrap_or_default(),
        // Direct path fallback (macOS)
        "/usr/local/lib/libpdfium.dylib".to_string(),
        "/usr/lib/libpdfium.dylib".to_string(),
    ];

    for path in &candidates {
        if path.is_empty() {
            continue;
        }
        if std::path::Path::new(path).exists() {
            if let Ok(bindings) = Pdfium::bind_to_library(path) {
                return Ok(Pdfium::new(bindings));
            }
        }
    }

    // Last resort - system library search
    Pdfium::bind_to_system_library()
        .map(|b| Pdfium::new(b))
        .map_err(|e| {
            format!(
                "PDFium not found. Install with: pip install pypdfium2. Error: {}",
                e
            )
        })
}

#[pyfunction]
pub fn chunk_pdf(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    if !file_path.to_ascii_lowercase().ends_with(".pdf") {
        return Err(PyValueError::new_err(format!(
            "Expected .pdf file path, got: {file_path}"
        )));
    }

    let start = Instant::now();

    let pdfium = get_pdfium().map_err(|e| PyRuntimeError::new_err(e))?;

    let doc = pdfium
        .load_pdf_from_file(file_path, None)
        .map_err(|e| PyIOError::new_err(format!("Failed to load PDF: {}", e)))?;

    let mut all_paragraphs: Vec<Paragraph> = Vec::new();
    let mut in_toc = false;

    for (_page_idx, page) in doc.pages().iter().enumerate() {
        let page_text = match page.text() {
            Ok(t) => t.all(),
            Err(_) => continue,
        };

        if page_text.trim().is_empty() {
            continue;
        }

        for block_lines in split_raw_blocks(&page_text) {
            let clean: Vec<String> = block_lines
                .iter()
                .map(|l| normalize_line(l))
                .filter(|l| !l.is_empty() && !is_noise_line(l))
                .collect();

            if clean.is_empty() {
                continue;
            }

            if is_toc_heading(&clean[0]) {
                in_toc = true;
                continue;
            }

            if in_toc {
                let real_lines: Vec<String> = clean
                    .into_iter()
                    .filter(|l| !looks_like_toc_entry(l) && !is_standalone_number(l))
                    .collect();
                if real_lines.is_empty() {
                    continue;
                }
                in_toc = false;
                if let Some(para) = build_paragraph(real_lines) {
                    all_paragraphs.push(para);
                }
                continue;
            }

            if let Some(para) = build_paragraph(clean) {
                all_paragraphs.push(para);
            }
        }
    }

    if all_paragraphs.is_empty() {
        return Err(PyRuntimeError::new_err(
            "PDF appears to contain no extractable text",
        ));
    }

    let units = absorb_heading_only_units(build_units(all_paragraphs));

    let mut raw_chunks: Vec<String> = Vec::new();
    for unit in &units {
        raw_chunks.extend(split_unit(unit, MAX_CHUNK_CHARS));
    }

    let chunks = merge_short_chunks(raw_chunks, MIN_CHUNK_CHARS, MAX_CHUNK_CHARS);

    if chunks.is_empty() {
        return Err(PyRuntimeError::new_err("No chunks generated from PDF"));
    }

    let rust_ms = start.elapsed().as_secs_f64() * 1000.0;

    let chunk_list: Vec<PyObject> = chunks
        .into_iter()
        .map(|text| {
            let content_type = classify_chunk(&text);
            let dict = PyDict::new_bound(py);
            dict.set_item("content", &text)?;
            dict.set_item("content_type", content_type.as_str())?;
            dict.set_item("metadata", pythonize(py, &pdf_metadata())?)?;
            Ok(dict.into_any().unbind())
        })
        .collect::<PyResult<_>>()?;

    let result = PyDict::new_bound(py);
    result.set_item("chunks", chunk_list)?;
    result.set_item("rust_ms", (rust_ms * 1000.0).round() / 1000.0)?;
    Ok(result.into_any().unbind())
}

pub fn register(m: &pyo3::Bound<'_, PyModule>) -> pyo3::PyResult<()> {
    m.add_function(pyo3::wrap_pyfunction!(chunk_pdf, m)?)?;
    Ok(())
}
