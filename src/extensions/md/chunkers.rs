//! `.md` chunking + markdown pyfunctions.
//!
//! **Migrated to the vendored engine.** Binding only — see CONSOLIDATION_PLAN.md.

crate::bind_format! {
    engine      = chunks_rs::formats::md,
    default     = chunk_md,
    section     = chunk_md_section,
    semantic    = chunk_md_semantic,
    sentence    = chunk_md_sentence,
    page_aware  = chunk_md_page_aware,
    sliding     = chunk_md_sliding_window,
    to_markdown = md_to_markdown,
    stream      = stream_md_chunks,
}
