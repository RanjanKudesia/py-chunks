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

/// Shipped as a separate entry point; it has always run the same path as
/// `default` (TECH_DEBT #54 tracks the docs that claim otherwise).
#[pyfunction]
fn chunk_pdf_fast(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    __run_mode(py, file_path, "default", 3, 1, 3, 15)
}

pub fn register_extra(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(chunk_pdf_fast, m)?)?;
    Ok(())
}
