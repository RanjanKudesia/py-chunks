//! `.ipynb` chunking + markdown pyfunctions.
//!
//! **Migrated to the vendored engine.** Binding only — see CONSOLIDATION_PLAN.md.

crate::bind_format! {
    engine      = chunks_rs::formats::ipynb,
    default     = chunk_ipynb,
    section     = chunk_ipynb_section,
    semantic    = chunk_ipynb_semantic,
    sentence    = chunk_ipynb_sentence,
    page_aware  = chunk_ipynb_page_aware,
    sliding     = chunk_ipynb_sliding_window,
    to_markdown = ipynb_to_markdown,
    stream      = stream_ipynb_chunks,
}

crate::bind_images! {
    chunk_with_images       = chunk_ipynb_with_images,
    to_markdown_with_images = ipynb_to_markdown_with_images,
}
