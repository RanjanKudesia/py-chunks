/// True streaming iterator for all Markdown chunking strategies.
///
/// Unlike PDF streaming (which uses a background thread to work around
/// pdfium's lifetime constraints), MD streaming uses a direct Rust state
/// machine — no threads, no channels.  Each `__next__` call advances the
/// state machine by exactly one block and drains one chunk from the
/// `VecDeque` pending queue.
///
/// Memory profile:
///   - All strategies: O(parsed_blocks) for the input Vec<MdBlock>
///   - Pending queue:  O(chunks produced in one block step, usually 0–2)
///   - No O(all_chunks) allocation — chunks are produced and handed to Python
///     one at a time.
///
/// Parity guarantee: yields identical content and metadata to the
/// corresponding batch function for every strategy.
use std::collections::{HashSet, VecDeque};

use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyModule;
use pyo3::wrap_pyfunction;
use pythonize::pythonize;
use serde_json::json;
use std::fs;

use super::super::shared::{
    ci_starts_with, CAUSE_EFFECT_STARTS, CONTRAST_CONTINUATION, ELABORATION_STARTS, EXAMPLE_STARTS,
    MAX_SEMANTIC_CHARS, REFERENCE_STARTS, TRANSITION_BREAKS,
};
use super::common::{
    classify_prose, current_section_heading, current_section_level, extract_heading_text,
    has_keyword_overlap, heading_level, heading_path_strings,
    parse_markdown_blocks, split_at_sentences, strip_block_content,
    tokenize_keywords, update_heading_stack, ChunkRecordInput, ContentType, MdBlock, MdBlockType,
    MAX_CHUNK_CHARS, MIN_CHUNK_CHARS,
};

use super::page_aware::build_page_aware_chunks;
use super::section::build_section_chunks;
use super::sentence::build_sentence_chunks;
use super::sliding_window::build_sliding_window_chunks;

// ── Pending-chunk queue ───────────────────────────────────────────────────────

fn chunk_to_pydict(py: Python<'_>, c: &ChunkRecordInput) -> PyResult<PyObject> {
    let dict = pyo3::types::PyDict::new_bound(py);
    dict.set_item("content", &c.content)?;
    dict.set_item("content_type", c.content_type.as_str())?;
    dict.set_item("metadata", pythonize(py, &c.metadata)?)?;
    Ok(dict.into_any().unbind())
}

// ── State structs ─────────────────────────────────────────────────────────────

// Each state struct owns a Vec<MdBlock> and a cursor.  The advance_*
// functions process one block per call and return 0-N chunks ready to emit.

// ─── Structural ───────────────────────────────────────────────────────────────

struct StructuralState {
    blocks: Vec<MdBlock>,
    index: usize,
    heading_stack: Vec<(u8, String)>,
    current_heading: Option<String>,
    current_section_level: u8,
    prose_parts: Vec<String>,
    prose_len: usize,
    /// Last prose chunk held for short-chunk merging (merge_short_prose).
    pending_prose: Option<ChunkRecordInput>,
    done: bool,
}

impl StructuralState {
    fn new(blocks: Vec<MdBlock>) -> Self {
        StructuralState {
            blocks,
            index: 0,
            heading_stack: Vec::new(),
            current_heading: None,
            current_section_level: 0,
            prose_parts: Vec::new(),
            prose_len: 0,
            pending_prose: None,
            done: false,
        }
    }

    fn md_meta(&self) -> serde_json::Value {
        json!({
            "section_heading": self.current_heading,
            "section_level":   self.current_section_level,
            "document_metadata": { "source_type": "md" }
        })
    }

    fn flush_prose(&mut self) -> Vec<ChunkRecordInput> {
        if self.prose_parts.is_empty() {
            return Vec::new();
        }
        let content = self.prose_parts.join("\n").trim().to_string();
        self.prose_parts.clear();
        self.prose_len = 0;
        if content.is_empty() {
            return Vec::new();
        }
        let ct = classify_prose(&content);
        let chunk = ChunkRecordInput {
            content_type: ct,
            content,
            metadata: self.md_meta(),
        };
        self.merge_prose_chunk(chunk)
    }

    /// Implements merge_short_prose: holds a short chunk in reserve and
    /// merges it with the next prose chunk if possible.
    fn merge_prose_chunk(&mut self, chunk: ChunkRecordInput) -> Vec<ChunkRecordInput> {
        let soft_max = MAX_CHUNK_CHARS + MIN_CHUNK_CHARS;
        let is_prose = matches!(
            chunk.content_type,
            ContentType::PlainParagraph
                | ContentType::ShortDisconnectedParagraph
                | ContentType::LongSingleParagraph
        );
        if is_prose && chunk.content.len() < MIN_CHUNK_CHARS {
            if let Some(ref mut prev) = self.pending_prose {
                let prev_is_prose = matches!(
                    prev.content_type,
                    ContentType::PlainParagraph
                        | ContentType::ShortDisconnectedParagraph
                        | ContentType::LongSingleParagraph
                );
                if prev_is_prose && prev.content.len() + chunk.content.len() + 1 <= soft_max {
                    prev.content = format!("{}\n{}", prev.content, chunk.content)
                        .trim()
                        .to_string();
                    prev.content_type = classify_prose(&prev.content);
                    return Vec::new(); // merged, nothing new to emit
                }
            }
            // Swap chunk into pending, emit whatever was there.
            let old = self.pending_prose.replace(chunk);
            return old.into_iter().collect();
        }
        // Non-short chunk: emit pending + this chunk.
        let old = self.pending_prose.take();
        let mut out: Vec<ChunkRecordInput> = old.into_iter().collect();
        out.push(chunk);
        out
    }

    fn drain_pending_prose(&mut self) -> Vec<ChunkRecordInput> {
        self.pending_prose.take().into_iter().collect()
    }

    /// Advance by one block.  Returns chunks ready for the queue (may be empty).
    /// Returns None when exhausted.
    fn advance(&mut self) -> Option<Vec<ChunkRecordInput>> {
        if self.done {
            return None;
        }
        if self.index >= self.blocks.len() {
            self.done = true;
            let mut out = self.flush_prose();
            out.extend(self.drain_pending_prose());
            return Some(out);
        }

        let block = self.blocks[self.index].clone();
        self.index += 1;

        match block.block_type {
            MdBlockType::Heading => {
                let mut out = self.flush_prose();
                out.extend(self.drain_pending_prose());
                let level = heading_level(&block.content);
                let text = extract_heading_text(&block.content);
                update_heading_stack(&mut self.heading_stack, level, text.clone());
                self.current_heading = Some(text.clone());
                self.current_section_level = level;
                out.push(ChunkRecordInput {
                    content_type: ContentType::HeadingSection,
                    content: text,
                    metadata: json!({
                        "section_heading": serde_json::Value::Null,
                        "section_level": level,
                        "document_metadata": { "source_type": "md" }
                    }),
                });
                Some(out)
            }
            MdBlockType::Code => {
                let mut out = self.flush_prose();
                out.extend(self.drain_pending_prose());
                out.push(ChunkRecordInput {
                    content_type: ContentType::CodeBlock,
                    content: block.content,
                    metadata: self.md_meta(),
                });
                Some(out)
            }
            MdBlockType::Table => {
                let mut out = self.flush_prose();
                out.extend(self.drain_pending_prose());
                out.push(ChunkRecordInput {
                    content_type: ContentType::Table,
                    content: block.content,
                    metadata: self.md_meta(),
                });
                Some(out)
            }
            MdBlockType::List => {
                let mut out = self.flush_prose();
                out.extend(self.drain_pending_prose());
                let clean = strip_block_content(&block.content, true);
                if !clean.is_empty() {
                    out.push(ChunkRecordInput {
                        content_type: ContentType::BulletNumberedList,
                        content: clean,
                        metadata: self.md_meta(),
                    });
                }
                Some(out)
            }
            MdBlockType::Paragraph => {
                let clean = strip_block_content(&block.content, false);
                if clean.is_empty() {
                    return Some(Vec::new());
                }
                let sub_blocks = if clean.len() > MAX_CHUNK_CHARS {
                    split_at_sentences(&clean, MAX_CHUNK_CHARS)
                } else {
                    vec![clean]
                };
                let mut out = Vec::new();
                for sub in sub_blocks {
                    let add = sub.len() + 1;
                    if self.prose_len + add > MAX_CHUNK_CHARS && !self.prose_parts.is_empty() {
                        out.extend(self.flush_prose());
                    }
                    self.prose_len += add;
                    self.prose_parts.push(sub);
                }
                Some(out)
            }
        }
    }
}

// ─── Semantic ─────────────────────────────────────────────────────────────────

// Signal word tables — imported from super::super::shared (single source of truth).
// See shared.rs for the full definitions and comments on each signal.

struct SemPart {
    content: String,
    block_type: MdBlockType,
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
    fn new(
        content: String,
        bt: MdBlockType,
        sh: Option<String>,
        hp: Vec<String>,
        sl: u8,
        kws: HashSet<String>,
    ) -> Self {
        let char_count = content.len();
        let eq = content.trim_end().ends_with('?');
        let edl = content.len() <= 80 && content.trim_end().ends_with(':');
        SemAccum {
            parts: vec![SemPart {
                content,
                block_type: bt,
                reason: "initial",
            }],
            section_heading: sh,
            heading_path: hp,
            section_level: sl,
            keywords: kws,
            char_count,
            ends_with_question: eq,
            ends_with_definition_label: edl,
        }
    }

    fn push(&mut self, content: String, bt: MdBlockType, reason: &'static str) {
        self.char_count += content.len() + 2;
        self.ends_with_question = content.trim_end().ends_with('?');
        self.ends_with_definition_label = content.len() <= 80 && content.trim_end().ends_with(':');
        self.keywords.extend(tokenize_keywords(&content));
        self.parts.push(SemPart {
            content,
            block_type: bt,
            reason,
        });
    }

    fn finalize(self, chunk_index: usize, total: usize) -> ChunkRecordInput {
        let content = self
            .parts
            .iter()
            .map(|p| p.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");

        let mut block_types: Vec<&'static str> = Vec::new();
        let mut merge_reasons: Vec<&'static str> = Vec::new();
        let mut reason_counts: std::collections::HashMap<&'static str, usize> =
            std::collections::HashMap::new();
        let mut has_list = false;
        let mut para_count = 0usize;
        for p in &self.parts {
            let t = match p.block_type {
                MdBlockType::Paragraph => "paragraph",
                MdBlockType::List => "list",
                _ => "other",
            };
            if !block_types.contains(&t) {
                block_types.push(t);
            }
            if p.reason != "initial" && !merge_reasons.contains(&p.reason) {
                merge_reasons.push(p.reason);
            }
            if p.reason != "initial" {
                *reason_counts.entry(p.reason).or_default() += 1;
            }
            if matches!(p.block_type, MdBlockType::List) {
                has_list = true;
            }
            if matches!(p.block_type, MdBlockType::Paragraph | MdBlockType::List) {
                para_count += 1;
            }
        }
        let primary = if self.parts.len() <= 1 {
            "initial"
        } else {
            // Sort by (count desc, key asc) for determinism when counts are tied.
            let mut reason_vec: Vec<(&'static str, usize)> =
                reason_counts.iter().map(|(k, v)| (*k, *v)).collect();
            reason_vec.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            reason_vec.first().map(|(k, _)| *k).unwrap_or("keyword_overlap")
        };
        let tw = content.split_whitespace().count().max(1);
        let kd = (self.keywords.len() as f64 / tw as f64 * 1000.0).round() / 1000.0;
        let avg_bl = if self.parts.is_empty() {
            0
        } else {
            self.parts.iter().map(|p| p.content.len()).sum::<usize>() / self.parts.len()
        };
        ChunkRecordInput {
            content_type: ContentType::Semantic,
            content,
            metadata: json!({
                "section_heading":         self.section_heading,
                "heading_path":            self.heading_path,
                "section_level":           self.section_level,
                "paragraph_count":         para_count,
                "block_types":             block_types,
                "merge_reasons":           merge_reasons,
                "primary_merge_reason":    primary,
                "has_list":                has_list,
                "keyword_density":         kd,
                "avg_block_length":        avg_bl,
                "chunk_index":             chunk_index,
                "document_metadata": {
                    "source_type":         "md",
                    "total_input_blocks":  total,
                }
            }),
        }
    }
}

fn sem_decide(clean: &str, bt: MdBlockType, acc: &SemAccum) -> Option<&'static str> {
    if acc.char_count + clean.len() + 2 > MAX_SEMANTIC_CHARS {
        return None;
    }
    let t = clean.trim_start();
    if TRANSITION_BREAKS.iter().any(|s| ci_starts_with(t, s)) {
        return None;
    }
    if REFERENCE_STARTS.iter().any(|s| ci_starts_with(t, s)) {
        return Some("reference_continuity");
    }
    if ELABORATION_STARTS.iter().any(|s| ci_starts_with(t, s)) {
        return Some("elaboration");
    }
    if EXAMPLE_STARTS.iter().any(|s| ci_starts_with(t, s)) {
        return Some("example");
    }
    if CAUSE_EFFECT_STARTS.iter().any(|s| ci_starts_with(t, s)) {
        return Some("cause_effect");
    }
    if CONTRAST_CONTINUATION.iter().any(|s| ci_starts_with(t, s)) {
        return Some("contrast_continuation");
    }
    if acc.ends_with_question {
        return Some("question_answer");
    }
    if acc.ends_with_definition_label && clean.len() > 60 {
        return Some("definition_expansion");
    }
    if clean.len() <= 80 {
        return Some("short_paragraph");
    }
    let bkw = tokenize_keywords(clean);
    if matches!(bt, MdBlockType::List) {
        if has_keyword_overlap(&acc.keywords, &bkw) {
            return Some("list_continuation");
        }
    }
    if has_keyword_overlap(&acc.keywords, &bkw) {
        return Some("keyword_overlap");
    }
    None
}

struct SemanticState {
    blocks: Vec<MdBlock>,
    index: usize,
    total: usize,
    heading_stack: Vec<(u8, String)>,
    accum: Option<SemAccum>,
    chunk_index: usize,
    done: bool,
}

impl SemanticState {
    fn new(blocks: Vec<MdBlock>) -> Self {
        let total = blocks.len();
        SemanticState {
            blocks,
            index: 0,
            total,
            heading_stack: Vec::new(),
            accum: None,
            chunk_index: 0,
            done: false,
        }
    }

    fn flush_accum(&mut self) -> Option<ChunkRecordInput> {
        self.accum.take().map(|a| {
            let c = a.finalize(self.chunk_index, self.total);
            self.chunk_index += 1;
            c
        })
    }

    fn heading_chunk(&mut self, level: u8, text: String) -> ChunkRecordInput {
        let path = heading_path_strings(&self.heading_stack);
        let parent_heading = if self.heading_stack.len() > 1 {
            current_section_heading(&self.heading_stack[..self.heading_stack.len() - 1])
        } else {
            None
        };
        let c = ChunkRecordInput {
            content_type: ContentType::HeadingSection,
            content: text.clone(),
            metadata: json!({
                "section_heading":      parent_heading,
                "heading_path":         path,
                "section_level":        level,
                "paragraph_count":      0,
                "block_types":          ["heading"],
                "merge_reasons":        [],
                "primary_merge_reason": "initial",
                "has_list":             false,
                "keyword_density":      0.0,
                "avg_block_length":     text.len(),
                "chunk_index":          self.chunk_index,
                "document_metadata": {
                    "source_type":        "md",
                    "total_input_blocks": self.total,
                }
            }),
        };
        self.chunk_index += 1;
        c
    }

    fn standalone_chunk(
        &mut self,
        content: String,
        ct: ContentType,
        reason: &'static str,
    ) -> ChunkRecordInput {
        let c = ChunkRecordInput {
            content_type: ct,
            content: content.clone(),
            metadata: json!({
                "section_heading":      current_section_heading(&self.heading_stack),
                "heading_path":         heading_path_strings(&self.heading_stack),
                "section_level":        current_section_level(&self.heading_stack),
                "paragraph_count":      0,
                "block_types":          [ct.as_str()],
                "merge_reasons":        [],
                "primary_merge_reason": reason,
                "has_list":             false,
                "keyword_density":      0.0,
                "avg_block_length":     content.len(),
                "chunk_index":          self.chunk_index,
                "document_metadata": {
                    "source_type":        "md",
                    "total_input_blocks": self.total,
                }
            }),
        };
        self.chunk_index += 1;
        c
    }

    fn advance(&mut self) -> Option<Vec<ChunkRecordInput>> {
        if self.done {
            return None;
        }
        if self.index >= self.blocks.len() {
            self.done = true;
            let c = self.flush_accum();
            return Some(c.into_iter().collect());
        }

        let block = self.blocks[self.index].clone();
        self.index += 1;

        match block.block_type {
            MdBlockType::Heading => {
                let mut out: Vec<ChunkRecordInput> = self.flush_accum().into_iter().collect();
                let level = heading_level(&block.content);
                let text = extract_heading_text(&block.content);
                update_heading_stack(&mut self.heading_stack, level, text.clone());
                out.push(self.heading_chunk(level, text));
                Some(out)
            }
            MdBlockType::Code => {
                let mut out: Vec<ChunkRecordInput> = self.flush_accum().into_iter().collect();
                out.push(self.standalone_chunk(
                    block.content,
                    ContentType::CodeBlock,
                    "structural_boundary",
                ));
                Some(out)
            }
            MdBlockType::Table => {
                let mut out: Vec<ChunkRecordInput> = self.flush_accum().into_iter().collect();
                out.push(self.standalone_chunk(
                    block.content,
                    ContentType::Table,
                    "structural_boundary",
                ));
                Some(out)
            }
            MdBlockType::Paragraph | MdBlockType::List => {
                let clean = strip_block_content(
                    &block.content,
                    matches!(block.block_type, MdBlockType::List),
                );
                if clean.is_empty() {
                    return Some(Vec::new());
                }
                let bkws = tokenize_keywords(&clean);
                match self.accum.as_mut() {
                    None => {
                        self.accum = Some(SemAccum::new(
                            clean,
                            block.block_type,
                            current_section_heading(&self.heading_stack),
                            heading_path_strings(&self.heading_stack),
                            current_section_level(&self.heading_stack),
                            bkws,
                        ));
                        Some(Vec::new())
                    }
                    Some(a) => match sem_decide(&clean, block.block_type, a) {
                        Some(reason) => {
                            a.push(clean, block.block_type, reason);
                            Some(Vec::new())
                        }
                        None => {
                            let old = self.flush_accum();
                            self.accum = Some(SemAccum::new(
                                clean,
                                block.block_type,
                                current_section_heading(&self.heading_stack),
                                heading_path_strings(&self.heading_stack),
                                current_section_level(&self.heading_stack),
                                bkws,
                            ));
                            Some(old.into_iter().collect())
                        }
                    },
                }
            }
        }
    }
}

// ─── Backend enum ─────────────────────────────────────────────────────────────

/// For modes whose state machines are complex or pre-batch-optimised
/// (section, sliding_window, sentence, page_aware), we compute all chunks
/// upfront then drain them lazily — identical memory as calling the batch
/// function but yields one chunk per __next__.
struct BatchDrainState {
    chunks: Vec<ChunkRecordInput>,
    index: usize,
}

impl BatchDrainState {
    fn new(chunks: Vec<ChunkRecordInput>) -> Self {
        BatchDrainState { chunks, index: 0 }
    }
    fn advance(&mut self) -> Option<&ChunkRecordInput> {
        if self.index >= self.chunks.len() {
            return None;
        }
        let c = &self.chunks[self.index];
        self.index += 1;
        Some(c)
    }
}

enum MdStreamBackend {
    Structural(StructuralState),
    Semantic(SemanticState),
    Batch(BatchDrainState),
}

// ── Iterator pyclass ──────────────────────────────────────────────────────────

#[pyclass]
pub struct MdStreamIterator {
    backend: MdStreamBackend,
    pending: VecDeque<ChunkRecordInput>,
    done: bool,
}

#[pymethods]
impl MdStreamIterator {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(&mut self, py: Python<'_>) -> PyResult<Option<PyObject>> {
        loop {
            // 1. Drain pending queue first.
            if let Some(chunk) = self.pending.pop_front() {
                return chunk_to_pydict(py, &chunk).map(Some);
            }

            if self.done {
                return Ok(None);
            }

            // 2. Advance state machine.
            match &mut self.backend {
                MdStreamBackend::Structural(s) => match s.advance() {
                    None => {
                        self.done = true;
                    }
                    Some(chunks) => {
                        self.pending.extend(chunks);
                    }
                },
                MdStreamBackend::Semantic(s) => match s.advance() {
                    None => {
                        self.done = true;
                    }
                    Some(chunks) => {
                        self.pending.extend(chunks);
                    }
                },
                MdStreamBackend::Batch(s) => match s.advance() {
                    None => {
                        self.done = true;
                    }
                    Some(chunk) => {
                        return chunk_to_pydict(py, chunk).map(Some);
                    }
                },
            }
        }
    }
}

// ── Public factory function ───────────────────────────────────────────────────

#[pyfunction]
pub fn stream_md_chunks(
    py: Python<'_>,
    file_path: &str,
    mode: &str,
    window_size: usize,
    overlap: usize,
    sentences_per_chunk: usize,
    paragraphs_per_page: usize,
) -> PyResult<Py<MdStreamIterator>> {
    if !file_path.to_ascii_lowercase().ends_with(".md") {
        return Err(PyValueError::new_err(format!(
            "Expected .md file path, got: {file_path}"
        )));
    }

    match mode {
        "default" | "structural" | "semantic" | "section" | "sliding_window" | "sentence"
        | "page_aware" => {}
        _ => {
            return Err(PyValueError::new_err(format!(
                "Unknown MD streaming mode: {mode}"
            )));
        }
    }

    if mode == "sliding_window" {
        if window_size == 0 {
            return Err(PyValueError::new_err("window_size must be greater than 0"));
        }
        if overlap >= window_size {
            return Err(PyValueError::new_err(
                "overlap must be less than window_size",
            ));
        }
    }
    if mode == "sentence" && sentences_per_chunk == 0 {
        return Err(PyValueError::new_err(
            "sentences_per_chunk must be greater than 0",
        ));
    }
    if mode == "page_aware" && paragraphs_per_page == 0 {
        return Err(PyValueError::new_err(
            "paragraphs_per_page must be greater than 0",
        ));
    }

    let bytes = fs::read(file_path)
        .map_err(|e| PyIOError::new_err(format!("Failed to read MD file: {e}")))?;

    let backend = match mode {
        "default" | "structural" => {
            let text = std::str::from_utf8(&bytes)
                .map(|s| s.to_string())
                .unwrap_or_else(|_| String::from_utf8_lossy(&bytes).to_string());
            let blocks = parse_markdown_blocks(&text);
            MdStreamBackend::Structural(StructuralState::new(blocks))
        }
        "semantic" => {
            let text = std::str::from_utf8(&bytes)
                .map(|s| s.to_string())
                .unwrap_or_else(|_| String::from_utf8_lossy(&bytes).to_string());
            let blocks = parse_markdown_blocks(&text);
            MdStreamBackend::Semantic(SemanticState::new(blocks))
        }
        "section" => {
            let chunks = build_section_chunks(&bytes).map_err(|e| PyRuntimeError::new_err(e))?;
            MdStreamBackend::Batch(BatchDrainState::new(chunks))
        }
        "sliding_window" => {
            let chunks = build_sliding_window_chunks(&bytes, window_size, overlap)
                .map_err(|e| PyRuntimeError::new_err(e))?;
            MdStreamBackend::Batch(BatchDrainState::new(chunks))
        }
        "sentence" => {
            let chunks = build_sentence_chunks(&bytes, sentences_per_chunk)
                .map_err(|e| PyRuntimeError::new_err(e))?;
            MdStreamBackend::Batch(BatchDrainState::new(chunks))
        }
        "page_aware" => {
            let chunks = build_page_aware_chunks(&bytes, paragraphs_per_page)
                .map_err(|e| PyRuntimeError::new_err(e))?;
            MdStreamBackend::Batch(BatchDrainState::new(chunks))
        }
        _ => unreachable!(),
    };

    Py::new(
        py,
        MdStreamIterator {
            backend,
            pending: VecDeque::new(),
            done: false,
        },
    )
}

pub fn register(m: &pyo3::Bound<'_, PyModule>) -> pyo3::PyResult<()> {
    m.add_class::<MdStreamIterator>()?;
    m.add_function(wrap_pyfunction!(stream_md_chunks, m)?)?;
    Ok(())
}
