//! `.json` / `.jsonl` / `.ndjson` chunking + markdown pyfunctions.
//!
//! **Migrated to the vendored engine.** Binding only — see CONSOLIDATION_PLAN.md.

crate::bind_format! {
    engine      = chunks_rs::formats::json,
    default     = chunk_json,
    section     = chunk_json_section,
    semantic    = chunk_json_semantic,
    sentence    = chunk_json_sentence,
    page_aware  = chunk_json_page_aware,
    sliding     = chunk_json_sliding_window,
    to_markdown = json_to_markdown,
    stream      = stream_json_chunks,
}
