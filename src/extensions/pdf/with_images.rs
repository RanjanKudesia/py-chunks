// src/extensions/pdf/with_images.rs
//
// Image-aware chunking for PDFs. Returns the same text chunks as the
// mode-specific `chunk_pdf_*` functions, prepended with one `content_type:
// "image"` chunk per page-occurrence of an embedded raster, plus the raw PNG
// bytes for every unique image. Mirrors `chunk_html_with_images` /
// `chunk_pptx_with_images`.

use pdfium_render::prelude::*;
use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyList, PyModule};
use pyo3::wrap_pyfunction;
use pythonize::pythonize;
use serde_json::json;

use super::images::extract_pdf_images;
use super::structural::{
    chunk_pdf, chunk_pdf_fast, chunk_pdf_page_aware, chunk_pdf_section, chunk_pdf_semantic,
    chunk_pdf_sentence, chunk_pdf_sliding_window, get_pdfium,
};

#[pyfunction]
#[pyo3(signature = (file_path, mode, window_size, overlap, sentences_per_chunk, paragraphs_per_page))]
pub fn chunk_pdf_with_images(
    py: Python<'_>,
    file_path: &str,
    mode: &str,
    window_size: usize,
    overlap: usize,
    sentences_per_chunk: usize,
    paragraphs_per_page: usize,
) -> PyResult<(Vec<PyObject>, Vec<(String, Py<PyBytes>)>)> {
    if !file_path.to_ascii_lowercase().ends_with(".pdf") {
        return Err(PyValueError::new_err(format!(
            "Expected .pdf file path, got: {file_path}"
        )));
    }

    // ── Extract images (mode-independent) ──────────────────────────────────
    let pdfium = get_pdfium().map_err(PyRuntimeError::new_err)?;
    let doc = pdfium
        .load_pdf_from_file(file_path, None)
        .map_err(|e| PyIOError::new_err(format!("Failed to load PDF: {e}")))?;
    let total_pages = doc.pages().len();
    let (image_infos, image_out) = extract_pdf_images(&doc);
    drop(doc);
    drop(pdfium);

    // ── Build image chunks first (prepend), then text chunks ───────────────
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
                        "page_number": info.page_number,
                        "image_name": info.hash_name,
                        "document_metadata": {
                            "source_type": "pdf",
                            "total_pages": total_pages
                        }
                    }),
                )?,
            )?;
            Ok(dict.into_any().unbind())
        })
        .collect::<PyResult<_>>()?;

    // ── Text chunks via the mode-specific function ─────────────────────────
    // Dispatch mirrors the Python wrapper's `_run_pdf_mode`: "default" maps to
    // the fast lightweight extractor, "structural" to the full font pipeline.
    let text_result = match mode {
        "default" => chunk_pdf_fast(py, file_path),
        "structural" => chunk_pdf(py, file_path),
        "section" => chunk_pdf_section(py, file_path),
        "semantic" => chunk_pdf_semantic(py, file_path),
        "sentence" => chunk_pdf_sentence(py, file_path, sentences_per_chunk),
        "sliding_window" => chunk_pdf_sliding_window(py, file_path, window_size, overlap),
        "page_aware" => chunk_pdf_page_aware(py, file_path, paragraphs_per_page),
        _ => return Err(PyValueError::new_err(format!("Unknown PDF mode: {mode}"))),
    };

    match text_result {
        Ok(result) => {
            // The mode functions return `{"chunks": [...], "rust_ms": ...}`; lift
            // the chunk dicts out and append them after the image chunks.
            let bound = result.bind(py);
            let result_dict = bound
                .downcast::<PyDict>()
                .map_err(|_| PyRuntimeError::new_err("PDF chunker returned a non-dict result"))?;
            let chunks_obj = result_dict
                .get_item("chunks")?
                .ok_or_else(|| PyRuntimeError::new_err("PDF chunker result missing 'chunks'"))?;
            let chunks_list = chunks_obj
                .downcast::<PyList>()
                .map_err(|_| PyRuntimeError::new_err("PDF chunker 'chunks' is not a list"))?;
            for item in chunks_list.iter() {
                chunk_list.push(item.unbind());
            }
        }
        // Scanned / image-only PDFs have no extractable text, so the text
        // chunkers raise. When we still recovered images, return those rather
        // than failing the whole call (matching the PPTX/HTML behaviour of not
        // hard-failing on text-less documents). With neither text nor images,
        // propagate the original error.
        Err(e) => {
            if chunk_list.is_empty() {
                return Err(e);
            }
        }
    }

    let image_out_py: Vec<(String, Py<PyBytes>)> = image_out
        .into_iter()
        .map(|(name, data)| (name, PyBytes::new_bound(py, &data).unbind()))
        .collect();

    Ok((chunk_list, image_out_py))
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(chunk_pdf_with_images, m)?)?;
    Ok(())
}
