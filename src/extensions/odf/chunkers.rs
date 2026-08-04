//! `.odt` / `.odp` chunking + markdown pyfunctions.
//!
//! **Migrated to the vendored engine.** Binding only — see CONSOLIDATION_PLAN.md.

crate::bind_format! {
    engine      = chunks_rs::formats::odf,
    default     = chunk_odf,
    section     = chunk_odf_section,
    semantic    = chunk_odf_semantic,
    sentence    = chunk_odf_sentence,
    page_aware  = chunk_odf_page_aware,
    sliding     = chunk_odf_sliding_window,
    to_markdown = odf_to_markdown,
    stream      = stream_odf_chunks,
}

crate::bind_images! {
    chunk_with_images       = chunk_odf_with_images,
    to_markdown_with_images = odf_to_markdown_with_images,
}
