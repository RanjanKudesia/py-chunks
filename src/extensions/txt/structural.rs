use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};
use pyo3::wrap_pyfunction;
use pythonize::pythonize;
use std::fs;
use std::time::Instant;

use super::common::{
    classify_block, classify_prose, extract_heading_text, parse_txt_blocks, split_at_sentences,
    txt_metadata, ChunkRecordInput, ContentType, MAX_CHUNK_CHARS, MIN_CHUNK_CHARS,
};

// ── Prose helpers ─────────────────────────────────────────────────────────────

fn flush_prose(
    chunks: &mut Vec<ChunkRecordInput>,
    heading: &Option<String>,
    parts: &mut Vec<String>,
    len: &mut usize,
) {
    if parts.is_empty() {
        return;
    }
    let content = parts.join("\n").trim().to_string();
    parts.clear();
    *len = 0;
    if content.is_empty() {
        return;
    }
    chunks.push(ChunkRecordInput {
        content_type: classify_prose(&content),
        content,
        metadata: txt_metadata(heading.clone()),
    });
}

fn merge_short_prose(
    chunks: Vec<ChunkRecordInput>,
    min_chars: usize,
) -> Vec<ChunkRecordInput> {
    let soft_max = MAX_CHUNK_CHARS + min_chars;
    let mut result: Vec<ChunkRecordInput> = Vec::new();
    for chunk in chunks {
        let is_prose = matches!(
            chunk.content_type,
            ContentType::PlainParagraph
                | ContentType::ShortDisconnectedParagraph
                | ContentType::LongSingleParagraph
        );
        if is_prose && chunk.content.len() < min_chars {
            if let Some(prev) = result.last_mut() {
                let prev_prose = matches!(
                    prev.content_type,
                    ContentType::PlainParagraph
                        | ContentType::ShortDisconnectedParagraph
                        | ContentType::LongSingleParagraph
                );
                if prev_prose
                    && prev.content.len() + chunk.content.len() + 1 <= soft_max
                {
                    prev.content =
                        format!("{}\n{}", prev.content, chunk.content).trim().to_string();
                    prev.content_type = classify_prose(&prev.content);
                    continue;
                }
            }
        }
        result.push(chunk);
    }
    result
}

// ── Core build function ───────────────────────────────────────────────────────

pub fn build_chunks_from_txt_bytes(bytes: &[u8]) -> Result<Vec<ChunkRecordInput>, String> {
    let text = std::str::from_utf8(bytes)
        .map(|v| v.to_string())
        .unwrap_or_else(|_| String::from_utf8_lossy(bytes).to_string());

    if text.trim().is_empty() {
        return Err("TXT file is empty after decoding".to_string());
    }

    let blocks = parse_txt_blocks(&text);
    let mut chunks: Vec<ChunkRecordInput> = Vec::new();
    let mut current_heading: Option<String> = None;
    let mut prose_parts: Vec<String> = Vec::new();
    let mut prose_len: usize = 0;

    for block in &blocks {
        match block.content_type {
            ContentType::HeadingSection => {
                flush_prose(&mut chunks, &current_heading, &mut prose_parts, &mut prose_len);
                let heading_text = extract_heading_text(&block.content);
                current_heading = Some(heading_text.clone());
                chunks.push(ChunkRecordInput {
                    content_type: ContentType::HeadingSection,
                    content: heading_text,
                    metadata: txt_metadata(None),
                });
            }
            ContentType::Table
            | ContentType::CodeBlock
            | ContentType::BulletNumberedList => {
                flush_prose(&mut chunks, &current_heading, &mut prose_parts, &mut prose_len);
                chunks.push(ChunkRecordInput {
                    content_type: block.content_type,
                    content: block.content.clone(),
                    metadata: txt_metadata(current_heading.clone()),
                });
            }
            _ => {
                let sub_blocks = if block.content.len() > MAX_CHUNK_CHARS {
                    split_at_sentences(&block.content, MAX_CHUNK_CHARS)
                } else {
                    vec![block.content.clone()]
                };
                for sub in sub_blocks {
                    let add = sub.len() + 1;
                    if prose_len + add > MAX_CHUNK_CHARS && !prose_parts.is_empty() {
                        flush_prose(
                            &mut chunks,
                            &current_heading,
                            &mut prose_parts,
                            &mut prose_len,
                        );
                    }
                    prose_len += add;
                    prose_parts.push(sub);
                }
            }
        }
    }

    flush_prose(&mut chunks, &current_heading, &mut prose_parts, &mut prose_len);
    let chunks = merge_short_prose(chunks, MIN_CHUNK_CHARS);

    if chunks.is_empty() {
        return Err("No chunks generated from TXT document".to_string());
    }
    Ok(chunks)
}

// ── PyO3 entry point ──────────────────────────────────────────────────────────

#[pyfunction]
pub fn chunk_txt(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    if !file_path.to_ascii_lowercase().ends_with(".txt") {
        return Err(PyValueError::new_err(format!(
            "Expected .txt file path, got: {file_path}"
        )));
    }
    let bytes = fs::read(file_path)
        .map_err(|e| PyIOError::new_err(format!("Failed to read TXT file: {e}")))?;

    let rust_start = Instant::now();
    let chunks_raw = build_chunks_from_txt_bytes(&bytes)
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to parse TXT: {e}")))?;
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
    m.add_function(wrap_pyfunction!(chunk_txt, m)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_bytes_returns_error() {
        assert!(build_chunks_from_txt_bytes(b"").is_err());
    }

    #[test]
    fn whitespace_only_returns_error() {
        assert!(build_chunks_from_txt_bytes(b"   \n\n  ").is_err());
    }

    #[test]
    fn simple_prose_produces_at_least_one_chunk() {
        let text = b"This is a plain paragraph with enough text to be classified as prose content.";
        let chunks = build_chunks_from_txt_bytes(text).unwrap();
        assert!(!chunks.is_empty());
    }

    #[test]
    fn atx_heading_produces_heading_chunk() {
        let text = b"# Introduction\n\nSome paragraph text that follows after the heading here.";
        let chunks = build_chunks_from_txt_bytes(text).unwrap();
        assert!(chunks.iter().any(|c| c.content_type.as_str() == "heading"),
            "no heading chunk found in: {:?}", chunks.iter().map(|c| c.content_type.as_str()).collect::<Vec<_>>());
    }

    #[test]
    fn fenced_code_block_produces_code_chunk() {
        let text = b"```\nfn main() { println!(\"hello\"); }\n```";
        let chunks = build_chunks_from_txt_bytes(text).unwrap();
        assert!(chunks.iter().any(|c| c.content_type.as_str() == "code_block"));
    }

    #[test]
    fn bullet_list_produces_list_chunk() {
        let text = b"- Item one\n- Item two\n- Item three";
        let chunks = build_chunks_from_txt_bytes(text).unwrap();
        assert!(chunks.iter().any(|c| c.content_type.as_str() == "bullet_list"));
    }

    #[test]
    fn heading_chunk_metadata_has_null_section_heading() {
        let text = b"# My Title\n\nBody paragraph with sufficient length for testing purposes.";
        let chunks = build_chunks_from_txt_bytes(text).unwrap();
        let h = chunks.iter().find(|c| c.content_type.as_str() == "heading").unwrap();
        assert_eq!(h.metadata["section_heading"], serde_json::Value::Null);
        assert_eq!(h.metadata["document_metadata"]["source_type"], "txt");
    }

    #[test]
    fn large_block_is_split_within_bounds() {
        let long_text = "This is a long test sentence that keeps repeating itself. ".repeat(40);
        let chunks = build_chunks_from_txt_bytes(long_text.as_bytes()).unwrap();
        for c in &chunks {
            assert!(
                c.content.len() <= MAX_CHUNK_CHARS + 200,
                "chunk too large: {} chars", c.content.len()
            );
        }
    }

    #[test]
    fn two_short_prose_blocks_are_merged() {
        let text = b"Short para.\n\nAnother one.";
        let chunks = build_chunks_from_txt_bytes(text).unwrap();
        let prose: Vec<_> = chunks.iter().filter(|c| c.content_type.as_str() != "heading").collect();
        assert_eq!(prose.len(), 1, "two sub-min-length prose blocks should merge");
    }

    #[test]
    fn utf8_text_is_decoded_correctly() {
        let text = "Unicode: café, naïve, résumé.\n\nSecond paragraph with adequate length.";
        let chunks = build_chunks_from_txt_bytes(text.as_bytes()).unwrap();
        assert!(!chunks.is_empty());
        assert!(chunks.iter().any(|c| c.content.contains("café")));
    }
}
