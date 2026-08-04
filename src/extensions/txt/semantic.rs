/// Semantic chunker for plain text.
///
/// Prose paragraphs (plain, long, short) are merged by topic continuity using
/// ten signals in priority order.  Headings, code blocks, tables, and lists
/// are always emitted standalone — they are structurally atomic.
///
/// Heading detection is heuristic (ALL CAPS, setext underline, ATX #):
/// each detected heading resets the section context carried in metadata.

use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};
use pyo3::wrap_pyfunction;
use pythonize::pythonize;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::time::Instant;

use super::common::{
    current_section_heading, current_section_level, extract_heading_text, has_keyword_overlap,
    heading_level_txt, heading_path_strings, parse_txt_blocks, tokenize_keywords,
    update_heading_stack, ChunkRecordInput, ContentType,
};
use super::super::shared::ci_starts_with;

// ── Signal word tables ────────────────────────────────────────────────────────

const TRANSITION_BREAKS: &[&str] = &[
    "however", "nevertheless", "in contrast", "on the other hand", "meanwhile",
    "conversely", "that said", "in summary", "to summarize", "to conclude",
    "in conclusion", "to wrap up", "overall", "in closing",
];
const REFERENCE_STARTS: &[&str] = &[
    "this ", "it ", "they ", "these ", "that ", "those ", "its ", "their ",
    "such ", "the above", "the following", "the latter", "the former",
];
const ELABORATION_STARTS: &[&str] = &[
    "additionally", "furthermore", "moreover", "in addition", "what is more",
    "on top of that", "notably", "importantly", "it is worth", "equally",
    "similarly", "likewise",
];
const EXAMPLE_STARTS: &[&str] = &[
    "for example", "for instance", "such as", "e.g.", "i.e.", "as an example",
    "to illustrate", "consider ", "as shown", "as demonstrated", "take ", "imagine ",
];
const CAUSE_EFFECT_STARTS: &[&str] = &[
    "because", "therefore", "thus", "hence", "as a result", "consequently",
    "this means", "this leads", "this causes", "this results", "this implies",
    "this suggests", "so ",
];
const CONTRAST_CONTINUATION: &[&str] = &[
    "although", "even though", "despite", "whereas", "even if", "regardless",
    "notwithstanding", "while it", "while this",
];

const MAX_SEMANTIC_CHARS: usize = 1500;
const SHORT_PARA_CHARS: usize = 80;

// ── Accumulator ───────────────────────────────────────────────────────────────

struct SemPart {
    content: String,
    reason: &'static str,
}

struct SemAccum {
    parts: Vec<SemPart>,
    section_heading: Option<String>,
    heading_path: Vec<String>,
    section_level: u8,
    keywords: HashSet<String>,
    char_count: usize,
    ends_with_question: bool,
    ends_with_definition_label: bool,
}

impl SemAccum {
    fn new(content: String, sh: Option<String>, hp: Vec<String>, sl: u8, kws: HashSet<String>) -> Self {
        let cc = content.len();
        let ewq = content.trim_end().ends_with('?');
        let ewdl = content.len() <= 80 && content.trim_end().ends_with(':');
        SemAccum {
            parts: vec![SemPart { content, reason: "initial" }],
            section_heading: sh, heading_path: hp, section_level: sl,
            keywords: kws, char_count: cc,
            ends_with_question: ewq, ends_with_definition_label: ewdl,
        }
    }
    fn push(&mut self, content: String, reason: &'static str) {
        self.char_count += content.len() + 2;
        self.ends_with_question = content.trim_end().ends_with('?');
        self.ends_with_definition_label = content.len() <= 80 && content.trim_end().ends_with(':');
        self.keywords.extend(tokenize_keywords(&content));
        self.parts.push(SemPart { content, reason });
    }
    fn finalize(self, chunk_index: usize, total: usize) -> ChunkRecordInput {
        let content = self.parts.iter().map(|p| p.content.as_str()).collect::<Vec<_>>().join("\n\n");
        let mut merge_reasons: Vec<&'static str> = Vec::new();
        let mut counts: HashMap<&'static str, usize> = HashMap::new();
        for p in &self.parts {
            if p.reason != "initial" {
                if !merge_reasons.contains(&p.reason) { merge_reasons.push(p.reason); }
                *counts.entry(p.reason).or_default() += 1;
            }
        }
        let primary = if self.parts.len() <= 1 { "initial" } else {
            // Sort by (count desc, key asc) for determinism when counts are tied.
            let mut reason_vec: Vec<(&'static str, usize)> = counts.into_iter().collect();
            reason_vec.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            reason_vec.first().map(|(k, _)| *k).unwrap_or("keyword_overlap")
        };
        let para_count = self.parts.len();
        let tw = content.split_whitespace().count().max(1);
        let kd = (self.keywords.len() as f64 / tw as f64 * 1000.0).round() / 1000.0;
        let avg_bl = if self.parts.is_empty() { 0 }
        else { self.parts.iter().map(|p| p.content.len()).sum::<usize>() / self.parts.len() };
        ChunkRecordInput {
            content_type: ContentType::Semantic,
            content,
            metadata: json!({
                "section_heading":      self.section_heading,
                "heading_path":         self.heading_path,
                "section_level":        self.section_level,
                "paragraph_count":      para_count,
                "merge_reasons":        merge_reasons,
                "primary_merge_reason": primary,
                "keyword_density":      kd,
                "avg_block_length":     avg_bl,
                "chunk_index":          chunk_index,
                "document_metadata": { "source_type": "txt", "total_input_blocks": total }
            }),
        }
    }
}

fn decide_merge(clean: &str, accum: &SemAccum) -> Option<&'static str> {
    if accum.char_count + clean.len() + 2 > MAX_SEMANTIC_CHARS { return None; }
    let t = clean.trim_start();
    if TRANSITION_BREAKS.iter().any(|s| ci_starts_with(t, s)) { return None; }
    if REFERENCE_STARTS.iter().any(|s| ci_starts_with(t, s)) { return Some("reference_continuity"); }
    if ELABORATION_STARTS.iter().any(|s| ci_starts_with(t, s)) { return Some("elaboration"); }
    if EXAMPLE_STARTS.iter().any(|s| ci_starts_with(t, s)) { return Some("example"); }
    if CAUSE_EFFECT_STARTS.iter().any(|s| ci_starts_with(t, s)) { return Some("cause_effect"); }
    if CONTRAST_CONTINUATION.iter().any(|s| ci_starts_with(t, s)) { return Some("contrast_continuation"); }
    if accum.ends_with_question { return Some("question_answer"); }
    if accum.ends_with_definition_label && clean.len() > 60 { return Some("definition_expansion"); }
    if clean.len() <= SHORT_PARA_CHARS { return Some("short_paragraph"); }
    let bkw = tokenize_keywords(clean);
    if has_keyword_overlap(&accum.keywords, &bkw) { return Some("keyword_overlap"); }
    None
}

// ── Core build function ───────────────────────────────────────────────────────

pub fn build_semantic_chunks(bytes: &[u8]) -> Result<Vec<ChunkRecordInput>, String> {
    let text = crate::extensions::text_encoding::decode_text(bytes).0;
    if text.trim().is_empty() { return Err("TXT file is empty".to_string()); }

    let blocks = parse_txt_blocks(&text);
    let total = blocks.len();
    let mut result: Vec<ChunkRecordInput> = Vec::new();
    let mut heading_stack: Vec<(u8, String)> = Vec::new();
    let mut accum: Option<SemAccum> = None;
    let mut chunk_index = 0usize;

    let flush = |accum: &mut Option<SemAccum>, result: &mut Vec<ChunkRecordInput>, ci: &mut usize, total: usize| {
        if let Some(a) = accum.take() {
            result.push(a.finalize(*ci, total));
            *ci += 1;
        }
    };

    for block in &blocks {
        let is_prose = matches!(
            block.content_type,
            ContentType::PlainParagraph
                | ContentType::LongSingleParagraph
                | ContentType::ShortDisconnectedParagraph
        );

        if block.content_type == ContentType::HeadingSection {
            flush(&mut accum, &mut result, &mut chunk_index, total);
            let level = heading_level_txt(&block.content);
            let text = extract_heading_text(&block.content);
            update_heading_stack(&mut heading_stack, level, text.clone());
            result.push(ChunkRecordInput {
                content_type: ContentType::HeadingSection,
                content: text.clone(),
                metadata: json!({
                    "section_heading":      current_section_heading(&heading_stack[..heading_stack.len()-1]),
                    "heading_path":         heading_path_strings(&heading_stack),
                    "section_level":        level,
                    "paragraph_count":      0,
                    "merge_reasons":        [],
                    "primary_merge_reason": "initial",
                    "keyword_density":      0.0,
                    "avg_block_length":     text.len(),
                    "chunk_index":          chunk_index,
                    "document_metadata": { "source_type": "txt", "total_input_blocks": total }
                }),
            });
            chunk_index += 1;
        } else if !is_prose {
            // Code, table, list: standalone
            flush(&mut accum, &mut result, &mut chunk_index, total);
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
        } else {
            let clean = block.content.trim().to_string();
            if clean.is_empty() { continue; }
            let bkws = tokenize_keywords(&clean);
            match accum.as_mut() {
                None => {
                    accum = Some(SemAccum::new(
                        clean, current_section_heading(&heading_stack),
                        heading_path_strings(&heading_stack),
                        current_section_level(&heading_stack), bkws,
                    ));
                }
                Some(a) => match decide_merge(&clean, a) {
                    Some(reason) => { a.push(clean, reason); }
                    None => {
                        flush(&mut accum, &mut result, &mut chunk_index, total);
                        accum = Some(SemAccum::new(
                            clean, current_section_heading(&heading_stack),
                            heading_path_strings(&heading_stack),
                            current_section_level(&heading_stack), bkws,
                        ));
                    }
                },
            }
        }
    }
    flush(&mut accum, &mut result, &mut chunk_index, total);
    if result.is_empty() { return Err("No semantic chunks generated".to_string()); }
    Ok(result)
}

// ── PyO3 entry point ──────────────────────────────────────────────────────────

#[pyfunction]
pub fn chunk_txt_semantic(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    if !file_path.to_ascii_lowercase().ends_with(".txt") {
        return Err(PyValueError::new_err(format!("Expected .txt file path, got: {file_path}")));
    }
    let bytes = fs::read(file_path)
        .map_err(|e| PyIOError::new_err(format!("Failed to read TXT file: {e}")))?;
    let rust_start = Instant::now();
    let chunks_raw = build_semantic_chunks(&bytes)
        .map_err(|e| PyRuntimeError::new_err(format!("TXT semantic chunking failed: {e}")))?;
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
    m.add_function(wrap_pyfunction!(chunk_txt_semantic, m)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn semantic_chunks(text: &str) -> Vec<ChunkRecordInput> {
        build_semantic_chunks(text.as_bytes()).expect("semantic chunking failed")
    }

    fn prose_chunks(chunks: &[ChunkRecordInput]) -> Vec<&ChunkRecordInput> {
        chunks.iter().filter(|c| c.content_type.as_str() == "semantic").collect()
    }

    #[test]
    fn empty_input_returns_error() {
        assert!(build_semantic_chunks(b"").is_err());
        assert!(build_semantic_chunks(b"   \n").is_err());
    }

    #[test]
    fn heading_produces_heading_chunk() {
        let chunks = semantic_chunks("# Chapter\n\nSome paragraph text follows here.");
        assert!(chunks.iter().any(|c| c.content_type.as_str() == "heading"));
    }

    #[test]
    fn reference_continuity_signal_merges_paragraphs() {
        // Second paragraph starts with "This " → reference_continuity
        let text = "First paragraph has some content worth reading about.\n\nThis expands on the point above.";
        let chunks = semantic_chunks(text);
        let prose = prose_chunks(&chunks);
        assert_eq!(prose.len(), 1, "reference_continuity should merge into one chunk");
        assert!(prose[0].metadata["merge_reasons"].as_array().unwrap()
            .iter().any(|r| r.as_str() == Some("reference_continuity")));
    }

    #[test]
    fn elaboration_signal_merges_paragraphs() {
        let text = "First topic sentence introduces an idea.\n\nAdditionally this extends the idea further.";
        let chunks = semantic_chunks(text);
        let prose = prose_chunks(&chunks);
        assert_eq!(prose.len(), 1);
        assert!(prose[0].metadata["merge_reasons"].as_array().unwrap()
            .iter().any(|r| r.as_str() == Some("elaboration")));
    }

    #[test]
    fn example_signal_merges_paragraphs() {
        let text = "The concept applies broadly to many situations.\n\nFor example this is a concrete case.";
        let chunks = semantic_chunks(text);
        let prose = prose_chunks(&chunks);
        assert_eq!(prose.len(), 1);
        assert!(prose[0].metadata["merge_reasons"].as_array().unwrap()
            .iter().any(|r| r.as_str() == Some("example")));
    }

    #[test]
    fn cause_effect_signal_merges_paragraphs() {
        let text = "The system detects the error condition clearly.\n\nTherefore the process halts and logs the failure.";
        let chunks = semantic_chunks(text);
        let prose = prose_chunks(&chunks);
        assert_eq!(prose.len(), 1);
        assert!(prose[0].metadata["merge_reasons"].as_array().unwrap()
            .iter().any(|r| r.as_str() == Some("cause_effect")));
    }

    #[test]
    fn transition_break_splits_paragraphs() {
        // "However" triggers a transition break — forces a new chunk
        let text = "First paragraph presents one view of the situation.\n\nHowever the second paragraph contradicts it.";
        let chunks = semantic_chunks(text);
        let prose = prose_chunks(&chunks);
        assert_eq!(prose.len(), 2, "transition break should split into two semantic chunks");
    }

    #[test]
    fn heading_always_flushes_accumulator() {
        let text = "First paragraph before the heading here.\n\n# New Section\n\nPost-heading paragraph content.";
        let chunks = semantic_chunks(text);
        // heading + at least two semantic chunks
        let headings: Vec<_> = chunks.iter().filter(|c| c.content_type.as_str() == "heading").collect();
        assert_eq!(headings.len(), 1);
        let prose = prose_chunks(&chunks);
        assert_eq!(prose.len(), 2);
    }

    #[test]
    fn question_answer_signal_merges_paragraphs() {
        // Second paragraph must not start with a reference/elaboration/cause/contrast word
        // so the question_answer signal (lower priority) is the one that fires.
        let text = "What motivates the design of this system?\n\nDocument processing drives every decision.";
        let chunks = semantic_chunks(text);
        let prose = prose_chunks(&chunks);
        assert_eq!(prose.len(), 1);
        assert!(prose[0].metadata["merge_reasons"].as_array().unwrap()
            .iter().any(|r| r.as_str() == Some("question_answer")));
    }

    #[test]
    fn short_paragraph_signal_merges_with_previous() {
        // Second paragraph ≤ 80 chars → short_paragraph merge
        let text = "A longer first paragraph with substantive content that sets up the topic.\n\nBrief follow-up.";
        let chunks = semantic_chunks(text);
        let prose = prose_chunks(&chunks);
        assert_eq!(prose.len(), 1);
        assert!(prose[0].metadata["merge_reasons"].as_array().unwrap()
            .iter().any(|r| r.as_str() == Some("short_paragraph")));
    }

    #[test]
    fn chunk_index_increments_per_output_chunk() {
        let text = "# Section\n\nParagraph one.\n\nHowever paragraph two is separate.";
        let chunks = semantic_chunks(text);
        for (i, c) in chunks.iter().enumerate() {
            assert_eq!(c.metadata["chunk_index"], i as u64);
        }
    }

    #[test]
    fn metadata_contains_source_type_txt() {
        let text = "Some paragraph with enough content to produce at least one chunk here.";
        let chunks = semantic_chunks(text);
        for c in &chunks {
            assert_eq!(c.metadata["document_metadata"]["source_type"], "txt");
        }
    }

    #[test]
    fn max_semantic_chars_prevents_oversized_chunk() {
        // Two paragraphs whose combined length exceeds MAX_SEMANTIC_CHARS should not merge
        let para = "word ".repeat(200); // ~1000 chars each
        let text = format!("{para}\n\n{para}");
        let chunks = semantic_chunks(&text);
        let prose = prose_chunks(&chunks);
        assert!(prose.len() >= 2, "oversized pair should stay as separate chunks");
    }

    #[test]
    fn code_block_is_emitted_standalone() {
        let text = "Some intro text that sets context.\n\n```\ncode block\n```\n\nPost-code paragraph.";
        let chunks = semantic_chunks(text);
        assert!(chunks.iter().any(|c| c.content_type.as_str() == "code_block"));
    }

    #[test]
    fn keyword_overlap_signal_merges_paragraphs() {
        // Shared keyword 'distributed' should trigger keyword_overlap
        let text = "The distributed system handles network partitions gracefully.\n\nDistributed algorithms require careful coordination.";
        let chunks = semantic_chunks(text);
        let prose = prose_chunks(&chunks);
        // Should merge (keyword overlap on 'distributed')
        assert_eq!(prose.len(), 1);
    }
}
