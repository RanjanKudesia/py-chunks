//! `.docx` chunking + markdown pyfunctions.
//!
//! **Migrated to the vendored engine.** Binding only — see CONSOLIDATION_PLAN.md.
//!
//! Same per-mode ABI as `.doc`/`.ppt` (20 pyfunctions, 6 iterator classes), so
//! it reuses their generator. Unlike those two, the engine's `docx` already
//! carried the argument validation and the `is_word_ooxml` extension check with
//! the exact message text this binding shipped — nothing had to move into the
//! engine first.

crate::bind_per_mode_format! {
    engine = chunks_rs::formats::docx,
    chunkers = {
        structural = chunk_docx,
        section    = chunk_docx_section,
        semantic   = chunk_docx_semantic,
        sliding    = chunk_docx_sliding_window,
        sentence   = chunk_docx_sentence,
        page_aware = chunk_docx_page_aware,
    },
    streams = {
        structural = chunk_docx_structural_stream,
        section    = chunk_docx_section_stream,
        semantic   = chunk_docx_semantic_stream,
        sliding    = chunk_docx_sliding_window_stream,
        sentence   = chunk_docx_sentence_stream,
        page_aware = chunk_docx_page_aware_stream,
    },
    iterators = {
        structural = DocxStructuralIterator,
        section    = DocxSectionIterator,
        semantic   = DocxSemanticIterator,
        sliding    = DocxSlidingWindowIterator,
        sentence   = DocxSentenceIterator,
        page_aware = DocxPageAwareIterator,
    },
    images = {
        structural = chunk_docx_structural_with_images,
        section    = chunk_docx_section_with_images,
        semantic   = chunk_docx_semantic_with_images,
        sliding    = chunk_docx_sliding_window_with_images,
        sentence   = chunk_docx_sentence_with_images,
        page_aware = chunk_docx_page_aware_with_images,
    },
    to_markdown             = docx_to_markdown,
    to_markdown_with_images = docx_to_markdown_with_images,
}
