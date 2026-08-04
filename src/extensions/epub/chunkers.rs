//! `.epub` chunking + markdown pyfunctions.
//!
//! **Migrated to the vendored engine.** Binding only — see CONSOLIDATION_PLAN.md.

crate::bind_format! {
    engine      = chunks_rs::formats::epub,
    default     = chunk_epub,
    section     = chunk_epub_section,
    semantic    = chunk_epub_semantic,
    sentence    = chunk_epub_sentence,
    page_aware  = chunk_epub_page_aware,
    sliding     = chunk_epub_sliding_window,
    to_markdown = epub_to_markdown,
    stream      = stream_epub_chunks,
}

crate::bind_images! {
    chunk_with_images       = chunk_epub_with_images,
    to_markdown_with_images = epub_to_markdown_with_images,
}
