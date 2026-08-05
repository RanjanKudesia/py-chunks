//! `.pptx` chunking + markdown pyfunctions.
//!
//! **Migrated to the vendored engine.** Binding only — see CONSOLIDATION_PLAN.md.
//!
//! `.pptx` is the hybrid of the four per-mode formats: per-mode chunkers and
//! per-mode image variants like `.doc`/`.ppt`/`.docx`, but a **single** generic
//! `stream_pptx_chunks` rather than six. It also ships two aliases that nothing
//! in the Python layer calls — they are importable from `py_chunks._rust`, so
//! removing them would be an ABI break.

crate::bind_per_mode_core! {
    engine = chunks_rs::formats::pptx,
    chunkers = {
        structural = chunk_pptx,
        section    = chunk_pptx_section,
        semantic   = chunk_pptx_semantic,
        sliding    = chunk_pptx_sliding_window,
        sentence   = chunk_pptx_sentence,
        page_aware = chunk_pptx_page_aware,
    },
    images = {
        structural = chunk_pptx_structural_with_images,
        section    = chunk_pptx_section_with_images,
        semantic   = chunk_pptx_semantic_with_images,
        sliding    = chunk_pptx_sliding_window_with_images,
        sentence   = chunk_pptx_sentence_with_images,
        page_aware = chunk_pptx_page_aware_with_images,
    },
    to_markdown             = pptx_to_markdown,
    to_markdown_with_images = pptx_to_markdown_with_images,
}

crate::bind_single_stream! {
    stream   = stream_pptx_chunks,
    iterator = PptxStreamIterator,
}

/// Alias of [`chunk_pptx`] — `default` and `structural` have always run the
/// same path. Nothing in `py_chunks` calls it; it stays because it is ABI.
#[pyfunction]
fn chunk_pptx_structural(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    __run_mode(py, file_path, "structural", 3, 1, 3, 15)
}

/// Alias of [`chunk_pptx_structural_with_images`], kept for the same reason.
#[pyfunction]
fn chunk_pptx_with_images(py: Python<'_>, file_path: &str) -> PyResult<__ImagePair> {
    __run_images(py, file_path, "structural", 3, 1, 3, 15)
}

pub fn register_aliases(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(chunk_pptx_structural, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_pptx_with_images, m)?)?;
    Ok(())
}
