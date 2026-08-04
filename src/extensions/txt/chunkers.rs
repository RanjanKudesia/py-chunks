//! `.txt` chunking + markdown pyfunctions.
//!
//! **Migrated to the vendored engine.** Binding only — see CONSOLIDATION_PLAN.md.

crate::bind_format! {
    engine      = chunks_rs::formats::txt,
    default     = chunk_txt,
    section     = chunk_txt_section,
    semantic    = chunk_txt_semantic,
    sentence    = chunk_txt_sentence,
    page_aware  = chunk_txt_page_aware,
    sliding     = chunk_txt_sliding_window,
    to_markdown = txt_to_markdown,
    stream      = stream_txt_chunks,
}
