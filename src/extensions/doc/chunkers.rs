//! `.doc` chunking + markdown pyfunctions.
//!
//! **Migrated to the vendored engine.** Binding only — see CONSOLIDATION_PLAN.md.

crate::bind_per_mode_format! {
    engine = chunks_rs::formats::doc,
    chunkers = {
        structural = chunk_doc,
        section    = chunk_doc_section,
        semantic   = chunk_doc_semantic,
        sliding    = chunk_doc_sliding_window,
        sentence   = chunk_doc_sentence,
        page_aware = chunk_doc_page_aware,
    },
    streams = {
        structural = chunk_doc_structural_stream,
        section    = chunk_doc_section_stream,
        semantic   = chunk_doc_semantic_stream,
        sliding    = chunk_doc_sliding_window_stream,
        sentence   = chunk_doc_sentence_stream,
        page_aware = chunk_doc_page_aware_stream,
    },
    iterators = {
        structural = DocStructuralIterator,
        section    = DocSectionIterator,
        semantic   = DocSemanticIterator,
        sliding    = DocSlidingWindowIterator,
        sentence   = DocSentenceIterator,
        page_aware = DocPageAwareIterator,
    },
    images = {
        structural = chunk_doc_structural_with_images,
        section    = chunk_doc_section_with_images,
        semantic   = chunk_doc_semantic_with_images,
        sliding    = chunk_doc_sliding_window_with_images,
        sentence   = chunk_doc_sentence_with_images,
        page_aware = chunk_doc_page_aware_with_images,
    },
    to_markdown             = doc_to_markdown,
    to_markdown_with_images = doc_to_markdown_with_images,
}
