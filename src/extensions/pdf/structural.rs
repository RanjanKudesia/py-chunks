use pdfium_render::prelude::*;
use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use std::collections::HashSet;
use std::sync::OnceLock;
use std::time::Instant;

use super::common::{
    absorb_heading_only_units, build_paragraph, build_units, classify_chunk, is_noise_line,
    is_standalone_number, is_toc_heading, looks_like_toc_entry, normalize_line, split_large_text,
    split_raw_blocks, split_unit, ChunkUnit, ContentType, Paragraph, MAX_CHUNK_CHARS,
    MIN_CHUNK_CHARS,
};

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

#[derive(Debug, Clone)]
struct PdfSpan {
    text: String,
    font_size: f32,
    page_number: usize,
    line_y: f32,
}

#[derive(Debug, Clone)]
pub(super) struct ParagraphRecord {
    pub paragraph: Paragraph,
    pub page_number: usize,
    pub paragraph_index: usize,
    pub avg_font_size: f32,
}

#[derive(Debug, Clone)]
struct ChunkRecord {
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
struct SemanticChunk {
    paragraphs: Vec<String>,
    page_number: usize,
    merge_reason: &'static str,
}

#[derive(Debug, Clone)]
struct SlidingWindowChunk {
    content: String,
    page_number: usize,
    window_index: usize,
    start_paragraph_index: usize,
    end_paragraph_index: usize,
    paragraph_count: usize,
}

#[derive(Debug, Clone)]
struct PageAwareChunk {
    content: String,
    page_number: usize,
    page_break_type: &'static str,
    paragraph_count: usize,
}

#[derive(Debug, Clone)]
struct ChunkRecordInput {
    content: String,
    metadata: serde_json::Value,
}

#[derive(Debug, Clone)]
struct IndexedSentence {
    paragraph_index: usize,
    text: String,
}

#[derive(Debug)]
pub(super) struct StructuralPrepared {
    pub units: Vec<ChunkUnit>,
    pub unit_page_meta: Vec<(usize, bool, f32)>,
    pub global_avg_font_size: f32,
}

#[derive(Debug)]
pub(super) struct ParagraphsPrepared {
    pub paragraph_records: Vec<ParagraphRecord>,
    pub global_avg_font_size: f32,
}

pub(super) fn classify_chunk_structural(
    text: &str,
    avg_font_size: f32,
    doc_avg_size: f32,
) -> ContentType {
    if avg_font_size >= doc_avg_size * 1.4 {
        return ContentType::HeadingSection;
    }

    classify_chunk(text)
}

fn extract_spans_from_doc(doc: &PdfDocument<'_>) -> Vec<PdfSpan> {
    let mut spans: Vec<PdfSpan> = Vec::new();

    for (page_idx, page) in doc.pages().iter().enumerate() {
        let page_number = page_idx + 1;
        let page_text = match page.text() {
            Ok(text) => text,
            Err(_) => continue,
        };

        for segment in page_text.segments().iter() {
            let chars = match segment.chars() {
                Ok(chars) => chars,
                Err(_) => continue,
            };

            if chars.is_empty() {
                continue;
            }

            let mut text = String::new();
            let mut font_size_sum = 0.0f32;
            let mut font_size_count = 0usize;
            let mut y_sum = 0.0f32;
            let mut y_count = 0usize;

            for ch in chars.iter() {
                if let Some(s) = ch.unicode_string() {
                    text.push_str(&s);
                }

                let fs = ch.scaled_font_size().value;
                if fs.is_finite() && fs > 0.0 {
                    font_size_sum += fs;
                    font_size_count += 1;
                }

                if let Ok(y) = ch.origin_y() {
                    let y_val = y.value;
                    if y_val.is_finite() {
                        y_sum += y_val;
                        y_count += 1;
                    }
                }
            }

            let text = normalize_line(&text);
            if text.is_empty() || is_noise_line(&text) {
                continue;
            }

            let avg_font_size = if font_size_count > 0 {
                font_size_sum / font_size_count as f32
            } else {
                0.0
            };

            let line_y = if y_count > 0 {
                y_sum / y_count as f32
            } else {
                segment.bounds().top().value
            };

            spans.push(PdfSpan {
                text,
                font_size: avg_font_size,
                page_number,
                line_y,
            });
        }
    }

    spans
}

fn group_spans_into_paragraphs(spans: Vec<PdfSpan>) -> Vec<(String, bool, usize, f32)> {
    if spans.is_empty() {
        return Vec::new();
    }

    let sizes: Vec<f32> = spans.iter().map(|s| s.font_size).collect();
    let avg_doc_size = sizes.iter().sum::<f32>() / sizes.len() as f32;
    let mut sorted_sizes = sizes.clone();
    sorted_sizes.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p90_idx = (((sorted_sizes.len() as f32) * 0.90) as usize).min(sorted_sizes.len() - 1);
    let p90_size = sorted_sizes[p90_idx];

    let mut paragraphs: Vec<(String, bool, usize, f32)> = Vec::new();
    let mut current_lines: Vec<String> = Vec::new();
    let mut current_page = spans[0].page_number;
    let mut current_size = spans[0].font_size;
    let mut prev_y = spans[0].line_y;
    let mut current_is_heading = false;

    for span in &spans {
        let is_heading = span.font_size >= p90_size || span.font_size >= avg_doc_size * 1.5;

        let y_gap = (prev_y - span.line_y).abs();
        let is_new_para = span.page_number != current_page
            || y_gap > span.font_size * 1.5
            || is_heading != current_is_heading;

        if is_new_para && !current_lines.is_empty() {
            let text = current_lines.join(" ").trim().to_string();
            if !text.is_empty() && text.len() >= 10 {
                paragraphs.push((text, current_is_heading, current_page, current_size));
            }
            current_lines.clear();
        }

        current_lines.push(span.text.clone());
        current_page = span.page_number;
        current_size = span.font_size;
        current_is_heading = is_heading;
        prev_y = span.line_y;
    }

    if !current_lines.is_empty() {
        let text = current_lines.join(" ").trim().to_string();
        if !text.is_empty() && text.len() >= 10 {
            paragraphs.push((text, current_is_heading, current_page, current_size));
        }
    }

    paragraphs
}

static PDFIUM_LIBRARY_PATH: OnceLock<Option<String>> = OnceLock::new();

fn resolve_pdfium_library_path() -> Option<String> {
    PDFIUM_LIBRARY_PATH
        .get_or_init(|| {
            let mut candidates = vec![
                std::env::var("VIRTUAL_ENV")
                    .map(|v| {
                        format!(
                            "{}/lib/python3.13/site-packages/pypdfium2_raw/libpdfium.dylib",
                            v
                        )
                    })
                    .unwrap_or_default(),
                "/Library/Frameworks/Python.framework/Versions/3.13/lib/python3.13/site-packages/pypdfium2_raw/libpdfium.dylib".to_string(),
                "/Library/Frameworks/Python.framework/Versions/3.12/lib/python3.12/site-packages/pypdfium2_raw/libpdfium.dylib".to_string(),
                "/Library/Frameworks/Python.framework/Versions/3.11/lib/python3.11/site-packages/pypdfium2_raw/libpdfium.dylib".to_string(),
                "/usr/local/lib/libpdfium.dylib".to_string(),
                "/usr/lib/libpdfium.dylib".to_string(),
            ];

            if std::path::Path::new("/opt/homebrew/opt/pdfium/lib/libpdfium.dylib").exists() {
                candidates.push("/opt/homebrew/opt/pdfium/lib/libpdfium.dylib".to_string());
            }

            candidates.into_iter().find(|path| {
                !path.is_empty() && std::path::Path::new(path).exists()
            })
        })
        .clone()
}

pub(super) fn get_pdfium() -> Result<Pdfium, String> {
    if let Some(path) = resolve_pdfium_library_path() {
        if let Some(parent) = std::path::Path::new(&path).parent() {
            if let Some(parent_str) = parent.to_str() {
                let mut dyld_path = parent_str.to_string();
                if let Ok(existing) = std::env::var("DYLD_LIBRARY_PATH") {
                    dyld_path = format!("{}:{}", dyld_path, existing);
                }
                std::env::set_var("DYLD_LIBRARY_PATH", &dyld_path);
            }
        }

        if let Ok(bindings) = Pdfium::bind_to_library(&path) {
            return Ok(Pdfium::new(bindings));
        }
    }

    Pdfium::bind_to_system_library()
        .map(|b| Pdfium::new(b))
        .map_err(|e| {
            format!(
                "PDFium not found. Install with: pip install pypdfium2. Error: {}",
                e
            )
        })
}

fn collect_paragraph_records(
    grouped_paragraphs: Vec<(String, bool, usize, f32)>,
) -> Vec<ParagraphRecord> {
    let mut paragraph_records: Vec<ParagraphRecord> = Vec::new();
    let mut in_toc = false;
    let mut paragraph_index = 0usize;

    for (text, structural_heading, page_number, avg_font_size) in grouped_paragraphs {
        for block_lines in split_raw_blocks(&text) {
            let clean: Vec<String> = block_lines
                .iter()
                .map(|l| normalize_line(l))
                .filter(|l| !l.is_empty() && !is_noise_line(l))
                .collect();

            if clean.is_empty() {
                continue;
            }

            if is_toc_heading(&clean[0]) {
                in_toc = true;
                continue;
            }

            if in_toc {
                let real_lines: Vec<String> = clean
                    .into_iter()
                    .filter(|l| !looks_like_toc_entry(l) && !is_standalone_number(l))
                    .collect();
                if real_lines.is_empty() {
                    continue;
                }
                in_toc = false;

                let para = if structural_heading {
                    Paragraph {
                        text: real_lines.join("\n"),
                        is_heading: true,
                    }
                } else {
                    match build_paragraph(real_lines) {
                        Some(p) => p,
                        None => continue,
                    }
                };

                paragraph_records.push(ParagraphRecord {
                    paragraph: para,
                    page_number,
                    paragraph_index,
                    avg_font_size,
                });
                paragraph_index += 1;
                continue;
            }

            let para = if structural_heading {
                Paragraph {
                    text: clean.join("\n"),
                    is_heading: true,
                }
            } else {
                match build_paragraph(clean) {
                    Some(p) => p,
                    None => continue,
                }
            };

            paragraph_records.push(ParagraphRecord {
                paragraph: para,
                page_number,
                paragraph_index,
                avg_font_size,
            });
            paragraph_index += 1;
        }
    }

    paragraph_records
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
    let body_parts = split_large_text(body, max_body_chars);

    body_parts
        .into_iter()
        .map(|part| format!("{}\n{}", heading, part.trim()).trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn flush_section(
    output: &mut Vec<SectionChunkRecord>,
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
        output.push(SectionChunkRecord {
            text,
            page_number,
            section_heading: heading.to_string(),
            section_level,
            heading_font_size,
            paragraph_count,
        });
    }
}

fn collapse_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
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

fn tokenize_keywords(text: &str) -> HashSet<String> {
    text.to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| w.len() >= 4 && !STOPWORDS.contains(w))
        .map(|s| s.to_string())
        .collect()
}

fn has_keyword_overlap(a: &HashSet<String>, b: &HashSet<String>) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a.intersection(b).take(2).count() >= 1
}

fn build_pdf_semantic_chunks(paragraph_records: Vec<ParagraphRecord>) -> Vec<SemanticChunk> {
    let records: Vec<(String, usize)> = paragraph_records
        .into_iter()
        .map(|r| (collapse_whitespace(&r.paragraph.text), r.page_number))
        .filter(|(p, _)| !p.is_empty())
        .collect();

    if records.is_empty() {
        return Vec::new();
    }

    let mut chunks: Vec<SemanticChunk> = Vec::new();
    let (first_text, first_page) = &records[0];
    let mut current = SemanticChunk {
        paragraphs: vec![first_text.clone()],
        page_number: *first_page,
        merge_reason: "keyword_overlap",
    };
    let mut current_keywords = tokenize_keywords(first_text);

    for (para, page_number) in records.iter().skip(1) {
        let para_keywords = tokenize_keywords(para);
        let mut should_merge = false;
        let mut reason = current.merge_reason;

        if *page_number == current.page_number {
            if starts_with_transition(para) {
                should_merge = false;
            } else if starts_with_reference_pronoun(para) {
                should_merge = true;
                reason = "reference_continuity";
            } else if is_short_semantic_paragraph(para) {
                should_merge = true;
                reason = "short_paragraph";
            } else if has_keyword_overlap(&current_keywords, &para_keywords) {
                should_merge = true;
                reason = "keyword_overlap";
            }
        }

        let current_chars = current.paragraphs.join("\n\n").len();
        let candidate_chars = current_chars + 2 + para.len();

        if should_merge && candidate_chars <= MAX_SEMANTIC_CHUNK_CHARS {
            current.paragraphs.push(para.clone());
            current.merge_reason = reason;
            current_keywords.extend(para_keywords);
        } else {
            chunks.push(current);
            current = SemanticChunk {
                paragraphs: vec![para.clone()],
                page_number: *page_number,
                merge_reason: if starts_with_transition(para) {
                    "transition_break"
                } else {
                    "keyword_overlap"
                },
            };
            current_keywords = para_keywords;
        }
    }

    chunks.push(current);
    chunks
}
fn split_paragraph_sentences(paragraph: &ParagraphRecord) -> Vec<IndexedSentence> {
    let chars: Vec<char> = paragraph.paragraph.text.chars().collect();
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
                out.push(IndexedSentence {
                    paragraph_index: paragraph.paragraph_index,
                    text: sentence,
                });
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
            out.push(IndexedSentence {
                paragraph_index: paragraph.paragraph_index,
                text: tail,
            });
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
fn build_pdf_sentence_chunks(
    paragraph_records: Vec<ParagraphRecord>,
    sentences_per_chunk: usize,
) -> Vec<ChunkRecordInput> {
    let sentences: Vec<IndexedSentence> = paragraph_records
        .iter()
        .flat_map(split_paragraph_sentences)
        .collect();

    if sentences.is_empty() {
        return Vec::new();
    }

    let mut chunks = Vec::new();
    let mut start = 0usize;
    let mut chunk_index = 0usize;

    while start < sentences.len() {
        let end = (start + sentences_per_chunk).min(sentences.len());
        let window = &sentences[start..end];
        let content = window
            .iter()
            .map(|sentence| sentence.text.clone())
            .collect::<Vec<_>>()
            .join(" ");

        chunks.push(ChunkRecordInput {
            content,
            metadata: serde_json::json!({
                "sentences_per_chunk": sentences_per_chunk,
                "actual_sentence_count": window.len(),
                "chunk_index": chunk_index,
                "source_paragraph_index": window[0].paragraph_index,
                "document_metadata": {
                    "source_type": "pdf"
                }
            }),
        });

        start = end;
        chunk_index += 1;
    }

    chunks
}
fn build_pdf_sliding_window_chunks(
    paragraph_records: Vec<ParagraphRecord>,
    window_size: usize,
    overlap: usize,
) -> Vec<SlidingWindowChunk> {
    let records: Vec<(String, usize)> = paragraph_records
        .into_iter()
        .map(|r| (collapse_whitespace(&r.paragraph.text), r.page_number))
        .filter(|(p, _)| !p.is_empty())
        .collect();

    if records.is_empty() {
        return Vec::new();
    }

    let step = window_size - overlap;
    let mut chunks: Vec<SlidingWindowChunk> = Vec::new();
    let mut start = 0usize;
    let mut window_index = 0usize;

    while start < records.len() {
        let end = (start + window_size).min(records.len());
        let window = &records[start..end];
        let content = window
            .iter()
            .map(|(paragraph, _)| paragraph.clone())
            .collect::<Vec<_>>()
            .join("\n\n");

        if !content.is_empty() {
            chunks.push(SlidingWindowChunk {
                content,
                page_number: window[0].1,
                window_index,
                start_paragraph_index: start,
                end_paragraph_index: end.saturating_sub(1),
                paragraph_count: window.len(),
            });
        }

        if end == records.len() {
            break;
        }

        start += step;
        window_index += 1;
    }

    chunks
}

fn build_pdf_page_aware_chunks(
    paragraph_records: Vec<ParagraphRecord>,
    paragraphs_per_page: usize,
) -> Vec<PageAwareChunk> {
    if paragraph_records.is_empty() {
        return Vec::new();
    }

    let mut chunks = Vec::new();
    let mut current_page = paragraph_records[0].page_number;
    let mut current_paragraphs: Vec<String> = Vec::new();
    let mut count_on_current_chunk = 0usize;

    for record in paragraph_records {
        // Real page transitions are explicit page boundaries for PDF.
        if record.page_number != current_page && !current_paragraphs.is_empty() {
            chunks.push(PageAwareChunk {
                content: current_paragraphs.join("\n\n"),
                page_number: current_page,
                page_break_type: "explicit",
                paragraph_count: count_on_current_chunk,
            });
            current_paragraphs.clear();
            count_on_current_chunk = 0;
            current_page = record.page_number;
        }

        current_paragraphs.push(collapse_whitespace(&record.paragraph.text));
        count_on_current_chunk += 1;

        // Fallback split for very dense pages.
        if count_on_current_chunk >= paragraphs_per_page {
            chunks.push(PageAwareChunk {
                content: current_paragraphs.join("\n\n"),
                page_number: current_page,
                page_break_type: "estimated",
                paragraph_count: count_on_current_chunk,
            });
            current_paragraphs.clear();
            count_on_current_chunk = 0;
        }
    }

    if !current_paragraphs.is_empty() {
        chunks.push(PageAwareChunk {
            content: current_paragraphs.join("\n\n"),
            page_number: current_page,
            page_break_type: "estimated",
            paragraph_count: count_on_current_chunk,
        });
    }

    chunks
}

#[pyfunction]
pub fn chunk_pdf_fast(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    if !file_path.to_ascii_lowercase().ends_with(".pdf") {
        return Err(PyValueError::new_err(format!(
            "Expected .pdf file path, got: {file_path}"
        )));
    }

    let start = Instant::now();

    let pdfium = get_pdfium().map_err(|e| PyRuntimeError::new_err(e))?;
    let doc = pdfium
        .load_pdf_from_file(file_path, None)
        .map_err(|e| PyIOError::new_err(format!("Failed to load PDF: {}", e)))?;

    let mut raw_chunk_records: Vec<(String, usize)> = Vec::new();

    for (page_idx, page) in doc.pages().iter().enumerate() {
        let page_number = page_idx + 1;
        let page_text = match page.text() {
            Ok(text) => text,
            Err(_) => continue,
        };

        let raw = normalize_line(&page_text.all());
        if raw.is_empty() {
            continue;
        }

        let mut page_had_blocks = false;
        for block_lines in split_raw_blocks(&raw) {
            let clean: Vec<String> = block_lines
                .iter()
                .map(|l| normalize_line(l))
                .filter(|l| !l.is_empty())
                .collect();

            if clean.is_empty() {
                continue;
            }

            page_had_blocks = true;

            let para = build_paragraph(clean.clone()).unwrap_or(Paragraph {
                text: clean.join(" "),
                is_heading: false,
            });

            let text = para.text.trim().to_string();
            if text.is_empty() {
                continue;
            }

            if text.len() > MAX_CHUNK_CHARS {
                for part in split_large_text(&text, MAX_CHUNK_CHARS) {
                    if !part.is_empty() {
                        raw_chunk_records.push((part, page_number));
                    }
                }
            } else {
                raw_chunk_records.push((text, page_number));
            }
        }

        if !page_had_blocks {
            raw_chunk_records.push((raw, page_number));
        }
    }

    if raw_chunk_records.is_empty() {
        return Err(PyRuntimeError::new_err(
            "PDF appears to contain no extractable text",
        ));
    }

    let soft_max = MAX_CHUNK_CHARS + MIN_CHUNK_CHARS;
    let mut merged_chunk_records: Vec<(String, usize)> = Vec::new();
    for (text, page_number) in raw_chunk_records {
        if let Some((prev_text, prev_page)) = merged_chunk_records.last_mut() {
            if *prev_page == page_number && text.len() < MIN_CHUNK_CHARS {
                let candidate = format!("{}\n{}", prev_text, text).trim().to_string();
                if candidate.len() <= soft_max {
                    *prev_text = candidate;
                    continue;
                }
            }
        }
        merged_chunk_records.push((text, page_number));
    }

    if merged_chunk_records.is_empty() {
        return Err(PyRuntimeError::new_err("No chunks generated from PDF"));
    }

    let rust_ms = start.elapsed().as_secs_f64() * 1000.0;

    let chunk_list: Vec<PyObject> = merged_chunk_records
        .iter()
        .map(|(text, page_number)| {
            let content_type = classify_chunk(text);
            let metadata_dict = PyDict::new_bound(py);
            metadata_dict.set_item("page_number", page_number)?;

            let dict = PyDict::new_bound(py);
            dict.set_item("content", text)?;
            dict.set_item("content_type", content_type.as_str())?;
            dict.set_item("metadata", &metadata_dict)?;
            Ok(dict.into_any().unbind())
        })
        .collect::<PyResult<_>>()?;

    let result = PyDict::new_bound(py);
    result.set_item("chunks", chunk_list)?;
    result.set_item("rust_ms", (rust_ms * 1000.0).round() / 1000.0)?;
    Ok(result.into_any().unbind())
}

#[pyfunction]
pub fn chunk_pdf(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    if !file_path.to_ascii_lowercase().ends_with(".pdf") {
        return Err(PyValueError::new_err(format!(
            "Expected .pdf file path, got: {file_path}"
        )));
    }

    let start = Instant::now();

    let pdfium = get_pdfium().map_err(|e| PyRuntimeError::new_err(e))?;

    let doc = pdfium
        .load_pdf_from_file(file_path, None)
        .map_err(|e| PyIOError::new_err(format!("Failed to load PDF: {}", e)))?;

    let prepared = prepare_structural_chunks(&doc).map_err(|e| PyRuntimeError::new_err(e))?;
    let units = prepared.units;
    let unit_page_meta = prepared.unit_page_meta;
    let global_avg_font_size = prepared.global_avg_font_size;

    let mut raw_chunk_records: Vec<ChunkRecord> = Vec::new();
    for (idx, unit) in units.iter().enumerate() {
        let (page_number, heading_hint, avg_font_size) = unit_page_meta
            .get(idx)
            .copied()
            .unwrap_or((1, false, global_avg_font_size));

        for chunk in split_unit(unit, MAX_CHUNK_CHARS) {
            raw_chunk_records.push(ChunkRecord {
                text: chunk,
                page_number,
                heading_hint,
                avg_font_size,
            });
        }
    }

    // Merge short chunks but preserve heading boundaries
    let soft_max = MAX_CHUNK_CHARS + MIN_CHUNK_CHARS;
    let mut merged_chunk_records: Vec<ChunkRecord> = Vec::new();

    for chunk in raw_chunk_records {
        if let Some(prev) = merged_chunk_records.last_mut() {
            // Never merge heading chunks - keep them separate
            if !chunk.heading_hint && !prev.heading_hint && chunk.text.len() < MIN_CHUNK_CHARS {
                let candidate = format!("{}\n{}", prev.text, chunk.text).trim().to_string();
                if candidate.len() <= soft_max {
                    prev.text = candidate;
                    prev.avg_font_size = (prev.avg_font_size + chunk.avg_font_size) / 2.0;
                    continue;
                }
            }
        }
        merged_chunk_records.push(chunk);
    }

    if merged_chunk_records.is_empty() {
        return Err(PyRuntimeError::new_err("No chunks generated from PDF"));
    }

    let rust_ms = start.elapsed().as_secs_f64() * 1000.0;

    let chunk_list: Vec<PyObject> = merged_chunk_records
        .iter()
        .map(|record| {
            let page_number = record.page_number;
            let avg_font_size = record.avg_font_size;
            let is_heading_chunk = record.heading_hint;

            let content_type = if is_heading_chunk {
                ContentType::HeadingSection
            } else {
                classify_chunk_structural(&record.text, avg_font_size, global_avg_font_size)
            };

            let metadata_dict = PyDict::new_bound(py);
            metadata_dict.set_item("page_number", page_number)?;
            metadata_dict.set_item("is_heading", is_heading_chunk)?;
            metadata_dict.set_item("avg_font_size", format!("{:.2}", avg_font_size))?;

            let dict = PyDict::new_bound(py);
            dict.set_item("content", &record.text)?;
            dict.set_item("content_type", content_type.as_str())?;
            dict.set_item("metadata", &metadata_dict)?;
            Ok(dict.into_any().unbind())
        })
        .collect::<PyResult<_>>()?;

    let result = PyDict::new_bound(py);
    result.set_item("chunks", chunk_list)?;
    result.set_item("rust_ms", (rust_ms * 1000.0).round() / 1000.0)?;
    Ok(result.into_any().unbind())
}

pub(super) fn prepare_structural_chunks(
    doc: &PdfDocument<'_>,
) -> Result<StructuralPrepared, String> {
    let prepared = prepare_paragraph_records(doc)?;
    let paragraph_records = prepared.paragraph_records;
    let global_avg_font_size = prepared.global_avg_font_size;

    let all_paragraphs: Vec<Paragraph> = paragraph_records
        .iter()
        .map(|r| r.paragraph.clone())
        .collect();

    let units = absorb_heading_only_units(build_units(all_paragraphs));
    let mut unit_page_meta: Vec<(usize, bool, f32)> = Vec::with_capacity(units.len());
    let mut para_cursor = 0usize;

    for unit in &units {
        let part_count = unit.parts.len();
        if part_count == 0 {
            unit_page_meta.push((1, false, global_avg_font_size));
            continue;
        }

        let start = para_cursor;
        let end = (para_cursor + part_count).min(paragraph_records.len());

        if start >= end {
            unit_page_meta.push((1, false, global_avg_font_size));
            para_cursor = para_cursor.saturating_add(part_count);
            continue;
        }

        let slice = &paragraph_records[start..end];
        let page_number = slice[0].page_number;
        let heading_hint = slice.iter().any(|r| r.paragraph.is_heading);
        let avg_font_size = slice.iter().map(|r| r.avg_font_size).sum::<f32>() / slice.len() as f32;

        unit_page_meta.push((page_number, heading_hint, avg_font_size));
        para_cursor = para_cursor.saturating_add(part_count);
    }

    Ok(StructuralPrepared {
        units,
        unit_page_meta,
        global_avg_font_size,
    })
}

pub(super) fn prepare_paragraph_records(
    doc: &PdfDocument<'_>,
) -> Result<ParagraphsPrepared, String> {
    let spans = extract_spans_from_doc(doc);
    if spans.is_empty() {
        return Err("PDF appears to contain no extractable text".to_string());
    }

    let global_avg_font_size = spans.iter().map(|s| s.font_size).sum::<f32>() / spans.len() as f32;
    let grouped_paragraphs = group_spans_into_paragraphs(spans);
    let paragraph_records = collect_paragraph_records(grouped_paragraphs);

    if paragraph_records.is_empty() {
        return Err("PDF appears to contain no extractable text".to_string());
    }

    Ok(ParagraphsPrepared {
        paragraph_records,
        global_avg_font_size,
    })
}

#[pyfunction]
pub fn chunk_pdf_section(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    if !file_path.to_ascii_lowercase().ends_with(".pdf") {
        return Err(PyValueError::new_err(format!(
            "Expected .pdf file path, got: {file_path}"
        )));
    }

    let start = Instant::now();

    let pdfium = get_pdfium().map_err(PyRuntimeError::new_err)?;
    let doc = pdfium
        .load_pdf_from_file(file_path, None)
        .map_err(|e| PyIOError::new_err(format!("Failed to load PDF: {}", e)))?;

    let spans = extract_spans_from_doc(&doc);
    if spans.is_empty() {
        return Err(PyRuntimeError::new_err(
            "PDF appears to contain no extractable text",
        ));
    }

    let global_avg_font_size = spans.iter().map(|s| s.font_size).sum::<f32>() / spans.len() as f32;
    let grouped_paragraphs = group_spans_into_paragraphs(spans);
    let paragraph_records = collect_paragraph_records(grouped_paragraphs);

    if paragraph_records.is_empty() {
        return Err(PyRuntimeError::new_err(
            "PDF appears to contain no extractable text",
        ));
    }

    let mut section_records: Vec<SectionChunkRecord> = Vec::new();
    let mut current_heading: Option<String> = None;
    let mut current_heading_size = global_avg_font_size;
    let mut current_page_number = paragraph_records[0].page_number;
    let mut current_body_lines: Vec<String> = Vec::new();
    let mut saw_heading = false;

    for record in paragraph_records {
        if record.paragraph.is_heading {
            if current_heading.is_none() && !current_body_lines.is_empty() {
                flush_section(
                    &mut section_records,
                    "Preamble",
                    &current_body_lines,
                    current_page_number,
                    global_avg_font_size,
                    global_avg_font_size,
                );
                current_body_lines.clear();
            }

            if let Some(existing_heading) = current_heading.take() {
                flush_section(
                    &mut section_records,
                    &existing_heading,
                    &current_body_lines,
                    current_page_number,
                    current_heading_size,
                    global_avg_font_size,
                );
                current_body_lines.clear();
            }

            current_heading = Some(record.paragraph.text.trim().to_string());
            current_heading_size = record.avg_font_size;
            current_page_number = record.page_number;
            saw_heading = true;
            continue;
        }

        if current_body_lines.is_empty() {
            current_page_number = record.page_number;
        }
        current_body_lines.push(record.paragraph.text);
    }

    if let Some(existing_heading) = current_heading {
        flush_section(
            &mut section_records,
            &existing_heading,
            &current_body_lines,
            current_page_number,
            current_heading_size,
            global_avg_font_size,
        );
    } else {
        let fallback_heading = if saw_heading { "Preamble" } else { "Document" };
        flush_section(
            &mut section_records,
            fallback_heading,
            &current_body_lines,
            current_page_number,
            global_avg_font_size,
            global_avg_font_size,
        );
    }

    if section_records.is_empty() {
        return Err(PyRuntimeError::new_err(
            "No section chunks generated from PDF",
        ));
    }

    let rust_ms = start.elapsed().as_secs_f64() * 1000.0;

    let chunk_list: Vec<PyObject> = section_records
        .iter()
        .map(|record| {
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
            Ok(dict.into_any().unbind())
        })
        .collect::<PyResult<_>>()?;

    let result = PyDict::new_bound(py);
    result.set_item("chunks", chunk_list)?;
    result.set_item("rust_ms", (rust_ms * 1000.0).round() / 1000.0)?;
    Ok(result.into_any().unbind())
}

#[pyfunction]
pub fn chunk_pdf_semantic(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    if !file_path.to_ascii_lowercase().ends_with(".pdf") {
        return Err(PyValueError::new_err(format!(
            "Expected .pdf file path, got: {file_path}"
        )));
    }

    let start = Instant::now();

    let pdfium = get_pdfium().map_err(PyRuntimeError::new_err)?;
    let doc = pdfium
        .load_pdf_from_file(file_path, None)
        .map_err(|e| PyIOError::new_err(format!("Failed to load PDF: {}", e)))?;

    let spans = extract_spans_from_doc(&doc);
    if spans.is_empty() {
        return Err(PyRuntimeError::new_err(
            "PDF appears to contain no extractable text",
        ));
    }

    let grouped_paragraphs = group_spans_into_paragraphs(spans);
    let paragraph_records = collect_paragraph_records(grouped_paragraphs);

    if paragraph_records.is_empty() {
        return Err(PyRuntimeError::new_err(
            "PDF appears to contain no extractable text",
        ));
    }

    let semantic_chunks = build_pdf_semantic_chunks(paragraph_records);
    if semantic_chunks.is_empty() {
        return Err(PyRuntimeError::new_err(
            "No semantic chunks generated from PDF",
        ));
    }

    let rust_ms = start.elapsed().as_secs_f64() * 1000.0;

    let chunk_list: Vec<PyObject> = semantic_chunks
        .iter()
        .map(|chunk| {
            let text = chunk.paragraphs.join("\n\n");
            let metadata_dict = PyDict::new_bound(py);
            metadata_dict.set_item("page_number", chunk.page_number)?;
            metadata_dict.set_item("paragraph_count", chunk.paragraphs.len())?;
            metadata_dict.set_item("merge_reason", chunk.merge_reason)?;

            let dict = PyDict::new_bound(py);
            dict.set_item("content", &text)?;
            dict.set_item("content_type", "semantic")?;
            dict.set_item("metadata", &metadata_dict)?;
            Ok(dict.into_any().unbind())
        })
        .collect::<PyResult<_>>()?;

    let result = PyDict::new_bound(py);
    result.set_item("chunks", chunk_list)?;
    result.set_item("rust_ms", (rust_ms * 1000.0).round() / 1000.0)?;
    Ok(result.into_any().unbind())
}

#[pyfunction]
pub fn chunk_pdf_sentence(
    py: Python<'_>,
    file_path: &str,
    sentences_per_chunk: usize,
) -> PyResult<PyObject> {
    if !file_path.to_ascii_lowercase().ends_with(".pdf") {
        return Err(PyValueError::new_err(format!(
            "Expected .pdf file path, got: {file_path}"
        )));
    }
    if sentences_per_chunk == 0 {
        return Err(PyValueError::new_err(
            "sentences_per_chunk must be greater than 0",
        ));
    }

    let start = Instant::now();

    let pdfium = get_pdfium().map_err(PyRuntimeError::new_err)?;
    let doc = pdfium
        .load_pdf_from_file(file_path, None)
        .map_err(|e| PyIOError::new_err(format!("Failed to load PDF: {}", e)))?;

    let spans = extract_spans_from_doc(&doc);
    if spans.is_empty() {
        return Err(PyRuntimeError::new_err(
            "PDF appears to contain no extractable text",
        ));
    }

    let grouped_paragraphs = group_spans_into_paragraphs(spans);
    let paragraph_records = collect_paragraph_records(grouped_paragraphs);

    if paragraph_records.is_empty() {
        return Err(PyRuntimeError::new_err(
            "PDF appears to contain no extractable text",
        ));
    }

    let sentence_chunks = build_pdf_sentence_chunks(paragraph_records, sentences_per_chunk);
    if sentence_chunks.is_empty() {
        return Err(PyRuntimeError::new_err(
            "No sentence chunks generated from PDF",
        ));
    }

    let rust_ms = start.elapsed().as_secs_f64() * 1000.0;

    let chunk_list: Vec<PyObject> = sentence_chunks
        .iter()
        .map(|chunk| {
            let metadata_dict = PyDict::new_bound(py);
            metadata_dict.set_item("sentences_per_chunk", sentences_per_chunk)?;
            if let Some(actual_count) = chunk.metadata.get("actual_sentence_count") {
                metadata_dict.set_item("actual_sentence_count", actual_count.as_u64())?;
            }
            if let Some(chunk_idx) = chunk.metadata.get("chunk_index") {
                metadata_dict.set_item("chunk_index", chunk_idx.as_u64())?;
            }
            if let Some(para_idx) = chunk.metadata.get("source_paragraph_index") {
                metadata_dict.set_item("source_paragraph_index", para_idx.as_u64())?;
            }

            let dict = PyDict::new_bound(py);
            dict.set_item("content", &chunk.content)?;
            dict.set_item("content_type", "sentence")?;
            dict.set_item("metadata", &metadata_dict)?;
            Ok(dict.into_any().unbind())
        })
        .collect::<PyResult<_>>()?;

    let result = PyDict::new_bound(py);
    result.set_item("chunks", chunk_list)?;
    result.set_item("rust_ms", (rust_ms * 1000.0).round() / 1000.0)?;
    Ok(result.into_any().unbind())
}

#[pyfunction]
pub fn chunk_pdf_sliding_window(
    py: Python<'_>,
    file_path: &str,
    window_size: usize,
    overlap: usize,
) -> PyResult<PyObject> {
    if !file_path.to_ascii_lowercase().ends_with(".pdf") {
        return Err(PyValueError::new_err(format!(
            "Expected .pdf file path, got: {file_path}"
        )));
    }
    if window_size == 0 {
        return Err(PyValueError::new_err("window_size must be greater than 0"));
    }
    if overlap >= window_size {
        return Err(PyValueError::new_err(
            "overlap must be less than window_size",
        ));
    }

    let start = Instant::now();

    let pdfium = get_pdfium().map_err(PyRuntimeError::new_err)?;
    let doc = pdfium
        .load_pdf_from_file(file_path, None)
        .map_err(|e| PyIOError::new_err(format!("Failed to load PDF: {}", e)))?;

    let spans = extract_spans_from_doc(&doc);
    if spans.is_empty() {
        return Err(PyRuntimeError::new_err(
            "PDF appears to contain no extractable text",
        ));
    }

    let grouped_paragraphs = group_spans_into_paragraphs(spans);
    let paragraph_records = collect_paragraph_records(grouped_paragraphs);

    if paragraph_records.is_empty() {
        return Err(PyRuntimeError::new_err(
            "PDF appears to contain no extractable text",
        ));
    }

    let sliding_chunks = build_pdf_sliding_window_chunks(paragraph_records, window_size, overlap);
    if sliding_chunks.is_empty() {
        return Err(PyRuntimeError::new_err(
            "No sliding-window chunks generated from PDF",
        ));
    }

    let rust_ms = start.elapsed().as_secs_f64() * 1000.0;

    let chunk_list: Vec<PyObject> = sliding_chunks
        .iter()
        .map(|chunk| {
            let para_range = PyList::new_bound(
                py,
                &[
                    chunk.start_paragraph_index as i64,
                    chunk.end_paragraph_index as i64,
                ],
            );
            let metadata_dict = PyDict::new_bound(py);
            metadata_dict.set_item("page_number", chunk.page_number)?;
            metadata_dict.set_item("window_size", window_size)?;
            metadata_dict.set_item("overlap", overlap)?;
            metadata_dict.set_item("window_index", chunk.window_index)?;
            metadata_dict.set_item("paragraph_count", chunk.paragraph_count)?;
            metadata_dict.set_item("paragraph_range", &para_range)?;

            let dict = PyDict::new_bound(py);
            dict.set_item("content", &chunk.content)?;
            dict.set_item("content_type", "sliding_window")?;
            dict.set_item("metadata", &metadata_dict)?;
            Ok(dict.into_any().unbind())
        })
        .collect::<PyResult<_>>()?;

    let result = PyDict::new_bound(py);
    result.set_item("chunks", chunk_list)?;
    result.set_item("rust_ms", (rust_ms * 1000.0).round() / 1000.0)?;
    Ok(result.into_any().unbind())
}

#[pyfunction]
pub fn chunk_pdf_page_aware(
    py: Python<'_>,
    file_path: &str,
    paragraphs_per_page: usize,
) -> PyResult<PyObject> {
    if !file_path.to_ascii_lowercase().ends_with(".pdf") {
        return Err(PyValueError::new_err(format!(
            "Expected .pdf file path, got: {file_path}"
        )));
    }
    if paragraphs_per_page == 0 {
        return Err(PyValueError::new_err(
            "paragraphs_per_page must be greater than 0",
        ));
    }

    let start = Instant::now();

    let pdfium = get_pdfium().map_err(PyRuntimeError::new_err)?;
    let doc = pdfium
        .load_pdf_from_file(file_path, None)
        .map_err(|e| PyIOError::new_err(format!("Failed to load PDF: {}", e)))?;

    let spans = extract_spans_from_doc(&doc);
    if spans.is_empty() {
        return Err(PyRuntimeError::new_err(
            "PDF appears to contain no extractable text",
        ));
    }

    let grouped_paragraphs = group_spans_into_paragraphs(spans);
    let paragraph_records = collect_paragraph_records(grouped_paragraphs);

    if paragraph_records.is_empty() {
        return Err(PyRuntimeError::new_err(
            "PDF appears to contain no extractable text",
        ));
    }

    let page_chunks = build_pdf_page_aware_chunks(paragraph_records, paragraphs_per_page);
    if page_chunks.is_empty() {
        return Err(PyRuntimeError::new_err(
            "No page-aware chunks generated from PDF",
        ));
    }

    let rust_ms = start.elapsed().as_secs_f64() * 1000.0;

    let chunk_list: Vec<PyObject> = page_chunks
        .iter()
        .map(|chunk| {
            let metadata_dict = PyDict::new_bound(py);
            metadata_dict.set_item("page_number", chunk.page_number)?;
            metadata_dict.set_item("page_break_type", chunk.page_break_type)?;
            metadata_dict.set_item("paragraph_count", chunk.paragraph_count)?;

            let doc_meta = PyDict::new_bound(py);
            doc_meta.set_item("source_type", "pdf")?;
            metadata_dict.set_item("document_metadata", &doc_meta)?;

            let dict = PyDict::new_bound(py);
            dict.set_item("content", &chunk.content)?;
            dict.set_item("content_type", "page_aware")?;
            dict.set_item("metadata", &metadata_dict)?;
            Ok(dict.into_any().unbind())
        })
        .collect::<PyResult<_>>()?;

    let result = PyDict::new_bound(py);
    result.set_item("chunks", chunk_list)?;
    result.set_item("rust_ms", (rust_ms * 1000.0).round() / 1000.0)?;
    Ok(result.into_any().unbind())
}

pub fn register(m: &pyo3::Bound<'_, PyModule>) -> pyo3::PyResult<()> {
    m.add_function(pyo3::wrap_pyfunction!(chunk_pdf_fast, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(chunk_pdf, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(chunk_pdf_section, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(chunk_pdf_semantic, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(chunk_pdf_sentence, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(chunk_pdf_sliding_window, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(chunk_pdf_page_aware, m)?)?;
    Ok(())
}
