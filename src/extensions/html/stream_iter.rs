/// Streaming iterator for all HTML chunking strategies.
///
/// `structural` and `semantic` use true block-by-block state machines.
/// `section`, `sliding_window`, `sentence`, `page_aware` use batch-drain.

use std::collections::{HashMap, HashSet, VecDeque};

use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyModule;
use pyo3::wrap_pyfunction;
use pythonize::pythonize;
use serde_json::json;
use std::fs;

use super::common::{
    classify_prose, current_section_heading, current_section_level, has_keyword_overlap,
    heading_path_strings, html_metadata, html_metadata_with_path, parse_html_blocks,
    remove_comments, split_at_sentences, tokenize_keywords, update_heading_stack,
    ChunkRecordInput, ContentType, HtmlBlock, HtmlBlockType, MAX_CHUNK_CHARS, MIN_CHUNK_CHARS,
};
use super::super::shared::{
    ci_starts_with, CAUSE_EFFECT_STARTS, CONTRAST_CONTINUATION, ELABORATION_STARTS,
    EXAMPLE_STARTS, REFERENCE_STARTS, SHORT_PARA_CHARS, TRANSITION_BREAKS,
};
use super::page_aware::build_page_aware_chunks;
use super::section::build_section_chunks;
use super::sentence::build_sentence_chunks;
use super::sliding_window::build_sliding_window_chunks;

fn chunk_to_pydict(py: Python<'_>, c: &ChunkRecordInput) -> PyResult<PyObject> {
    let dict = pyo3::types::PyDict::new_bound(py);
    dict.set_item("content", &c.content)?;
    dict.set_item("content_type", c.content_type.as_str())?;
    dict.set_item("metadata", pythonize(py, &c.metadata)?)?;
    Ok(dict.into_any().unbind())
}

// ── Structural state machine ──────────────────────────────────────────────────

struct StructuralState {
    blocks: Vec<HtmlBlock>, index: usize,
    current_heading: Option<String>,
    prose_parts: Vec<String>, prose_len: usize,
    pending_prose: Option<ChunkRecordInput>, done: bool,
}
impl StructuralState {
    fn new(blocks: Vec<HtmlBlock>) -> Self {
        StructuralState { blocks, index: 0, current_heading: None, prose_parts: Vec::new(), prose_len: 0, pending_prose: None, done: false }
    }
    fn flush_prose(&mut self) -> Vec<ChunkRecordInput> {
        if self.prose_parts.is_empty() { return Vec::new(); }
        let content = self.prose_parts.join("\n").trim().to_string();
        self.prose_parts.clear(); self.prose_len = 0;
        if content.is_empty() { return Vec::new(); }
        let ct = classify_prose(&content);
        let chunk = ChunkRecordInput { content_type: ct, content, metadata: html_metadata(self.current_heading.clone()) };
        self.merge_prose(chunk)
    }
    fn merge_prose(&mut self, chunk: ChunkRecordInput) -> Vec<ChunkRecordInput> {
        let soft_max = MAX_CHUNK_CHARS + MIN_CHUNK_CHARS;
        let is_prose = |ct: ContentType| matches!(ct, ContentType::PlainParagraph | ContentType::ShortDisconnectedParagraph | ContentType::LongSingleParagraph);
        if is_prose(chunk.content_type) && chunk.content.len() < MIN_CHUNK_CHARS {
            if let Some(ref mut prev) = self.pending_prose {
                if is_prose(prev.content_type) && prev.content.len() + chunk.content.len() + 1 <= soft_max {
                    prev.content = format!("{}\n{}", prev.content, chunk.content).trim().to_string();
                    prev.content_type = classify_prose(&prev.content);
                    return Vec::new();
                }
            }
            let old = self.pending_prose.replace(chunk);
            return old.into_iter().collect();
        }
        let old = self.pending_prose.take();
        let mut out: Vec<ChunkRecordInput> = old.into_iter().collect();
        out.push(chunk);
        out
    }
    fn drain_pending(&mut self) -> Vec<ChunkRecordInput> { self.pending_prose.take().into_iter().collect() }
    fn advance(&mut self) -> Option<Vec<ChunkRecordInput>> {
        if self.done { return None; }
        if self.index >= self.blocks.len() {
            self.done = true;
            let mut out = self.flush_prose();
            out.extend(self.drain_pending());
            return Some(out);
        }
        let block = self.blocks[self.index].clone();
        self.index += 1;
        match block.block_type {
            HtmlBlockType::Heading => {
                let mut out = self.flush_prose(); out.extend(self.drain_pending());
                self.current_heading = Some(block.content.clone());
                out.push(ChunkRecordInput { content_type: ContentType::HeadingSection, content: block.content, metadata: html_metadata(None) });
                Some(out)
            }
            HtmlBlockType::Code => {
                let mut out = self.flush_prose(); out.extend(self.drain_pending());
                out.push(ChunkRecordInput { content_type: ContentType::CodeBlock, content: block.content, metadata: html_metadata(self.current_heading.clone()) });
                Some(out)
            }
            HtmlBlockType::Table => {
                let mut out = self.flush_prose(); out.extend(self.drain_pending());
                out.push(ChunkRecordInput { content_type: ContentType::Table, content: block.content, metadata: html_metadata(self.current_heading.clone()) });
                Some(out)
            }
            HtmlBlockType::List => {
                let mut out = self.flush_prose(); out.extend(self.drain_pending());
                if !block.content.is_empty() {
                    out.push(ChunkRecordInput { content_type: ContentType::BulletNumberedList, content: block.content, metadata: html_metadata(self.current_heading.clone()) });
                }
                Some(out)
            }
            HtmlBlockType::Paragraph => {
                if block.content.is_empty() { return Some(Vec::new()); }
                let parts = if block.content.len() > MAX_CHUNK_CHARS { split_at_sentences(&block.content, MAX_CHUNK_CHARS) } else { vec![block.content] };
                let mut out = Vec::new();
                for part in parts {
                    let add = part.len() + 1;
                    if self.prose_len + add > MAX_CHUNK_CHARS && !self.prose_parts.is_empty() { out.extend(self.flush_prose()); }
                    self.prose_len += add; self.prose_parts.push(part);
                }
                Some(out)
            }
        }
    }
}

// ── Semantic state machine ────────────────────────────────────────────────────

const MAX_SEMANTIC_CHARS: usize = 1500;
// Signal constants imported from shared — identical to html/semantic.rs so
// batch and streaming always produce the same merge decisions.

struct SemPart { content: String, reason: &'static str }
struct SemAccum {
    parts: Vec<SemPart>, section_heading: Option<String>, heading_path: Vec<String>,
    section_level: u8, keywords: HashSet<String>, char_count: usize,
    ends_with_question: bool, ends_with_definition_label: bool,
}
impl SemAccum {
    fn new(content: String, sh: Option<String>, hp: Vec<String>, sl: u8, kws: HashSet<String>) -> Self {
        let cc = content.len(); let ewq = content.trim_end().ends_with('?');
        let ewdl = content.len() <= 80 && content.trim_end().ends_with(':');
        SemAccum { parts: vec![SemPart { content, reason: "initial" }], section_heading: sh, heading_path: hp, section_level: sl, keywords: kws, char_count: cc, ends_with_question: ewq, ends_with_definition_label: ewdl }
    }
    fn push(&mut self, content: String, reason: &'static str) {
        self.char_count += content.len() + 2; self.ends_with_question = content.trim_end().ends_with('?');
        self.ends_with_definition_label = content.len() <= 80 && content.trim_end().ends_with(':');
        self.keywords.extend(tokenize_keywords(&content));
        self.parts.push(SemPart { content, reason });
    }
    fn finalize(self, ci: usize, total: usize) -> ChunkRecordInput {
        let content = self.parts.iter().map(|p| p.content.as_str()).collect::<Vec<_>>().join("\n\n");
        let mut merge_reasons: Vec<&'static str> = Vec::new();
        let mut counts: HashMap<&'static str, usize> = HashMap::new();
        for p in &self.parts { if p.reason != "initial" { if !merge_reasons.contains(&p.reason) { merge_reasons.push(p.reason); } *counts.entry(p.reason).or_default() += 1; } }
        let primary = if self.parts.len() <= 1 { "initial" } else {
            // Sort by (count desc, key asc) for determinism when counts are tied.
            let mut reason_vec: Vec<(&'static str, usize)> = counts.into_iter().collect();
            reason_vec.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            reason_vec.first().map(|(k, _)| *k).unwrap_or("keyword_overlap")
        };
        let tw = content.split_whitespace().count().max(1);
        let kd = (self.keywords.len() as f64 / tw as f64 * 1000.0).round() / 1000.0;
        ChunkRecordInput {
            content_type: ContentType::Semantic, content,
            metadata: json!({ "section_heading": self.section_heading, "heading_path": self.heading_path, "section_level": self.section_level, "paragraph_count": self.parts.len(), "merge_reasons": merge_reasons, "primary_merge_reason": primary, "keyword_density": kd, "chunk_index": ci, "document_metadata": { "source_type": "html", "total_input_blocks": total } }),
        }
    }
}
fn sem_decide(clean: &str, acc: &SemAccum) -> Option<&'static str> {
    if acc.char_count + clean.len() + 2 > MAX_SEMANTIC_CHARS { return None; }
    let t = clean.trim_start();
    if TRANSITION_BREAKS.iter().any(|s| ci_starts_with(t, s)) { return None; }
    if REFERENCE_STARTS.iter().any(|s| ci_starts_with(t, s)) { return Some("reference_continuity"); }
    if ELABORATION_STARTS.iter().any(|s| ci_starts_with(t, s)) { return Some("elaboration"); }
    if EXAMPLE_STARTS.iter().any(|s| ci_starts_with(t, s)) { return Some("example"); }
    if CAUSE_EFFECT_STARTS.iter().any(|s| ci_starts_with(t, s)) { return Some("cause_effect"); }
    if CONTRAST_CONTINUATION.iter().any(|s| ci_starts_with(t, s)) { return Some("contrast_continuation"); }
    if acc.ends_with_question { return Some("question_answer"); }
    if acc.ends_with_definition_label && clean.len() > 60 { return Some("definition_expansion"); }
    if clean.len() <= SHORT_PARA_CHARS { return Some("short_paragraph"); }
    let bkw = tokenize_keywords(clean);
    if has_keyword_overlap(&acc.keywords, &bkw) { return Some("keyword_overlap"); }
    None
}

struct SemanticState {
    blocks: Vec<HtmlBlock>, index: usize, total: usize,
    heading_stack: Vec<(u8, String)>, accum: Option<SemAccum>,
    chunk_index: usize, done: bool,
}
impl SemanticState {
    fn new(blocks: Vec<HtmlBlock>) -> Self {
        let total = blocks.len();
        SemanticState { blocks, index: 0, total, heading_stack: Vec::new(), accum: None, chunk_index: 0, done: false }
    }
    fn flush_accum(&mut self) -> Option<ChunkRecordInput> {
        self.accum.take().map(|a| { let c = a.finalize(self.chunk_index, self.total); self.chunk_index += 1; c })
    }
    fn advance(&mut self) -> Option<Vec<ChunkRecordInput>> {
        if self.done { return None; }
        if self.index >= self.blocks.len() { self.done = true; return Some(self.flush_accum().into_iter().collect()); }
        let block = self.blocks[self.index].clone(); self.index += 1;
        match block.block_type {
            HtmlBlockType::Heading => {
                let mut out: Vec<ChunkRecordInput> = self.flush_accum().into_iter().collect();
                update_heading_stack(&mut self.heading_stack, block.heading_level, block.content.clone());
                out.push(ChunkRecordInput {
                    content_type: ContentType::HeadingSection, content: block.content.clone(),
                    metadata: json!({ "section_heading": current_section_heading(&self.heading_stack[..self.heading_stack.len()-1]), "heading_path": heading_path_strings(&self.heading_stack), "section_level": block.heading_level, "paragraph_count": 0, "merge_reasons": [], "primary_merge_reason": "initial", "keyword_density": 0.0, "chunk_index": self.chunk_index, "document_metadata": { "source_type": "html", "total_input_blocks": self.total } }),
                });
                self.chunk_index += 1; Some(out)
            }
            HtmlBlockType::Code | HtmlBlockType::Table | HtmlBlockType::List => {
                let mut out: Vec<ChunkRecordInput> = self.flush_accum().into_iter().collect();
                let ct = match block.block_type { HtmlBlockType::Code => ContentType::CodeBlock, HtmlBlockType::Table => ContentType::Table, _ => ContentType::BulletNumberedList };
                out.push(ChunkRecordInput { content_type: ct, content: block.content.clone(), metadata: html_metadata_with_path(current_section_heading(&self.heading_stack), heading_path_strings(&self.heading_stack), current_section_level(&self.heading_stack)) });
                self.chunk_index += 1; Some(out)
            }
            HtmlBlockType::Paragraph => {
                let clean = block.content.trim().to_string();
                if clean.is_empty() { return Some(Vec::new()); }
                let bkws = tokenize_keywords(&clean);
                match self.accum.as_mut() {
                    None => { self.accum = Some(SemAccum::new(clean, current_section_heading(&self.heading_stack), heading_path_strings(&self.heading_stack), current_section_level(&self.heading_stack), bkws)); Some(Vec::new()) }
                    Some(a) => match sem_decide(&clean, a) {
                        Some(reason) => { a.push(clean, reason); Some(Vec::new()) }
                        None => { let old = self.flush_accum(); self.accum = Some(SemAccum::new(clean, current_section_heading(&self.heading_stack), heading_path_strings(&self.heading_stack), current_section_level(&self.heading_stack), bkws)); Some(old.into_iter().collect()) }
                    },
                }
            }
        }
    }
}

// ── Batch drain ───────────────────────────────────────────────────────────────

struct BatchDrainState { chunks: Vec<ChunkRecordInput>, index: usize }
impl BatchDrainState {
    fn new(chunks: Vec<ChunkRecordInput>) -> Self { BatchDrainState { chunks, index: 0 } }
    fn advance(&mut self) -> Option<&ChunkRecordInput> {
        if self.index >= self.chunks.len() { return None; }
        let c = &self.chunks[self.index]; self.index += 1; Some(c)
    }
}

// ── Backend enum ──────────────────────────────────────────────────────────────

enum HtmlStreamBackend { Structural(StructuralState), Semantic(SemanticState), Batch(BatchDrainState) }

// ── Iterator pyclass ──────────────────────────────────────────────────────────

#[pyclass]
pub struct HtmlStreamIterator {
    backend: HtmlStreamBackend,
    pending: VecDeque<ChunkRecordInput>,
    done: bool,
}

#[pymethods]
impl HtmlStreamIterator {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> { slf }
    fn __next__(&mut self, py: Python<'_>) -> PyResult<Option<PyObject>> {
        loop {
            if let Some(chunk) = self.pending.pop_front() { return chunk_to_pydict(py, &chunk).map(Some); }
            if self.done { return Ok(None); }
            match &mut self.backend {
                HtmlStreamBackend::Structural(s) => match s.advance() { None => { self.done = true; } Some(chunks) => { self.pending.extend(chunks); } },
                HtmlStreamBackend::Semantic(s) => match s.advance() { None => { self.done = true; } Some(chunks) => { self.pending.extend(chunks); } },
                HtmlStreamBackend::Batch(s) => match s.advance() { None => { self.done = true; } Some(chunk) => { return chunk_to_pydict(py, chunk).map(Some); } },
            }
        }
    }
}

// ── Factory pyfunction ────────────────────────────────────────────────────────

#[pyfunction]
pub fn stream_html_chunks(
    py: Python<'_>, file_path: &str, mode: &str,
    window_size: usize, overlap: usize, sentences_per_chunk: usize, paragraphs_per_page: usize,
) -> PyResult<Py<HtmlStreamIterator>> {
    let lower = file_path.to_ascii_lowercase();
    if !lower.ends_with(".html") && !lower.ends_with(".htm") {
        return Err(PyValueError::new_err(format!("Expected .html/.htm, got: {file_path}")));
    }
    match mode {
        "default"|"structural"|"semantic"|"section"|"sliding_window"|"sentence"|"page_aware" => {}
        _ => return Err(PyValueError::new_err(format!("Unknown HTML streaming mode: {mode}"))),
    }
    if mode == "sliding_window" {
        if window_size == 0 { return Err(PyValueError::new_err("window_size must be > 0")); }
        if overlap >= window_size { return Err(PyValueError::new_err("overlap must be < window_size")); }
    }
    if mode == "sentence" && sentences_per_chunk == 0 { return Err(PyValueError::new_err("sentences_per_chunk must be > 0")); }
    if mode == "page_aware" && paragraphs_per_page == 0 { return Err(PyValueError::new_err("paragraphs_per_page must be > 0")); }

    let bytes = fs::read(file_path).map_err(|e| PyIOError::new_err(format!("{e}")))?;

    let backend = match mode {
        "default" | "structural" => {
            let text = std::str::from_utf8(&bytes).map(|s| s.to_string()).unwrap_or_else(|_| String::from_utf8_lossy(&bytes).to_string());
            let blocks = parse_html_blocks(&remove_comments(&text));
            HtmlStreamBackend::Structural(StructuralState::new(blocks))
        }
        "semantic" => {
            let text = std::str::from_utf8(&bytes).map(|s| s.to_string()).unwrap_or_else(|_| String::from_utf8_lossy(&bytes).to_string());
            let blocks = parse_html_blocks(&remove_comments(&text));
            HtmlStreamBackend::Semantic(SemanticState::new(blocks))
        }
        "section" => HtmlStreamBackend::Batch(BatchDrainState::new(build_section_chunks(&bytes).map_err(PyRuntimeError::new_err)?)),
        "sliding_window" => HtmlStreamBackend::Batch(BatchDrainState::new(build_sliding_window_chunks(&bytes, window_size, overlap).map_err(PyRuntimeError::new_err)?)),
        "sentence" => HtmlStreamBackend::Batch(BatchDrainState::new(build_sentence_chunks(&bytes, sentences_per_chunk).map_err(PyRuntimeError::new_err)?)),
        "page_aware" => HtmlStreamBackend::Batch(BatchDrainState::new(build_page_aware_chunks(&bytes, paragraphs_per_page).map_err(PyRuntimeError::new_err)?)),
        _ => unreachable!(),
    };

    Py::new(py, HtmlStreamIterator { backend, pending: VecDeque::new(), done: false })
}

pub fn register(m: &pyo3::Bound<'_, PyModule>) -> pyo3::PyResult<()> {
    m.add_class::<HtmlStreamIterator>()?;
    m.add_function(wrap_pyfunction!(stream_html_chunks, m)?)?;
    Ok(())
}
