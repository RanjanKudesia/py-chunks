//! `.pdf` chunking + markdown pyfunctions.
//!
//! **Migrated to the vendored engine.** Binding only — see CONSOLIDATION_PLAN.md.

crate::bind_format! {
    engine      = chunks_rs::formats::pdf,
    default     = chunk_pdf,
    section     = chunk_pdf_section,
    semantic    = chunk_pdf_semantic,
    sentence    = chunk_pdf_sentence,
    page_aware  = chunk_pdf_page_aware,
    sliding     = chunk_pdf_sliding_window,
    to_markdown = pdf_to_markdown,
    stream      = stream_pdf_chunks,
}

crate::bind_images! {
    chunk_with_images       = chunk_pdf_with_images,
    to_markdown_with_images = pdf_to_markdown_with_images,
    no_row_args,
}

/// The `default` mode: per-page heading ranking, no document-wide pass.
#[pyfunction]
fn chunk_pdf_fast(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    __run_mode(py, file_path, "default", 3, 1, 3, 15)
}

/// The `structural` mode: heading levels ranked across the whole document.
///
/// PDF needs its own entry point because `bind_format!`'s generated
/// `chunk_pdf` passes `"default"` — correct for every other format, where the
/// two modes *are* the same pipeline, and silently wrong for PDF now that they
/// differ (TECH_DEBT #54). Without this, Python's `structural` would take the
/// fast path while Rust's and JavaScript's did not.
#[pyfunction]
fn chunk_pdf_structural(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    __run_mode(py, file_path, "structural", 3, 1, 3, 15)
}

pub fn register_extra(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(chunk_pdf_fast, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_pdf_structural, m)?)?;
    Ok(())
}
