/// Page-aware chunker for Markdown.
///
/// Since Markdown has no real page concept, boundaries are approximated in
/// priority order:
///   1. Heading boundary  — any heading always starts a new chunk.
///   2. Paragraph count   — once `paragraphs_per_page` prose blocks have
///                          accumulated, flush and start a new chunk.
///
/// Code blocks and tables count as one block each toward the paragraph quota,
/// but are never split across chunks.
///
/// Metadata per chunk:
///   page_break_type    — "heading_boundary" | "estimated"
///   paragraph_count    — blocks in this chunk
///   section_heading    — heading context at the start of this chunk
///   heading_path       — full ancestor breadcrumb
///   chunk_index        — 0-based position
use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};
use pyo3::wrap_pyfunction;
use pythonize::pythonize;
use serde_json::json;
use std::fs;
use std::time::Instant;

use super::common::{
    current_section_heading, current_section_level, extract_heading_text, heading_level,
    heading_path_strings, parse_markdown_blocks, split_at_paragraph_boundary, strip_block_content,
    update_heading_stack, ChunkRecordInput, ContentType, MdBlockType,
};
const MAX_PAGE_AWARE_CHUNK_CHARS: usize = 2000;

// ── Internal accumulator ──────────────────────────────────────────────────────

struct PageAccum {
    parts: Vec<String>,
    section_heading: Option<String>,
    heading_path: Vec<String>,
    section_level: u8,
    break_type: &'static str,
}

impl PageAccum {
    fn new(
        section_heading: Option<String>,
        heading_path: Vec<String>,
        section_level: u8,
        break_type: &'static str,
    ) -> Self {
        PageAccum {
            parts: Vec::new(),
            section_heading,
            heading_path,
            section_level,
            break_type,
        }
    }

    fn push(&mut self, content: String) {
        self.parts.push(content);
    }

    fn len(&self) -> usize {
        self.parts.len()
    }

    fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    fn into_content(self) -> String {
        self.parts.join("\n\n")
    }
}

// ── Core algorithm ────────────────────────────────────────────────────────────

pub fn build_page_aware_chunks(
    bytes: &[u8],
    paragraphs_per_page: usize,
) -> Result<Vec<ChunkRecordInput>, String> {
    if paragraphs_per_page == 0 {
        return Err("paragraphs_per_page must be greater than 0".to_string());
    }

    let text = std::str::from_utf8(bytes)
        .map(|s| s.to_string())
        .unwrap_or_else(|_| String::from_utf8_lossy(bytes).to_string());
    if text.trim().is_empty() {
        return Err("Markdown file is empty".to_string());
    }

    let blocks = parse_markdown_blocks(&text);
    let total_input_blocks = blocks.len();
    let mut heading_stack: Vec<(u8, String)> = Vec::new();
    let mut result: Vec<ChunkRecordInput> = Vec::new();
    let mut accum: Option<PageAccum> = None;
    let mut chunk_index = 0usize;

    let flush = |accum: &mut Option<PageAccum>,
                 result: &mut Vec<ChunkRecordInput>,
                 chunk_index: &mut usize,
                 total: usize| {
        if let Some(a) = accum.take() {
            if !a.is_empty() {
                let paragraph_count = a.len();
                let break_type = a.break_type;
                let section_heading = a.section_heading.clone();
                let heading_path = a.heading_path.clone();
                let section_level = a.section_level;
                let content = a.into_content();
                for part in split_at_paragraph_boundary(&content, MAX_PAGE_AWARE_CHUNK_CHARS) {
                    result.push(ChunkRecordInput {
                        content_type: ContentType::PageAware,
                        content: part,
                        metadata: json!({
                            "page_break_type":    break_type,
                            "paragraph_count":    paragraph_count,
                            "section_heading":    section_heading,
                            "section_level":      section_level,
                            "heading_path":       heading_path,
                            "chunk_index":        *chunk_index,
                            "document_metadata": {
                                "source_type":        "md",
                                "total_input_blocks": total,
                            }
                        }),
                    });
                    *chunk_index += 1;
                }
            }
        }
    };

    for block in blocks {
        match block.block_type {
            MdBlockType::Heading => {
                // Heading boundary: flush current page.
                flush(
                    &mut accum,
                    &mut result,
                    &mut chunk_index,
                    total_input_blocks,
                );

                let level = heading_level(&block.content);
                let text = extract_heading_text(&block.content);
                update_heading_stack(&mut heading_stack, level, text.clone());

                // Emit heading as its own chunk.
                result.push(ChunkRecordInput {
                    content_type: ContentType::HeadingSection,
                    content: text.clone(),
                    metadata: json!({
                        "page_break_type":    "heading_boundary",
                        "paragraph_count":    0,
                        "section_heading":    text,
                        "section_level":      level,
                        "heading_path":       heading_path_strings(&heading_stack),
                        "chunk_index":        chunk_index,
                        "document_metadata": {
                            "source_type":        "md",
                            "total_input_blocks": total_input_blocks,
                        }
                    }),
                });
                chunk_index += 1;

                // Start accumulator for content under this heading.
                accum = Some(PageAccum::new(
                    Some(text),
                    heading_path_strings(&heading_stack),
                    level,
                    "heading_boundary",
                ));
            }

            MdBlockType::Paragraph => {
                let clean = strip_block_content(&block.content, false);
                if clean.is_empty() {
                    continue;
                }
                let a = accum.get_or_insert_with(|| {
                    PageAccum::new(
                        current_section_heading(&heading_stack),
                        heading_path_strings(&heading_stack),
                        current_section_level(&heading_stack),
                        "estimated",
                    )
                });
                a.push(clean);
                if a.len() >= paragraphs_per_page {
                    flush(
                        &mut accum,
                        &mut result,
                        &mut chunk_index,
                        total_input_blocks,
                    );
                }
            }

            MdBlockType::List => {
                let clean = strip_block_content(&block.content, true);
                if clean.is_empty() {
                    continue;
                }
                let a = accum.get_or_insert_with(|| {
                    PageAccum::new(
                        current_section_heading(&heading_stack),
                        heading_path_strings(&heading_stack),
                        current_section_level(&heading_stack),
                        "estimated",
                    )
                });
                a.push(clean);
                if a.len() >= paragraphs_per_page {
                    flush(
                        &mut accum,
                        &mut result,
                        &mut chunk_index,
                        total_input_blocks,
                    );
                }
            }

            MdBlockType::Code => {
                let a = accum.get_or_insert_with(|| {
                    PageAccum::new(
                        current_section_heading(&heading_stack),
                        heading_path_strings(&heading_stack),
                        current_section_level(&heading_stack),
                        "estimated",
                    )
                });
                a.push(block.content);
                if a.len() >= paragraphs_per_page {
                    flush(
                        &mut accum,
                        &mut result,
                        &mut chunk_index,
                        total_input_blocks,
                    );
                }
            }

            MdBlockType::Table => {
                let a = accum.get_or_insert_with(|| {
                    PageAccum::new(
                        current_section_heading(&heading_stack),
                        heading_path_strings(&heading_stack),
                        current_section_level(&heading_stack),
                        "estimated",
                    )
                });
                a.push(block.content);
                if a.len() >= paragraphs_per_page {
                    flush(
                        &mut accum,
                        &mut result,
                        &mut chunk_index,
                        total_input_blocks,
                    );
                }
            }
        }
    }

    flush(
        &mut accum,
        &mut result,
        &mut chunk_index,
        total_input_blocks,
    );

    if result.is_empty() {
        return Err("No page-aware chunks generated".to_string());
    }
    Ok(result)
}

// ── PyO3 entry point ──────────────────────────────────────────────────────────

#[pyfunction]
pub fn chunk_md_page_aware(
    py: Python<'_>,
    file_path: &str,
    paragraphs_per_page: usize,
) -> PyResult<PyObject> {
    if !file_path.to_ascii_lowercase().ends_with(".md") {
        return Err(PyValueError::new_err(format!(
            "Expected .md file path, got: {file_path}"
        )));
    }
    if paragraphs_per_page == 0 {
        return Err(PyValueError::new_err(
            "paragraphs_per_page must be greater than 0",
        ));
    }

    let bytes = fs::read(file_path)
        .map_err(|e| PyIOError::new_err(format!("Failed to read MD file: {e}")))?;

    let rust_start = Instant::now();
    let chunks_raw = build_page_aware_chunks(&bytes, paragraphs_per_page)
        .map_err(|e| PyRuntimeError::new_err(format!("MD page-aware chunking failed: {e}")))?;
    let rust_ms = rust_start.elapsed().as_secs_f64() * 1000.0;

    let chunk_list: Vec<PyObject> = chunks_raw
        .into_iter()
        .map(|c| {
            let dict = PyDict::new_bound(py);
            dict.set_item("content", &c.content)?;
            dict.set_item("content_type", c.content_type.as_str())?;
            dict.set_item("metadata", pythonize(py, &c.metadata)?)?;
            Ok(dict.into_any().unbind())
        })
        .collect::<PyResult<_>>()?;

    let result = PyDict::new_bound(py);
    result.set_item("chunks", chunk_list)?;
    result.set_item("rust_ms", (rust_ms * 1000.0).round() / 1000.0)?;
    Ok(result.into_any().unbind())
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(chunk_md_page_aware, m)?)?;
    Ok(())
}
