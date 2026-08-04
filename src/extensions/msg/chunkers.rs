//! `.msg` chunking + markdown pyfunctions.
//!
//! **Migrated to the vendored engine.** Binding only — see CONSOLIDATION_PLAN.md.

crate::bind_format! {
    engine      = chunks_rs::formats::msg,
    default     = chunk_msg,
    section     = chunk_msg_section,
    semantic    = chunk_msg_semantic,
    sentence    = chunk_msg_sentence,
    page_aware  = chunk_msg_page_aware,
    sliding     = chunk_msg_sliding_window,
    to_markdown = msg_to_markdown,
    stream      = stream_msg_chunks,
}

crate::bind_images! {
    chunk_with_images       = chunk_msg_with_images,
    to_markdown_with_images = msg_to_markdown_with_images,
}
