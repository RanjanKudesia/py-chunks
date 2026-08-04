//! `.ppt` chunking + markdown pyfunctions.
//!
//! **Migrated to the vendored engine.** Binding only — see CONSOLIDATION_PLAN.md.
//!
//! `.ppt` has no chunking logic of its own: the engine runs the `.doc`
//! paragraph builders over paragraphs extracted from the PowerPoint Document
//! stream. That is why the two formats had to migrate together.

crate::bind_per_mode_format! {
    engine = chunks_rs::formats::ppt,
    chunkers = {
        structural = chunk_ppt,
        section    = chunk_ppt_section,
        semantic   = chunk_ppt_semantic,
        sliding    = chunk_ppt_sliding_window,
        sentence   = chunk_ppt_sentence,
        page_aware = chunk_ppt_page_aware,
    },
    streams = {
        structural = chunk_ppt_structural_stream,
        section    = chunk_ppt_section_stream,
        semantic   = chunk_ppt_semantic_stream,
        sliding    = chunk_ppt_sliding_window_stream,
        sentence   = chunk_ppt_sentence_stream,
        page_aware = chunk_ppt_page_aware_stream,
    },
    iterators = {
        structural = PptStructuralIterator,
        section    = PptSectionIterator,
        semantic   = PptSemanticIterator,
        sliding    = PptSlidingWindowIterator,
        sentence   = PptSentenceIterator,
        page_aware = PptPageAwareIterator,
    },
    images = {
        structural = chunk_ppt_structural_with_images,
        section    = chunk_ppt_section_with_images,
        semantic   = chunk_ppt_semantic_with_images,
        sliding    = chunk_ppt_sliding_window_with_images,
        sentence   = chunk_ppt_sentence_with_images,
        page_aware = chunk_ppt_page_aware_with_images,
    },
    to_markdown             = ppt_to_markdown,
    to_markdown_with_images = ppt_to_markdown_with_images,
}
