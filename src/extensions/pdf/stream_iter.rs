use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyModule};
use pyo3::wrap_pyfunction;
use std::collections::{HashSet, VecDeque};

use super::common::{split_unit, ChunkUnit, ContentType, MAX_CHUNK_CHARS, MIN_CHUNK_CHARS};
use super::structural::{
    classify_chunk_structural, get_pdfium, prepare_paragraph_records, prepare_structural_chunks,
    ParagraphRecord,
};

#[derive(Debug, Clone)]
struct StructuralChunkRecord {
    text: String,
    page_number: usize,
    heading_hint: bool,
    avg_font_size: f32,
}

#[derive(Debug, Clone)]
struct SectionChunkRecord {
    text: String,
    page_number: usize,
    section_heading: String,
    section_level: u8,
    heading_font_size: f32,
    paragraph_count: usize,
}

#[derive(Debug, Clone)]
struct SemanticChunkRecord {
    text: String,
    page_number: usize,
    paragraph_count: usize,
    merge_reason: &'static str,
}

#[derive(Debug, Clone)]
struct SentenceChunkRecord {
    content: String,
    actual_sentence_count: usize,
    chunk_index: usize,
    source_paragraph_index: usize,
}

#[derive(Debug, Clone)]
struct SlidingWindowChunkRecord {
    content: String,
    page_number: usize,
    window_index: usize,
    start_paragraph_index: usize,
    end_paragraph_index: usize,
    paragraph_count: usize,
}

#[derive(Debug, Clone)]
struct PageAwareChunkRecord {
    content: String,
    page_number: usize,
    page_break_type: &'static str,
    paragraph_count: usize,
}

#[derive(Debug, Clone)]
struct IndexedSentence {
    paragraph_index: usize,
    text: String,
}

#[derive(Debug)]
struct FastStreamState {
    pages: Vec<(usize, String)>,
    page_index: usize,
    current_page_chunks: Vec<String>,
    current_chunk_index: usize,
    current_page_number: usize,
    pending_merged: Option<(String, usize)>,
}

#[derive(Debug)]
struct StructuralStreamState {
    units: Vec<ChunkUnit>,
    unit_page_meta: Vec<(usize, bool, f32)>,
    unit_index: usize,
    current_parts: Vec<String>,
    current_part_index: usize,
    current_page_number: usize,
    current_heading_hint: bool,
    current_avg_font_size: f32,
    pending_merged: Option<StructuralChunkRecord>,
    global_avg_font_size: f32,
}

#[derive(Debug)]
struct SectionStreamState {
    records: Vec<ParagraphRecord>,
    index: usize,
    current_heading: Option<String>,
    current_heading_size: f32,
    current_page_number: usize,
    current_body_lines: Vec<String>,
    saw_heading: bool,
    emitted_final: bool,
    pending: VecDeque<SectionChunkRecord>,
    global_avg_font_size: f32,
}

#[derive(Debug)]
struct SemanticRunningChunk {
    paragraphs: Vec<String>,
    page_number: usize,
    merge_reason: &'static str,
    keywords: HashSet<String>,
}

#[derive(Debug)]
struct SemanticStreamState {
    records: Vec<(String, usize)>,
    index: usize,
    current: Option<SemanticRunningChunk>,
}

#[derive(Debug)]
struct SentenceStreamState {
    records: Vec<ParagraphRecord>,
    paragraph_index: usize,
    sentence_buffer: Vec<IndexedSentence>,
    sentence_buffer_index: usize,
    chunk_index: usize,
    sentences_per_chunk: usize,
}

#[derive(Debug)]
struct SlidingWindowStreamState {
    records: Vec<(String, usize)>,
    start: usize,
    window_index: usize,
    window_size: usize,
    overlap: usize,
    done: bool,
}

#[derive(Debug)]
struct PageAwareStreamState {
    records: Vec<ParagraphRecord>,
    index: usize,
    current_page: usize,
    current_paragraphs: Vec<String>,
    count_on_current_chunk: usize,
    paragraphs_per_page: usize,
    initialized: bool,
    finished: bool,
    pending_record: Option<ParagraphRecord>,
}

enum PdfStreamBackend {
    Fast(FastStreamState),
    Structural(StructuralStreamState),
    Section(SectionStreamState),
    Semantic(SemanticStreamState),
    Sentence(SentenceStreamState),
    SlidingWindow(SlidingWindowStreamState),
    PageAware(PageAwareStreamState),
}

#[pyclass]
pub struct PdfStreamIterator {
    backend: PdfStreamBackend,
}

impl StructuralStreamState {
    fn next_raw_chunk(&mut self) -> Option<StructuralChunkRecord> {
        loop {
            if self.current_part_index < self.current_parts.len() {
                let text = self.current_parts[self.current_part_index].clone();
                self.current_part_index += 1;
                return Some(StructuralChunkRecord {
                    text,
                    page_number: self.current_page_number,
                    heading_hint: self.current_heading_hint,
                    avg_font_size: self.current_avg_font_size,
                });
            }

            if self.unit_index >= self.units.len() {
                return None;
            }

            let unit = &self.units[self.unit_index];
            let (page_number, heading_hint, avg_font_size) = self
                .unit_page_meta
                .get(self.unit_index)
                .copied()
                .unwrap_or((1, false, self.global_avg_font_size));

            self.current_parts = split_unit(unit, MAX_CHUNK_CHARS);
            self.current_part_index = 0;
            self.current_page_number = page_number;
            self.current_heading_hint = heading_hint;
            self.current_avg_font_size = avg_font_size;
            self.unit_index += 1;
        }
    }

    fn next_merged_chunk(&mut self) -> Option<StructuralChunkRecord> {
        let soft_max = MAX_CHUNK_CHARS + MIN_CHUNK_CHARS;

        loop {
            match self.next_raw_chunk() {
                Some(next_chunk) => {
                    if let Some(mut pending) = self.pending_merged.take() {
                        if !next_chunk.heading_hint
                            && !pending.heading_hint
                            && next_chunk.text.len() < MIN_CHUNK_CHARS
                        {
                            let candidate = format!("{}\n{}", pending.text, next_chunk.text)
                                .trim()
                                .to_string();
                            if candidate.len() <= soft_max {
                                pending.text = candidate;
                                pending.avg_font_size =
                                    (pending.avg_font_size + next_chunk.avg_font_size) / 2.0;
                                self.pending_merged = Some(pending);
                                continue;
                            }
                        }

                        self.pending_merged = Some(next_chunk);
                        return Some(pending);
                    }

                    self.pending_merged = Some(next_chunk);
                }
                None => return self.pending_merged.take(),
            }
        }
    }
}

const MAX_SECTION_CHARS: usize = 2000;
const MAX_SEMANTIC_CHUNK_CHARS: usize = 1500;
const SHORT_SEMANTIC_PARAGRAPH_CHARS: usize = 80;

const REFERENCE_STARTS: [&str; 8] = [
    "this", "it", "they", "these", "that", "those", "its", "their",
];

const TRANSITION_STARTS: [&str; 12] = [
    "however",
    "nevertheless",
    "in contrast",
    "on the other hand",
    "meanwhile",
    "conversely",
    "that said",
    "in summary",
    "to conclude",
    "therefore",
    "thus",
    "hence",
];

const STOPWORDS: [&str; 24] = [
    "the", "and", "for", "are", "was", "were", "this", "that", "with", "from", "have", "been",
    "will", "they", "their", "which", "about", "into", "more", "also", "when", "than", "those",
    "these",
];

fn collapse_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn split_sentences_with_bounds(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut start = 0usize;
    let len = chars.len();
    let mut i = 0usize;

    while i < len {
        let ch = chars[i];
        if matches!(ch, '.' | '?' | '!') && should_split_sentence_at(&chars, i, ch) {
            let sentence = chars[start..=i].iter().collect::<String>();
            let sentence = collapse_whitespace(&sentence);
            if !sentence.is_empty() {
                out.push(sentence);
            }

            let mut next_start = i + 1;
            while next_start < len && chars[next_start].is_whitespace() {
                next_start += 1;
            }
            start = next_start;
            i = next_start;
            continue;
        }
        i += 1;
    }

    if start < len {
        let tail = chars[start..].iter().collect::<String>();
        let tail = collapse_whitespace(&tail);
        if !tail.is_empty() {
            out.push(tail);
        }
    }

    out
}

fn should_split_sentence_at(chars: &[char], punct_idx: usize, punct: char) -> bool {
    if punct_idx + 2 >= chars.len() {
        return false;
    }

    if chars[punct_idx + 1] != ' ' || !chars[punct_idx + 2].is_uppercase() {
        return false;
    }

    if punct == '.' {
        let prefix = chars[..=punct_idx].iter().collect::<String>();
        if ends_with_title_abbreviation(&prefix)
            || ends_with_numeric_marker(&prefix)
            || ends_with_initials_abbreviation(&prefix)
        {
            return false;
        }
    }

    true
}

fn ends_with_title_abbreviation(prefix: &str) -> bool {
    let lower = prefix.trim_end().to_ascii_lowercase();
    ["mr.", "mrs.", "dr.", "prof.", "sr.", "jr.", "vs.", "etc."]
        .iter()
        .any(|abbr| lower.ends_with(abbr))
}

fn ends_with_numeric_marker(prefix: &str) -> bool {
    let trimmed = prefix.trim_end();
    let without_dot = trimmed.strip_suffix('.').unwrap_or(trimmed);
    let token = without_dot
        .split_whitespace()
        .last()
        .unwrap_or_default()
        .trim();
    !token.is_empty() && token.chars().all(|c| c.is_ascii_digit())
}

fn ends_with_initials_abbreviation(prefix: &str) -> bool {
    let trimmed = prefix.trim_end();
    let bytes = trimmed.as_bytes();
    if bytes.len() < 4 || *bytes.last().unwrap_or(&b' ') != b'.' {
        return false;
    }

    let mut i = bytes.len();
    let mut groups = 0usize;
    while i >= 2 {
        if bytes[i - 1] != b'.' || !bytes[i - 2].is_ascii_alphabetic() {
            break;
        }
        groups += 1;
        if i < 3 {
            break;
        }
        i -= 2;
        if i == 0 || bytes[i - 1] != b'.' {
            break;
        }
        i -= 1;
    }

    groups >= 2
}

fn split_large_text_for_stream(text: &str, max_chars: usize) -> Vec<String> {
    if text.len() <= max_chars {
        return vec![text.trim().to_string()];
    }

    let sentences = split_sentences_with_bounds(text);
    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();

    for sentence in sentences {
        let candidate = if current.is_empty() {
            sentence.clone()
        } else {
            format!("{current} {sentence}")
        };

        if candidate.len() <= max_chars {
            current = candidate;
        } else {
            if !current.is_empty() {
                chunks.push(current.trim().to_string());
            }
            current = sentence;
            while current.len() > max_chars {
                let split_at = current[..max_chars]
                    .rfind(' ')
                    .unwrap_or(max_chars / 2)
                    .max(max_chars / 2);
                chunks.push(current[..split_at].trim().to_string());
                current = current[split_at..].trim().to_string();
            }
        }
    }

    if !current.trim().is_empty() {
        chunks.push(current.trim().to_string());
    }

    if chunks.is_empty() {
        vec![text.trim().to_string()]
    } else {
        chunks.into_iter().filter(|c| !c.is_empty()).collect()
    }
}

fn infer_section_level(heading_font_size: f32, doc_avg_font_size: f32) -> u8 {
    if heading_font_size >= doc_avg_font_size * 1.9 {
        1
    } else if heading_font_size >= doc_avg_font_size * 1.6 {
        2
    } else {
        3
    }
}

fn split_section_with_heading(heading: &str, body: &str, max_chars: usize) -> Vec<String> {
    let heading = heading.trim();
    let body = body.trim();

    if heading.is_empty() && body.is_empty() {
        return Vec::new();
    }

    if body.is_empty() {
        return vec![heading.to_string()];
    }

    let heading_prefix_len = heading.len().saturating_add(1);
    let max_body_chars = max_chars.saturating_sub(heading_prefix_len).max(200);
    let body_parts = split_large_text_for_stream(body, max_body_chars);

    body_parts
        .into_iter()
        .map(|part| format!("{}\n{}", heading, part.trim()).trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn tokenize_keywords(text: &str) -> HashSet<String> {
    text.to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| w.len() >= 4 && !STOPWORDS.contains(w))
        .map(|s| s.to_string())
        .collect()
}

fn starts_with_reference_pronoun(paragraph: &str) -> bool {
    let lower = paragraph.trim_start().to_ascii_lowercase();
    REFERENCE_STARTS
        .iter()
        .any(|p| lower.starts_with(&format!("{p} ")) || lower == *p)
}

fn starts_with_transition(paragraph: &str) -> bool {
    let lower = paragraph.trim_start().to_ascii_lowercase();
    TRANSITION_STARTS
        .iter()
        .any(|t| lower.starts_with(&format!("{t} ")) || lower == *t)
}

fn is_short_semantic_paragraph(paragraph: &str) -> bool {
    paragraph.len() <= SHORT_SEMANTIC_PARAGRAPH_CHARS
}

fn has_keyword_overlap(a: &HashSet<String>, b: &HashSet<String>) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a.intersection(b).take(2).count() >= 1
}

impl FastStreamState {
    fn next_raw_chunk(&mut self) -> Option<(String, usize)> {
        loop {
            if self.current_chunk_index < self.current_page_chunks.len() {
                let text = self.current_page_chunks[self.current_chunk_index].clone();
                self.current_chunk_index += 1;
                return Some((text, self.current_page_number));
            }

            if self.page_index >= self.pages.len() {
                return None;
            }

            let (page_number, raw) = &self.pages[self.page_index];
            self.page_index += 1;
            self.current_page_number = *page_number;
            self.current_chunk_index = 0;
            self.current_page_chunks.clear();

            let mut page_had_blocks = false;
            for block_lines in super::common::split_raw_blocks(raw) {
                let clean: Vec<String> = block_lines
                    .iter()
                    .map(|l| super::common::normalize_line(l))
                    .filter(|l| !l.is_empty())
                    .collect();

                if clean.is_empty() {
                    continue;
                }

                page_had_blocks = true;
                let para = super::common::build_paragraph(clean.clone()).unwrap_or(
                    super::common::Paragraph {
                        text: clean.join(" "),
                        is_heading: false,
                    },
                );

                let text = para.text.trim().to_string();
                if text.is_empty() {
                    continue;
                }

                if text.len() > MAX_CHUNK_CHARS {
                    self.current_page_chunks
                        .extend(split_large_text_for_stream(&text, MAX_CHUNK_CHARS));
                } else {
                    self.current_page_chunks.push(text);
                }
            }

            if !page_had_blocks {
                self.current_page_chunks.push(raw.clone());
            }
        }
    }

    fn next_merged_chunk(&mut self) -> Option<(String, usize)> {
        let soft_max = MAX_CHUNK_CHARS + MIN_CHUNK_CHARS;

        loop {
            match self.next_raw_chunk() {
                Some((text, page_number)) => {
                    if let Some((prev_text, prev_page)) = self.pending_merged.take() {
                        if prev_page == page_number && text.len() < MIN_CHUNK_CHARS {
                            let candidate = format!("{}\n{}", prev_text, text).trim().to_string();
                            if candidate.len() <= soft_max {
                                self.pending_merged = Some((candidate, page_number));
                                continue;
                            }
                        }

                        self.pending_merged = Some((text, page_number));
                        return Some((prev_text, prev_page));
                    }

                    self.pending_merged = Some((text, page_number));
                }
                None => return self.pending_merged.take(),
            }
        }
    }
}

impl SectionStreamState {
    fn flush_section(
        pending: &mut VecDeque<SectionChunkRecord>,
        heading: &str,
        body_lines: &[String],
        page_number: usize,
        heading_font_size: f32,
        doc_avg_font_size: f32,
    ) {
        let body = body_lines.join("\n").trim().to_string();
        let chunks = split_section_with_heading(heading, &body, MAX_SECTION_CHARS);
        let section_level = infer_section_level(heading_font_size, doc_avg_font_size);
        let paragraph_count = body_lines.len();

        for text in chunks {
            pending.push_back(SectionChunkRecord {
                text,
                page_number,
                section_heading: heading.to_string(),
                section_level,
                heading_font_size,
                paragraph_count,
            });
        }
    }

    fn next_record(&mut self) -> Option<SectionChunkRecord> {
        loop {
            if let Some(record) = self.pending.pop_front() {
                return Some(record);
            }

            if self.index < self.records.len() {
                let record = self.records[self.index].clone();
                self.index += 1;

                if record.paragraph.is_heading {
                    if self.current_heading.is_none() && !self.current_body_lines.is_empty() {
                        Self::flush_section(
                            &mut self.pending,
                            "Preamble",
                            &self.current_body_lines,
                            self.current_page_number,
                            self.global_avg_font_size,
                            self.global_avg_font_size,
                        );
                        self.current_body_lines.clear();
                    }

                    if let Some(existing_heading) = self.current_heading.take() {
                        Self::flush_section(
                            &mut self.pending,
                            &existing_heading,
                            &self.current_body_lines,
                            self.current_page_number,
                            self.current_heading_size,
                            self.global_avg_font_size,
                        );
                        self.current_body_lines.clear();
                    }

                    self.current_heading = Some(record.paragraph.text.trim().to_string());
                    self.current_heading_size = record.avg_font_size;
                    self.current_page_number = record.page_number;
                    self.saw_heading = true;
                    continue;
                }

                if self.current_body_lines.is_empty() {
                    self.current_page_number = record.page_number;
                }
                self.current_body_lines.push(record.paragraph.text);
                continue;
            }

            if self.emitted_final {
                return None;
            }

            self.emitted_final = true;
            if let Some(existing_heading) = self.current_heading.take() {
                Self::flush_section(
                    &mut self.pending,
                    &existing_heading,
                    &self.current_body_lines,
                    self.current_page_number,
                    self.current_heading_size,
                    self.global_avg_font_size,
                );
            } else {
                let fallback_heading = if self.saw_heading {
                    "Preamble"
                } else {
                    "Document"
                };
                Self::flush_section(
                    &mut self.pending,
                    fallback_heading,
                    &self.current_body_lines,
                    self.current_page_number,
                    self.global_avg_font_size,
                    self.global_avg_font_size,
                );
            }
            self.current_body_lines.clear();
        }
    }
}

impl SemanticStreamState {
    fn next_record(&mut self) -> Option<SemanticChunkRecord> {
        loop {
            if self.index >= self.records.len() {
                let running = self.current.take()?;
                return Some(SemanticChunkRecord {
                    text: running.paragraphs.join("\n\n"),
                    page_number: running.page_number,
                    paragraph_count: running.paragraphs.len(),
                    merge_reason: running.merge_reason,
                });
            }

            let (para, page_number) = self.records[self.index].clone();
            self.index += 1;

            if self.current.is_none() {
                self.current = Some(SemanticRunningChunk {
                    paragraphs: vec![para.clone()],
                    page_number,
                    merge_reason: "keyword_overlap",
                    keywords: tokenize_keywords(&para),
                });
                continue;
            }

            let mut current = self.current.take().unwrap_or(SemanticRunningChunk {
                paragraphs: vec![para.clone()],
                page_number,
                merge_reason: "keyword_overlap",
                keywords: tokenize_keywords(&para),
            });

            let para_keywords = tokenize_keywords(&para);
            let mut should_merge = false;
            let mut reason = current.merge_reason;

            if page_number == current.page_number {
                if starts_with_transition(&para) {
                    should_merge = false;
                } else if starts_with_reference_pronoun(&para) {
                    should_merge = true;
                    reason = "reference_continuity";
                } else if is_short_semantic_paragraph(&para) {
                    should_merge = true;
                    reason = "short_paragraph";
                } else if has_keyword_overlap(&current.keywords, &para_keywords) {
                    should_merge = true;
                    reason = "keyword_overlap";
                }
            }

            let current_chars = current.paragraphs.join("\n\n").len();
            let candidate_chars = current_chars + 2 + para.len();

            if should_merge && candidate_chars <= MAX_SEMANTIC_CHUNK_CHARS {
                current.paragraphs.push(para.clone());
                current.merge_reason = reason;
                current.keywords.extend(para_keywords);
                self.current = Some(current);
                continue;
            }

            let emitted = SemanticChunkRecord {
                text: current.paragraphs.join("\n\n"),
                page_number: current.page_number,
                paragraph_count: current.paragraphs.len(),
                merge_reason: current.merge_reason,
            };

            self.current = Some(SemanticRunningChunk {
                paragraphs: vec![para.clone()],
                page_number,
                merge_reason: if starts_with_transition(&para) {
                    "transition_break"
                } else {
                    "keyword_overlap"
                },
                keywords: para_keywords,
            });
            return Some(emitted);
        }
    }
}

impl SentenceStreamState {
    fn next_sentence(&mut self) -> Option<IndexedSentence> {
        loop {
            if self.sentence_buffer_index < self.sentence_buffer.len() {
                let out = self.sentence_buffer[self.sentence_buffer_index].clone();
                self.sentence_buffer_index += 1;
                return Some(out);
            }

            if self.paragraph_index >= self.records.len() {
                return None;
            }

            let paragraph = &self.records[self.paragraph_index];
            self.paragraph_index += 1;

            let sentence_texts = split_sentences_with_bounds(&paragraph.paragraph.text);
            self.sentence_buffer = sentence_texts
                .into_iter()
                .filter(|s| !s.is_empty())
                .map(|text| IndexedSentence {
                    paragraph_index: paragraph.paragraph_index,
                    text,
                })
                .collect();
            self.sentence_buffer_index = 0;
        }
    }

    fn next_record(&mut self) -> Option<SentenceChunkRecord> {
        let mut window: Vec<IndexedSentence> = Vec::with_capacity(self.sentences_per_chunk);
        while window.len() < self.sentences_per_chunk {
            match self.next_sentence() {
                Some(s) => window.push(s),
                None => break,
            }
        }

        if window.is_empty() {
            return None;
        }

        let record = SentenceChunkRecord {
            content: window
                .iter()
                .map(|s| s.text.clone())
                .collect::<Vec<_>>()
                .join(" "),
            actual_sentence_count: window.len(),
            chunk_index: self.chunk_index,
            source_paragraph_index: window[0].paragraph_index,
        };
        self.chunk_index += 1;
        Some(record)
    }
}

impl SlidingWindowStreamState {
    fn next_record(&mut self) -> Option<SlidingWindowChunkRecord> {
        if self.done || self.records.is_empty() {
            return None;
        }

        while self.start < self.records.len() {
            let end = (self.start + self.window_size).min(self.records.len());
            let window = &self.records[self.start..end];
            let content = window
                .iter()
                .map(|(paragraph, _)| paragraph.clone())
                .collect::<Vec<_>>()
                .join("\n\n");

            let window_index = self.window_index;
            let current_start = self.start;
            if end == self.records.len() {
                self.done = true;
            } else {
                self.start += self.window_size - self.overlap;
                self.window_index += 1;
            }

            if !content.is_empty() {
                return Some(SlidingWindowChunkRecord {
                    content,
                    page_number: window[0].1,
                    window_index,
                    start_paragraph_index: current_start,
                    end_paragraph_index: end.saturating_sub(1),
                    paragraph_count: window.len(),
                });
            }

            if self.done {
                return None;
            }
        }

        self.done = true;
        None
    }
}

impl PageAwareStreamState {
    fn next_record(&mut self) -> Option<PageAwareChunkRecord> {
        loop {
            if self.finished {
                return None;
            }

            if !self.initialized {
                if self.records.is_empty() {
                    self.finished = true;
                    return None;
                }
                self.current_page = self.records[0].page_number;
                self.initialized = true;
            }

            let next_record = if let Some(pending) = self.pending_record.take() {
                Some(pending)
            } else if self.index < self.records.len() {
                let r = self.records[self.index].clone();
                self.index += 1;
                Some(r)
            } else {
                None
            };

            match next_record {
                Some(record) => {
                    if record.page_number != self.current_page
                        && !self.current_paragraphs.is_empty()
                    {
                        self.pending_record = Some(record);
                        let out = PageAwareChunkRecord {
                            content: self.current_paragraphs.join("\n\n"),
                            page_number: self.current_page,
                            page_break_type: "explicit",
                            paragraph_count: self.count_on_current_chunk,
                        };
                        self.current_paragraphs.clear();
                        self.count_on_current_chunk = 0;
                        if let Some(next) = &self.pending_record {
                            self.current_page = next.page_number;
                        }
                        return Some(out);
                    }

                    self.current_paragraphs
                        .push(collapse_whitespace(&record.paragraph.text));
                    self.count_on_current_chunk += 1;

                    if self.count_on_current_chunk >= self.paragraphs_per_page {
                        let out = PageAwareChunkRecord {
                            content: self.current_paragraphs.join("\n\n"),
                            page_number: self.current_page,
                            page_break_type: "estimated",
                            paragraph_count: self.count_on_current_chunk,
                        };
                        self.current_paragraphs.clear();
                        self.count_on_current_chunk = 0;
                        return Some(out);
                    }
                }
                None => {
                    if !self.current_paragraphs.is_empty() {
                        let out = PageAwareChunkRecord {
                            content: self.current_paragraphs.join("\n\n"),
                            page_number: self.current_page,
                            page_break_type: "estimated",
                            paragraph_count: self.count_on_current_chunk,
                        };
                        self.current_paragraphs.clear();
                        self.count_on_current_chunk = 0;
                        self.finished = true;
                        return Some(out);
                    }
                    self.finished = true;
                    return None;
                }
            }
        }
    }
}

#[pymethods]
impl PdfStreamIterator {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(&mut self, py: Python<'_>) -> PyResult<Option<PyObject>> {
        match &mut self.backend {
            PdfStreamBackend::Fast(state) => {
                let Some((text, page_number)) = state.next_merged_chunk() else {
                    return Ok(None);
                };

                let metadata_dict = PyDict::new_bound(py);
                metadata_dict.set_item("page_number", page_number)?;

                let dict = PyDict::new_bound(py);
                dict.set_item("content", &text)?;
                dict.set_item(
                    "content_type",
                    super::common::classify_chunk(&text).as_str(),
                )?;
                dict.set_item("metadata", &metadata_dict)?;
                Ok(Some(dict.into_any().unbind()))
            }
            PdfStreamBackend::Structural(state) => {
                let Some(record) = state.next_merged_chunk() else {
                    return Ok(None);
                };

                let content_type = if record.heading_hint {
                    ContentType::HeadingSection
                } else {
                    classify_chunk_structural(
                        &record.text,
                        record.avg_font_size,
                        state.global_avg_font_size,
                    )
                };

                let metadata_dict = PyDict::new_bound(py);
                metadata_dict.set_item("page_number", record.page_number)?;
                metadata_dict.set_item("is_heading", record.heading_hint)?;
                metadata_dict.set_item("avg_font_size", format!("{:.2}", record.avg_font_size))?;

                let dict = PyDict::new_bound(py);
                dict.set_item("content", &record.text)?;
                dict.set_item("content_type", content_type.as_str())?;
                dict.set_item("metadata", &metadata_dict)?;
                Ok(Some(dict.into_any().unbind()))
            }
            PdfStreamBackend::Section(state) => {
                let Some(record) = state.next_record() else {
                    return Ok(None);
                };

                let heading_path = PyList::new_bound(py, &[record.section_heading.as_str()]);
                let metadata_dict = PyDict::new_bound(py);
                metadata_dict.set_item("page_number", record.page_number)?;
                metadata_dict.set_item("section_heading", &record.section_heading)?;
                metadata_dict.set_item("section_level", record.section_level)?;
                metadata_dict.set_item("heading_path", &heading_path)?;
                metadata_dict.set_item("paragraph_count", record.paragraph_count)?;
                metadata_dict.set_item(
                    "heading_font_size",
                    format!("{:.2}", record.heading_font_size),
                )?;

                let dict = PyDict::new_bound(py);
                dict.set_item("content", &record.text)?;
                dict.set_item("content_type", "section")?;
                dict.set_item("metadata", &metadata_dict)?;
                Ok(Some(dict.into_any().unbind()))
            }
            PdfStreamBackend::Semantic(state) => {
                let Some(record) = state.next_record() else {
                    return Ok(None);
                };

                let metadata_dict = PyDict::new_bound(py);
                metadata_dict.set_item("page_number", record.page_number)?;
                metadata_dict.set_item("paragraph_count", record.paragraph_count)?;
                metadata_dict.set_item("merge_reason", record.merge_reason)?;

                let dict = PyDict::new_bound(py);
                dict.set_item("content", &record.text)?;
                dict.set_item("content_type", "semantic")?;
                dict.set_item("metadata", &metadata_dict)?;
                Ok(Some(dict.into_any().unbind()))
            }
            PdfStreamBackend::Sentence(state) => {
                let Some(record) = state.next_record() else {
                    return Ok(None);
                };

                let metadata_dict = PyDict::new_bound(py);
                metadata_dict.set_item("sentences_per_chunk", state.sentences_per_chunk)?;
                metadata_dict.set_item("actual_sentence_count", record.actual_sentence_count)?;
                metadata_dict.set_item("chunk_index", record.chunk_index)?;
                metadata_dict.set_item("source_paragraph_index", record.source_paragraph_index)?;

                let dict = PyDict::new_bound(py);
                dict.set_item("content", &record.content)?;
                dict.set_item("content_type", "sentence")?;
                dict.set_item("metadata", &metadata_dict)?;
                Ok(Some(dict.into_any().unbind()))
            }
            PdfStreamBackend::SlidingWindow(state) => {
                let Some(record) = state.next_record() else {
                    return Ok(None);
                };

                let para_range = PyList::new_bound(
                    py,
                    &[
                        record.start_paragraph_index as i64,
                        record.end_paragraph_index as i64,
                    ],
                );
                let metadata_dict = PyDict::new_bound(py);
                metadata_dict.set_item("page_number", record.page_number)?;
                metadata_dict.set_item("window_size", state.window_size)?;
                metadata_dict.set_item("overlap", state.overlap)?;
                metadata_dict.set_item("window_index", record.window_index)?;
                metadata_dict.set_item("paragraph_count", record.paragraph_count)?;
                metadata_dict.set_item("paragraph_range", &para_range)?;

                let dict = PyDict::new_bound(py);
                dict.set_item("content", &record.content)?;
                dict.set_item("content_type", "sliding_window")?;
                dict.set_item("metadata", &metadata_dict)?;
                Ok(Some(dict.into_any().unbind()))
            }
            PdfStreamBackend::PageAware(state) => {
                let Some(record) = state.next_record() else {
                    return Ok(None);
                };

                let metadata_dict = PyDict::new_bound(py);
                metadata_dict.set_item("page_number", record.page_number)?;
                metadata_dict.set_item("page_break_type", record.page_break_type)?;
                metadata_dict.set_item("paragraph_count", record.paragraph_count)?;

                let doc_meta = PyDict::new_bound(py);
                doc_meta.set_item("source_type", "pdf")?;
                metadata_dict.set_item("document_metadata", &doc_meta)?;

                let dict = PyDict::new_bound(py);
                dict.set_item("content", &record.content)?;
                dict.set_item("content_type", "page_aware")?;
                dict.set_item("metadata", &metadata_dict)?;
                Ok(Some(dict.into_any().unbind()))
            }
        }
    }
}

#[pyfunction]
pub fn stream_pdf_chunks(
    py: Python<'_>,
    file_path: &str,
    mode: &str,
    window_size: usize,
    overlap: usize,
    sentences_per_chunk: usize,
    paragraphs_per_page: usize,
) -> PyResult<Py<PdfStreamIterator>> {
    if !file_path.to_ascii_lowercase().ends_with(".pdf") {
        return Err(PyValueError::new_err(format!(
            "Expected .pdf file path, got: {file_path}"
        )));
    }

    match mode {
        "default" => {
            let pdfium = get_pdfium().map_err(PyRuntimeError::new_err)?;
            let doc = pdfium
                .load_pdf_from_file(file_path, None)
                .map_err(|e| PyIOError::new_err(format!("Failed to load PDF: {}", e)))?;

            let mut pages = Vec::new();
            for (page_idx, page) in doc.pages().iter().enumerate() {
                let page_number = page_idx + 1;
                let page_text = match page.text() {
                    Ok(text) => text,
                    Err(_) => continue,
                };
                let raw = super::common::normalize_line(&page_text.all());
                if !raw.is_empty() {
                    pages.push((page_number, raw));
                }
            }

            if pages.is_empty() {
                return Err(PyRuntimeError::new_err(
                    "PDF appears to contain no extractable text",
                ));
            }

            Py::new(
                py,
                PdfStreamIterator {
                    backend: PdfStreamBackend::Fast(FastStreamState {
                        pages,
                        page_index: 0,
                        current_page_chunks: Vec::new(),
                        current_chunk_index: 0,
                        current_page_number: 1,
                        pending_merged: None,
                    }),
                },
            )
        }
        "structural" => {
            let pdfium = get_pdfium().map_err(PyRuntimeError::new_err)?;
            let doc = pdfium
                .load_pdf_from_file(file_path, None)
                .map_err(|e| PyIOError::new_err(format!("Failed to load PDF: {}", e)))?;

            let prepared =
                prepare_structural_chunks(&doc).map_err(PyRuntimeError::new_err)?;

            let state = StructuralStreamState {
                units: prepared.units,
                unit_page_meta: prepared.unit_page_meta,
                unit_index: 0,
                current_parts: Vec::new(),
                current_part_index: 0,
                current_page_number: 1,
                current_heading_hint: false,
                current_avg_font_size: prepared.global_avg_font_size,
                pending_merged: None,
                global_avg_font_size: prepared.global_avg_font_size,
            };

            Py::new(
                py,
                PdfStreamIterator {
                    backend: PdfStreamBackend::Structural(state),
                },
            )
        }
        "section" => {
            let pdfium = get_pdfium().map_err(PyRuntimeError::new_err)?;
            let doc = pdfium
                .load_pdf_from_file(file_path, None)
                .map_err(|e| PyIOError::new_err(format!("Failed to load PDF: {}", e)))?;

            let prepared = prepare_paragraph_records(&doc).map_err(PyRuntimeError::new_err)?;
            let records = prepared.paragraph_records;
            let current_page_number = records.first().map(|r| r.page_number).unwrap_or(1);

            Py::new(
                py,
                PdfStreamIterator {
                    backend: PdfStreamBackend::Section(SectionStreamState {
                        records,
                        index: 0,
                        current_heading: None,
                        current_heading_size: prepared.global_avg_font_size,
                        current_page_number,
                        current_body_lines: Vec::new(),
                        saw_heading: false,
                        emitted_final: false,
                        pending: VecDeque::new(),
                        global_avg_font_size: prepared.global_avg_font_size,
                    }),
                },
            )
        }
        "semantic" => {
            let pdfium = get_pdfium().map_err(PyRuntimeError::new_err)?;
            let doc = pdfium
                .load_pdf_from_file(file_path, None)
                .map_err(|e| PyIOError::new_err(format!("Failed to load PDF: {}", e)))?;

            let prepared = prepare_paragraph_records(&doc).map_err(PyRuntimeError::new_err)?;
            let records: Vec<(String, usize)> = prepared
                .paragraph_records
                .into_iter()
                .map(|r| (collapse_whitespace(&r.paragraph.text), r.page_number))
                .filter(|(p, _)| !p.is_empty())
                .collect();

            if records.is_empty() {
                return Err(PyRuntimeError::new_err(
                    "PDF appears to contain no extractable text",
                ));
            }

            Py::new(
                py,
                PdfStreamIterator {
                    backend: PdfStreamBackend::Semantic(SemanticStreamState {
                        records,
                        index: 0,
                        current: None,
                    }),
                },
            )
        }
        "sentence" => {
            if sentences_per_chunk == 0 {
                return Err(PyValueError::new_err(
                    "sentences_per_chunk must be greater than 0",
                ));
            }

            let pdfium = get_pdfium().map_err(PyRuntimeError::new_err)?;
            let doc = pdfium
                .load_pdf_from_file(file_path, None)
                .map_err(|e| PyIOError::new_err(format!("Failed to load PDF: {}", e)))?;

            let prepared = prepare_paragraph_records(&doc).map_err(PyRuntimeError::new_err)?;

            Py::new(
                py,
                PdfStreamIterator {
                    backend: PdfStreamBackend::Sentence(SentenceStreamState {
                        records: prepared.paragraph_records,
                        paragraph_index: 0,
                        sentence_buffer: Vec::new(),
                        sentence_buffer_index: 0,
                        chunk_index: 0,
                        sentences_per_chunk,
                    }),
                },
            )
        }
        "sliding_window" => {
            if window_size == 0 {
                return Err(PyValueError::new_err("window_size must be greater than 0"));
            }
            if overlap >= window_size {
                return Err(PyValueError::new_err(
                    "overlap must be less than window_size",
                ));
            }

            let pdfium = get_pdfium().map_err(PyRuntimeError::new_err)?;
            let doc = pdfium
                .load_pdf_from_file(file_path, None)
                .map_err(|e| PyIOError::new_err(format!("Failed to load PDF: {}", e)))?;

            let prepared = prepare_paragraph_records(&doc).map_err(PyRuntimeError::new_err)?;
            let records: Vec<(String, usize)> = prepared
                .paragraph_records
                .into_iter()
                .map(|r| (collapse_whitespace(&r.paragraph.text), r.page_number))
                .filter(|(p, _)| !p.is_empty())
                .collect();

            if records.is_empty() {
                return Err(PyRuntimeError::new_err(
                    "PDF appears to contain no extractable text",
                ));
            }

            Py::new(
                py,
                PdfStreamIterator {
                    backend: PdfStreamBackend::SlidingWindow(SlidingWindowStreamState {
                        records,
                        start: 0,
                        window_index: 0,
                        window_size,
                        overlap,
                        done: false,
                    }),
                },
            )
        }
        "page_aware" => {
            if paragraphs_per_page == 0 {
                return Err(PyValueError::new_err(
                    "paragraphs_per_page must be greater than 0",
                ));
            }

            let pdfium = get_pdfium().map_err(PyRuntimeError::new_err)?;
            let doc = pdfium
                .load_pdf_from_file(file_path, None)
                .map_err(|e| PyIOError::new_err(format!("Failed to load PDF: {}", e)))?;

            let prepared = prepare_paragraph_records(&doc).map_err(PyRuntimeError::new_err)?;

            Py::new(
                py,
                PdfStreamIterator {
                    backend: PdfStreamBackend::PageAware(PageAwareStreamState {
                        records: prepared.paragraph_records,
                        index: 0,
                        current_page: 1,
                        current_paragraphs: Vec::new(),
                        count_on_current_chunk: 0,
                        paragraphs_per_page,
                        initialized: false,
                        finished: false,
                        pending_record: None,
                    }),
                },
            )
        }
        _ => Err(PyValueError::new_err(
            "mode must be 'default', 'structural', 'section', 'semantic', 'sliding_window', 'sentence', or 'page_aware' for PDF",
        )),
    }
}

pub fn register(m: &pyo3::Bound<'_, PyModule>) -> pyo3::PyResult<()> {
    m.add_class::<PdfStreamIterator>()?;
    m.add_function(wrap_pyfunction!(stream_pdf_chunks, m)?)?;
    Ok(())
}
