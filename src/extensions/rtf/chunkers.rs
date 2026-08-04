//! `.rtf` chunking + markdown pyfunctions.
//!
//! **Migrated to the vendored engine.** Binding only — see CONSOLIDATION_PLAN.md.

crate::bind_format! {
    engine      = chunks_rs::formats::rtf,
    default     = chunk_rtf,
    section     = chunk_rtf_section,
    semantic    = chunk_rtf_semantic,
    sentence    = chunk_rtf_sentence,
    page_aware  = chunk_rtf_page_aware,
    sliding     = chunk_rtf_sliding_window,
    to_markdown = rtf_to_markdown,
    stream      = stream_rtf_chunks,
}
