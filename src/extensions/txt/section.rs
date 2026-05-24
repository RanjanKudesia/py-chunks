/// Section chunker for plain text.
///
/// Detects heading blocks heuristically (ALL CAPS, setext underline, ATX #)
/// and groups every block that follows into one section chunk until the next
/// heading.  Sections exceeding MAX_SECTION_CHARS are split at paragraph
/// boundaries.
///
/// Metadata per chunk:
///   section_heading  — heading text that opened this section
///   section_level    — inferred from underline style / ATX depth
///   heading_path     — full ancestor breadcrumb
///   paragraph_count  — non-heading blocks accumulated
///   block_types      — unique block types present
///   char_count       — total content characters

use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};
use pyo3::wrap_pyfunction;
use pythonize::pythonize;
use serde_json::json;
use std::fs;
use std::time::Instant;

use super::common::{
    current_section_level, extract_heading_text, heading_level_txt, heading_path_strings,
    parse_txt_blocks, update_heading_stack, ChunkRecordInput, ContentType,
};

const MAX_SECTION_CHARS: usize = 2000;

// ── Section body accumulator ──────────────────────────────────────────────────

struct SectionBody {
    parts: Vec<(String, &'static str)>,
    section_heading: String,
    section_level: u8,
    heading_path: Vec<String>,
}

impl SectionBody {
    fn joined(&self) -> String {
        self.parts.iter().map(|(c, _)| c.as_str()).collect::<Vec<_>>().join("\n\n")
    }
    fn char_count(&self) -> usize {
        self.parts.iter().map(|(c, _)| c.len()).sum::<usize>()
            + self.parts.len().saturating_sub(1) * 2
    }
    fn block_types(&self) -> Vec<&'static str> {
        let mut seen: Vec<&'static str> = Vec::new();
        for (_, t) in &self.parts { if !seen.contains(t) { seen.push(t); } }
        seen
    }
    fn paragraph_count(&self) -> usize {
        self.parts.iter().filter(|(_, t)| *t == "paragraph" || *t == "list").count()
    }
}

fn split_large_section(text: &str, max_chars: usize) -> Vec<String> {
    if text.len() <= max_chars { return vec![text.trim().to_string()]; }
    let mut chunks = Vec::new();
    let mut current = String::new();
    for para in text.split("\n\n") {
        let candidate = if current.is_empty() { para.to_string() }
        else { format!("{}\n\n{}", current, para) };
        if candidate.len() <= max_chars { current = candidate; }
        else {
            if !current.is_empty() { chunks.push(current.trim().to_string()); }
            current = para.to_string();
        }
    }
    if !current.trim().is_empty() { chunks.push(current.trim().to_string()); }
    if chunks.is_empty() { vec![text.trim().to_string()] } else { chunks }
}

fn flush_section(
    result: &mut Vec<ChunkRecordInput>,
    body: SectionBody,
    chunk_index: &mut usize,
    total: usize,
) {
    let joined = body.joined();
    if joined.trim().is_empty() { return; }
    let block_types = body.block_types();
    let paragraph_count = body.paragraph_count();
    let parts = split_large_section(&joined, MAX_SECTION_CHARS);
    let part_count = parts.len();
    for (i, content) in parts.into_iter().enumerate() {
        if content.is_empty() { continue; }
        result.push(ChunkRecordInput {
            content_type: ContentType::Section,
            content: content.clone(),
            metadata: json!({
                "section_heading":  body.section_heading,
                "section_level":    body.section_level,
                "heading_path":     body.heading_path,
                "paragraph_count":  paragraph_count,
                "block_types":      block_types,
                "char_count":       content.len(),
                "split_part":       if part_count > 1 { json!(i + 1) } else { serde_json::Value::Null },
                "split_total":      if part_count > 1 { json!(part_count) } else { serde_json::Value::Null },
                "chunk_index":      *chunk_index,
                "document_metadata": { "source_type": "txt", "total_input_blocks": total }
            }),
        });
        *chunk_index += 1;
    }
}

fn block_type_str(ct: ContentType) -> &'static str {
    match ct {
        ContentType::PlainParagraph
        | ContentType::LongSingleParagraph
        | ContentType::ShortDisconnectedParagraph => "paragraph",
        ContentType::BulletNumberedList => "list",
        ContentType::CodeBlock => "code_block",
        ContentType::Table => "table",
        _ => "other",
    }
}

// ── Core build function ───────────────────────────────────────────────────────

pub fn build_section_chunks(bytes: &[u8]) -> Result<Vec<ChunkRecordInput>, String> {
    let text = std::str::from_utf8(bytes)
        .map(|s| s.to_string())
        .unwrap_or_else(|_| String::from_utf8_lossy(bytes).to_string());
    if text.trim().is_empty() { return Err("TXT file is empty".to_string()); }

    let blocks = parse_txt_blocks(&text);
    let total = blocks.len();
    let mut result: Vec<ChunkRecordInput> = Vec::new();
    let mut heading_stack: Vec<(u8, String)> = Vec::new();
    let mut current: Option<SectionBody> = None;
    let mut chunk_index = 0usize;

    for block in &blocks {
        if block.content_type == ContentType::HeadingSection {
            if let Some(body) = current.take() {
                flush_section(&mut result, body, &mut chunk_index, total);
            }
            let level = heading_level_txt(&block.content);
            let text = extract_heading_text(&block.content);
            update_heading_stack(&mut heading_stack, level, text.clone());

            result.push(ChunkRecordInput {
                content_type: ContentType::HeadingSection,
                content: text.clone(),
                metadata: json!({
                    "section_heading":  text.clone(),
                    "section_level":    level,
                    "heading_path":     heading_path_strings(&heading_stack),
                    "paragraph_count":  0,
                    "block_types":      ["heading"],
                    "char_count":       text.len(),
                    "chunk_index":      chunk_index,
                    "document_metadata": { "source_type": "txt", "total_input_blocks": total }
                }),
            });
            chunk_index += 1;

            current = Some(SectionBody {
                parts: Vec::new(),
                section_heading: text,
                section_level: level,
                heading_path: heading_path_strings(&heading_stack),
            });
        } else {
            let bts = block_type_str(block.content_type);
            let a = current.get_or_insert_with(|| SectionBody {
                parts: Vec::new(),
                section_heading: "Preamble".to_string(),
                section_level: current_section_level(&heading_stack),
                heading_path: heading_path_strings(&heading_stack),
            });
            if a.char_count() + block.content.len() + 2 > MAX_SECTION_CHARS && !a.parts.is_empty() {
                let next_heading = a.section_heading.clone();
                let next_level = a.section_level;
                let next_path = a.heading_path.clone();
                let body = current.take().unwrap();
                flush_section(&mut result, body, &mut chunk_index, total);
                current = Some(SectionBody {
                    parts: Vec::new(),
                    section_heading: next_heading,
                    section_level: next_level,
                    heading_path: next_path,
                });
                current.as_mut().unwrap().parts.push((block.content.clone(), bts));
            } else {
                a.parts.push((block.content.clone(), bts));
            }
        }
    }

    if let Some(body) = current.take() {
        flush_section(&mut result, body, &mut chunk_index, total);
    }
    if result.is_empty() { return Err("No section chunks generated".to_string()); }
    Ok(result)
}

// ── PyO3 entry point ──────────────────────────────────────────────────────────

#[pyfunction]
pub fn chunk_txt_section(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    if !file_path.to_ascii_lowercase().ends_with(".txt") {
        return Err(PyValueError::new_err(format!("Expected .txt file path, got: {file_path}")));
    }
    let bytes = fs::read(file_path)
        .map_err(|e| PyIOError::new_err(format!("Failed to read TXT file: {e}")))?;
    let rust_start = Instant::now();
    let chunks_raw = build_section_chunks(&bytes)
        .map_err(|e| PyRuntimeError::new_err(format!("TXT section chunking failed: {e}")))?;
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
    m.add_function(wrap_pyfunction!(chunk_txt_section, m)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── split_large_section ───────────────────────────────────────────────────

    #[test]
    fn split_large_section_short_text_unchanged() {
        let text = "Short section text.";
        let parts = split_large_section(text, 1000);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0], text.trim());
    }

    #[test]
    fn split_large_section_splits_at_paragraph_boundary() {
        let para = "paragraph text here. ".repeat(20); // ~420 chars each
        let text = format!("{}\n\n{}", para, para);
        let parts = split_large_section(&text, 500);
        assert!(parts.len() >= 2, "should split into at least 2 parts");
    }

    #[test]
    fn split_large_section_never_returns_empty_strings() {
        let para = "x".repeat(300);
        let text = format!("{}\n\n{}\n\n{}", para, para, para);
        let parts = split_large_section(&text, 500);
        assert!(parts.iter().all(|p| !p.is_empty()));
    }

    // ── build_section_chunks ──────────────────────────────────────────────────

    #[test]
    fn empty_input_returns_error() {
        assert!(build_section_chunks(b"").is_err());
    }

    #[test]
    fn pre_heading_content_uses_preamble_section_heading() {
        let text = b"Some text before any heading.\n\nMore preamble text here.";
        let chunks = build_section_chunks(text).unwrap();
        let section: Vec<_> = chunks.iter().filter(|c| c.content_type.as_str() == "section").collect();
        assert!(!section.is_empty());
        assert_eq!(
            section[0].metadata["section_heading"].as_str().unwrap(),
            "Preamble"
        );
    }

    #[test]
    fn heading_creates_heading_chunk_then_section_chunk() {
        let text = b"# My Section\n\nParagraph under the section heading content.";
        let chunks = build_section_chunks(text).unwrap();
        assert!(chunks.iter().any(|c| c.content_type.as_str() == "heading"));
        let section: Vec<_> = chunks.iter().filter(|c| c.content_type.as_str() == "section").collect();
        assert!(!section.is_empty());
        assert_eq!(
            section[0].metadata["section_heading"].as_str().unwrap(),
            "My Section"
        );
    }

    #[test]
    fn chunk_index_is_sequential() {
        let text = b"# Section A\n\nParagraph under A.\n\n# Section B\n\nParagraph under B.";
        let chunks = build_section_chunks(text).unwrap();
        for (i, c) in chunks.iter().enumerate() {
            assert_eq!(c.metadata["chunk_index"], i as u64);
        }
    }

    #[test]
    fn source_type_is_txt() {
        let text = b"Some content paragraph here.\n\nAnother one.";
        let chunks = build_section_chunks(text).unwrap();
        for c in &chunks {
            assert_eq!(c.metadata["document_metadata"]["source_type"], "txt");
        }
    }
}
