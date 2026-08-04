//! `.eml` chunking + markdown pyfunctions.
//!
//! **Migrated to the vendored engine.** Binding only — see CONSOLIDATION_PLAN.md.

crate::bind_format! {
    engine      = chunks_rs::formats::eml,
    default     = chunk_eml,
    section     = chunk_eml_section,
    semantic    = chunk_eml_semantic,
    sentence    = chunk_eml_sentence,
    page_aware  = chunk_eml_page_aware,
    sliding     = chunk_eml_sliding_window,
    to_markdown = eml_to_markdown,
    stream      = stream_eml_chunks,
}

crate::bind_images! {
    chunk_with_images       = chunk_eml_with_images,
    to_markdown_with_images = eml_to_markdown_with_images,
}
