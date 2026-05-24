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
    parse_txt_blocks, split_sentences, update_heading_stack, ChunkRecordInput, ContentType,
};

struct IndexedSentence {
    text: String,
    paragraph_index: usize,
    section_heading: Option<String>,
    heading_path: Vec<String>,
}

pub fn build_sentence_chunks(
    bytes: &[u8],
    sentences_per_chunk: usize,
) -> Result<Vec<ChunkRecordInput>, String> {
    if sentences_per_chunk == 0 { return Err("sentences_per_chunk must be greater than 0".to_string()); }

    let text = std::str::from_utf8(bytes)
        .map(|s| s.to_string())
        .unwrap_or_else(|_| String::from_utf8_lossy(bytes).to_string());
    if text.trim().is_empty() { return Err("TXT file is empty".to_string()); }

    let blocks = parse_txt_blocks(&text);
    let total = blocks.len();
    let mut result: Vec<ChunkRecordInput> = Vec::new();
    let mut heading_stack: Vec<(u8, String)> = Vec::new();
    let mut sentences: Vec<IndexedSentence> = Vec::new();
    let mut para_index = 0usize;
    let mut chunk_index = 0usize;

    let flush_sentences = |sentences: &mut Vec<IndexedSentence>,
                           result: &mut Vec<ChunkRecordInput>,
                           ci: &mut usize,
                           spc: usize,
                           total: usize| {
        let mut i = 0usize;
        while i < sentences.len() {
            let end = (i + spc).min(sentences.len());
            let window = &sentences[i..end];
            let content = window.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" ");
            if !content.is_empty() {
                result.push(ChunkRecordInput {
                    content_type: ContentType::Sentence,
                    content,
                    metadata: json!({
                        "sentences_per_chunk":    spc,
                        "actual_sentence_count":  window.len(),
                        "chunk_index":            *ci,
                        "source_paragraph_index": window[0].paragraph_index,
                        "section_heading":        window[0].section_heading,
                        "heading_path":           window[0].heading_path,
                        "document_metadata": { "source_type": "txt", "total_input_blocks": total }
                    }),
                });
                *ci += 1;
            }
            i = end;
        }
        sentences.clear();
    };

    let is_prose = |ct: ContentType| matches!(
        ct,
        ContentType::PlainParagraph | ContentType::LongSingleParagraph | ContentType::ShortDisconnectedParagraph
    );

    for block in &blocks {
        if block.content_type == ContentType::HeadingSection {
            flush_sentences(&mut sentences, &mut result, &mut chunk_index, sentences_per_chunk, total);
            let level = heading_level_txt(&block.content);
            let text = extract_heading_text(&block.content);
            update_heading_stack(&mut heading_stack, level, text.clone());
            result.push(ChunkRecordInput {
                content_type: ContentType::HeadingSection,
                content: text.clone(),
                metadata: json!({
                    "section_heading": text,
                    "section_level":   level,
                    "heading_path":    heading_path_strings(&heading_stack),
                    "chunk_index":     chunk_index,
                    "document_metadata": { "source_type": "txt", "total_input_blocks": total }
                }),
            });
            chunk_index += 1;
        } else if block.content_type == ContentType::CodeBlock || block.content_type == ContentType::Table {
            flush_sentences(&mut sentences, &mut result, &mut chunk_index, sentences_per_chunk, total);
            result.push(ChunkRecordInput {
                content_type: block.content_type,
                content: block.content.clone(),
                metadata: json!({
                    "section_heading": current_section_heading(&heading_stack),
                    "heading_path":    heading_path_strings(&heading_stack),
                    "chunk_index":     chunk_index,
                    "document_metadata": { "source_type": "txt", "total_input_blocks": total }
                }),
            });
            chunk_index += 1;
            para_index += 1;
        } else if block.content_type == ContentType::BulletNumberedList {
            // List items become individual "sentences".
            let sh = current_section_heading(&heading_stack);
            let hp = heading_path_strings(&heading_stack);
            let pi = para_index;
            for line in block.content.lines() {
                let t = line.trim().to_string();
                if !t.is_empty() {
                    sentences.push(IndexedSentence {
                        text: t,
                        paragraph_index: pi,
                        section_heading: sh.clone(),
                        heading_path: hp.clone(),
                    });
                }
            }
            para_index += 1;
        } else if is_prose(block.content_type) {
            let sh = current_section_heading(&heading_stack);
            let hp = heading_path_strings(&heading_stack);
            let pi = para_index;
            for s in split_sentences(&block.content) {
                sentences.push(IndexedSentence {
                    text: s, paragraph_index: pi,
                    section_heading: sh.clone(), heading_path: hp.clone(),
                });
            }
            para_index += 1;
        }
    }
    flush_sentences(&mut sentences, &mut result, &mut chunk_index, sentences_per_chunk, total);
    if result.is_empty() { return Err("No sentence chunks generated".to_string()); }
    Ok(result)
}

#[pyfunction]
pub fn chunk_txt_sentence(
    py: Python<'_>,
    file_path: &str,
    sentences_per_chunk: usize,
) -> PyResult<PyObject> {
    if !file_path.to_ascii_lowercase().ends_with(".txt") {
        return Err(PyValueError::new_err(format!("Expected .txt file path, got: {file_path}")));
    }
    if sentences_per_chunk == 0 {
        return Err(PyValueError::new_err("sentences_per_chunk must be greater than 0"));
    }
    let bytes = fs::read(file_path)
        .map_err(|e| PyIOError::new_err(format!("Failed to read TXT file: {e}")))?;
    let rust_start = Instant::now();
    let chunks_raw = build_sentence_chunks(&bytes, sentences_per_chunk)
        .map_err(|e| PyRuntimeError::new_err(format!("TXT sentence chunking failed: {e}")))?;
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
    m.add_function(wrap_pyfunction!(chunk_txt_sentence, m)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_sentences_per_chunk_returns_error() {
        assert!(build_sentence_chunks(b"Some text.", 0).is_err());
    }

    #[test]
    fn empty_input_returns_error() {
        assert!(build_sentence_chunks(b"", 3).is_err());
    }

    #[test]
    fn three_sentences_grouped_by_one() {
        let text = b"First sentence. Second sentence. Third sentence.";
        let chunks = build_sentence_chunks(text, 1).unwrap();
        let sent: Vec<_> = chunks.iter().filter(|c| c.content_type.as_str() == "sentence").collect();
        assert_eq!(sent.len(), 3, "spc=1 should produce one chunk per sentence");
    }

    #[test]
    fn six_sentences_grouped_by_two() {
        let text = b"One. Two. Three. Four. Five. Six.";
        let chunks = build_sentence_chunks(text, 2).unwrap();
        let sent: Vec<_> = chunks.iter().filter(|c| c.content_type.as_str() == "sentence").collect();
        assert_eq!(sent.len(), 3);
    }

    #[test]
    fn heading_always_emitted_standalone() {
        let text = b"# Section\n\nFirst sentence. Second sentence.";
        let chunks = build_sentence_chunks(text, 5).unwrap();
        assert!(chunks.iter().any(|c| c.content_type.as_str() == "heading"));
    }

    #[test]
    fn sentence_metadata_has_correct_sentences_per_chunk() {
        let text = b"Sentence one. Sentence two. Sentence three.";
        let chunks = build_sentence_chunks(text, 2).unwrap();
        let sent: Vec<_> = chunks.iter().filter(|c| c.content_type.as_str() == "sentence").collect();
        assert_eq!(sent[0].metadata["sentences_per_chunk"], 2);
    }

    #[test]
    fn last_chunk_may_have_fewer_sentences() {
        // 3 sentences with spc=2 → chunk1 has 2, chunk2 has 1
        let text = b"Sent one. Sent two. Sent three.";
        let chunks = build_sentence_chunks(text, 2).unwrap();
        let sent: Vec<_> = chunks.iter().filter(|c| c.content_type.as_str() == "sentence").collect();
        let last = sent.last().unwrap();
        assert!(last.metadata["actual_sentence_count"].as_u64().unwrap() <= 2);
    }

    #[test]
    fn bullet_list_items_become_individual_sentences() {
        let text = b"- First bullet\n- Second bullet\n- Third bullet";
        let chunks = build_sentence_chunks(text, 1).unwrap();
        let sent: Vec<_> = chunks.iter().filter(|c| c.content_type.as_str() == "sentence").collect();
        assert_eq!(sent.len(), 3);
    }

    #[test]
    fn source_type_is_txt() {
        let text = b"One sentence. Two sentence.";
        let chunks = build_sentence_chunks(text, 1).unwrap();
        for c in &chunks {
            assert_eq!(c.metadata["document_metadata"]["source_type"], "txt");
        }
    }
}
