//! `.html` / `.htm` chunking + markdown pyfunctions.
//!
//! **Migrated to the vendored engine.** Binding only — see CONSOLIDATION_PLAN.md.

crate::bind_format! {
    engine      = chunks_rs::formats::html,
    default     = chunk_html,
    section     = chunk_html_section,
    semantic    = chunk_html_semantic,
    sentence    = chunk_html_sentence,
    page_aware  = chunk_html_page_aware,
    sliding     = chunk_html_sliding_window,
    to_markdown = html_to_markdown,
    stream      = stream_html_chunks,
}

crate::bind_images! {
    chunk_with_images       = chunk_html_with_images,
    to_markdown_with_images = html_to_markdown_with_images,
    no_row_args,
}

/// `structural` is an alias of `default` for HTML and has always been exposed
/// as its own pyfunction; keep it rather than change the published surface.
#[pyfunction]
fn chunk_html_structural(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    __run_mode(py, file_path, "structural", 3, 1, 3, 15)
}

pub fn register_extra(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(chunk_html_structural, m)?)?;
    Ok(())
}
