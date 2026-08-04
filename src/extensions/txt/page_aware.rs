use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};
use pyo3::wrap_pyfunction;
use pythonize::pythonize;
use serde_json::json;
use std::fs;
use std::time::Instant;

use super::common::{
    current_section_heading, extract_heading_text, heading_level_txt, heading_path_strings,
    parse_txt_blocks, update_heading_stack, ChunkRecordInput, ContentType,
};

struct PageAccum {
    parts: Vec<String>,
    section_heading: Option<String>,
    heading_path: Vec<String>,
    break_type: &'static str,
}

impl PageAccum {
    fn new(sh: Option<String>, hp: Vec<String>, bt: &'static str) -> Self {
        PageAccum { parts: Vec::new(), section_heading: sh, heading_path: hp, break_type: bt }
    }
    fn push(&mut self, s: String) { self.parts.push(s); }
    fn len(&self) -> usize { self.parts.len() }
    fn is_empty(&self) -> bool { self.parts.is_empty() }
    fn into_content(self) -> String { self.parts.join("\n\n") }
}

pub fn build_page_aware_chunks(
    bytes: &[u8],
    paragraphs_per_page: usize,
) -> Result<Vec<ChunkRecordInput>, String> {
    if paragraphs_per_page == 0 { return Err("paragraphs_per_page must be greater than 0".to_string()); }
    let text = crate::extensions::text_encoding::decode_text(bytes).0;
    if text.trim().is_empty() { return Err("TXT file is empty".to_string()); }

    let blocks = parse_txt_blocks(&text);
    let total = blocks.len();
    let mut heading_stack: Vec<(u8, String)> = Vec::new();
    let mut result: Vec<ChunkRecordInput> = Vec::new();
    let mut accum: Option<PageAccum> = None;
    let mut chunk_index = 0usize;

    let flush = |accum: &mut Option<PageAccum>,
                 result: &mut Vec<ChunkRecordInput>,
                 ci: &mut usize,
                 total: usize| {
        if let Some(a) = accum.take() {
            if !a.is_empty() {
                let paragraph_count = a.len();
                let break_type = a.break_type;
                let sh = a.section_heading.clone();
                let hp = a.heading_path.clone();
                let content = a.into_content();
                result.push(ChunkRecordInput {
                    content_type: ContentType::PageAware,
                    content,
                    metadata: json!({
                        "page_break_type":   break_type,
                        "paragraph_count":   paragraph_count,
                        "section_heading":   sh,
                        "heading_path":      hp,
                        "chunk_index":       *ci,
                        "document_metadata": { "source_type": "txt", "total_input_blocks": total }
                    }),
                });
                *ci += 1;
            }
        }
    };

    for block in &blocks {
        if block.content_type == ContentType::HeadingSection {
            flush(&mut accum, &mut result, &mut chunk_index, total);
            let level = heading_level_txt(&block.content);
            let text = extract_heading_text(&block.content);
            update_heading_stack(&mut heading_stack, level, text.clone());
            result.push(ChunkRecordInput {
                content_type: ContentType::HeadingSection,
                content: text.clone(),
                metadata: json!({
                    "page_break_type":  "heading_boundary",
                    "paragraph_count":  0,
                    "section_heading":  text,
                    "section_level":    level,
                    "heading_path":     heading_path_strings(&heading_stack),
                    "chunk_index":      chunk_index,
                    "document_metadata": { "source_type": "txt", "total_input_blocks": total }
                }),
            });
            chunk_index += 1;
            accum = Some(PageAccum::new(
                current_section_heading(&heading_stack),
                heading_path_strings(&heading_stack),
                "heading_boundary",
            ));
        } else {
            let a = accum.get_or_insert_with(|| PageAccum::new(
                current_section_heading(&heading_stack),
                heading_path_strings(&heading_stack),
                "estimated",
            ));
            a.push(block.content.clone());
            if a.len() >= paragraphs_per_page {
                flush(&mut accum, &mut result, &mut chunk_index, total);
            }
        }
    }
    flush(&mut accum, &mut result, &mut chunk_index, total);
    if result.is_empty() { return Err("No page-aware chunks generated".to_string()); }
    Ok(result)
}

#[pyfunction]
pub fn chunk_txt_page_aware(
    py: Python<'_>,
    file_path: &str,
    paragraphs_per_page: usize,
) -> PyResult<PyObject> {
    if !file_path.to_ascii_lowercase().ends_with(".txt") {
        return Err(PyValueError::new_err(format!("Expected .txt file path, got: {file_path}")));
    }
    if paragraphs_per_page == 0 {
        return Err(PyValueError::new_err("paragraphs_per_page must be greater than 0"));
    }
    let bytes = fs::read(file_path)
        .map_err(|e| PyIOError::new_err(format!("Failed to read TXT file: {e}")))?;
    let rust_start = Instant::now();
    let chunks_raw = build_page_aware_chunks(&bytes, paragraphs_per_page)
        .map_err(|e| PyRuntimeError::new_err(format!("TXT page-aware chunking failed: {e}")))?;
    let rust_ms = rust_start.elapsed().as_secs_f64() * 1000.0;
    let chunk_list: Vec<PyObject> = chunks_raw.into_iter().map(|c| {
        let dict = PyDict::new_bound(py);
        dict.set_item("content", &c.content)?;
        dict.set_item("content_type", c.content_type.as_str())?;
        dict.set_item("metadata", pythonize(py, &c.metadata)?)?;
        Ok(dict.into_any().unbind())
    }).collect::<PyResult<_>>()?;
    let result = PyDict::new_bound(py);
    result.set_item("chunks", chunk_list)?;
    result.set_item("rust_ms", (rust_ms * 1000.0).round() / 1000.0)?;
    Ok(result.into_any().unbind())
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(chunk_txt_page_aware, m)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn four_para_text() -> &'static str {
        "First paragraph.\n\nSecond paragraph.\n\nThird paragraph.\n\nFourth paragraph."
    }

    #[test]
    fn zero_paragraphs_per_page_returns_error() {
        assert!(build_page_aware_chunks(b"text", 0).is_err());
    }

    #[test]
    fn empty_input_returns_error() {
        assert!(build_page_aware_chunks(b"", 3).is_err());
    }

    #[test]
    fn paragraphs_per_page_1_produces_one_chunk_per_block() {
        let text = four_para_text();
        let chunks = build_page_aware_chunks(text.as_bytes(), 1).unwrap();
        let page: Vec<_> = chunks.iter().filter(|c| c.content_type.as_str() == "page_aware").collect();
        assert_eq!(page.len(), 4);
    }

    #[test]
    fn paragraphs_per_page_2_groups_correctly() {
        let text = four_para_text();
        let chunks = build_page_aware_chunks(text.as_bytes(), 2).unwrap();
        let page: Vec<_> = chunks.iter().filter(|c| c.content_type.as_str() == "page_aware").collect();
        assert_eq!(page.len(), 2);
    }

    #[test]
    fn heading_flushes_accumulator_and_emits_standalone() {
        let text = "# Heading\n\nFirst para.\n\nSecond para.";
        let chunks = build_page_aware_chunks(text.as_bytes(), 5).unwrap();
        assert!(chunks.iter().any(|c| c.content_type.as_str() == "heading"));
    }

    #[test]
    fn break_type_estimated_for_preamble_content() {
        let text = four_para_text();
        let chunks = build_page_aware_chunks(text.as_bytes(), 2).unwrap();
        let page: Vec<_> = chunks.iter().filter(|c| c.content_type.as_str() == "page_aware").collect();
        assert_eq!(page[0].metadata["page_break_type"], "estimated");
    }

    #[test]
    fn break_type_heading_boundary_after_heading() {
        let text = "# Section\n\nParagraph after heading content.";
        let chunks = build_page_aware_chunks(text.as_bytes(), 5).unwrap();
        let page: Vec<_> = chunks.iter().filter(|c| c.content_type.as_str() == "page_aware").collect();
        if !page.is_empty() {
            assert_eq!(page[0].metadata["page_break_type"], "heading_boundary");
        }
    }

    #[test]
    fn metadata_paragraph_count_matches_grouped_blocks() {
        let text = four_para_text();
        let chunks = build_page_aware_chunks(text.as_bytes(), 2).unwrap();
        let page: Vec<_> = chunks.iter().filter(|c| c.content_type.as_str() == "page_aware").collect();
        assert_eq!(page[0].metadata["paragraph_count"], 2);
    }
}
