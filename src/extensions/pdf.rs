use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};
use pyo3::wrap_pyfunction;
use pythonize::pythonize;
use serde_json::{json, Value};
use std::fs;
use std::time::Instant;

const MAX_CHUNK_CHARS: usize = 1200;
const MIN_CHUNK_CHARS: usize = 350;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContentType {
    PlainParagraph,
    HeadingSection,
    BulletNumberedList,
    Table,
    LongSingleParagraph,
    ShortDisconnectedParagraph,
}

#[derive(Debug, Clone)]
struct ChunkRecordInput {
    content_type: ContentType,
    content: String,
    metadata: Value,
}

#[derive(Debug, Clone)]
struct Paragraph {
    text: String,
    is_heading: bool,
}

#[derive(Debug)]
struct ChunkUnit {
    heading: Option<String>,
    parts: Vec<String>,
}

impl ContentType {
    fn as_str(self) -> &'static str {
        match self {
            ContentType::PlainParagraph => "plain_paragraph",
            ContentType::HeadingSection => "heading",
            ContentType::BulletNumberedList => "bullet_list",
            ContentType::Table => "table",
            ContentType::LongSingleParagraph => "long_single_paragraph",
            ContentType::ShortDisconnectedParagraph => "short_disconnected_paragraph",
        }
    }
}

#[pyfunction]
fn chunk_pdf(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    if !file_path.to_ascii_lowercase().ends_with(".pdf") {
        return Err(PyValueError::new_err(format!(
            "Expected .pdf file path, got: {file_path}"
        )));
    }

    // File I/O is not counted in rust_ms; rust_ms only measures extract + chunking.
    let bytes = fs::read(file_path)
        .map_err(|e| PyIOError::new_err(format!("Failed to read PDF file: {e}")))?;

    let rust_start = Instant::now();
    let chunks_raw = build_chunks_from_pdf_bytes(&bytes)
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to parse PDF: {e}")))?;
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

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(chunk_pdf, m)?)?;
    Ok(())
}

fn build_chunks_from_pdf_bytes(bytes: &[u8]) -> Result<Vec<ChunkRecordInput>, String> {
    let raw_text = pdf_extract::extract_text_from_mem(bytes)
        .map_err(|e| format!("Failed to extract text from PDF: {e}"))?;

    if raw_text.trim().is_empty() {
        return Err("PDF appears to contain no extractable text".to_string());
    }

    let paragraphs = extract_paragraphs(&raw_text);
    let units = absorb_heading_only_units(build_units(paragraphs));

    let mut raw_chunks: Vec<String> = Vec::new();
    for unit in &units {
        raw_chunks.extend(split_unit(unit, MAX_CHUNK_CHARS));
    }

    let chunks = merge_short_chunks(raw_chunks, MIN_CHUNK_CHARS, MAX_CHUNK_CHARS);
    if chunks.is_empty() {
        return Err("No chunks generated from PDF document".to_string());
    }

    Ok(chunks
        .into_iter()
        .map(|text| ChunkRecordInput {
            content_type: classify_chunk(&text),
            content: text,
            metadata: pdf_metadata(),
        })
        .collect())
}

fn extract_paragraphs(raw_text: &str) -> Vec<Paragraph> {
    let mut paragraphs: Vec<Paragraph> = Vec::new();
    let mut in_toc = false;

    for page_text in split_pages(raw_text) {
        for block_lines in split_raw_blocks(&page_text) {
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
                if let Some(para) = build_paragraph(real_lines) {
                    paragraphs.push(para);
                }
                continue;
            }

            if let Some(para) = build_paragraph(clean) {
                paragraphs.push(para);
            }
        }
    }

    paragraphs
}

fn split_pages(text: &str) -> Vec<String> {
    let pages: Vec<String> = text
        .split('\u{000C}')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(String::from)
        .collect();

    if pages.is_empty() {
        vec![text.trim().to_string()]
    } else {
        pages
    }
}

fn split_raw_blocks(text: &str) -> Vec<Vec<String>> {
    let mut blocks: Vec<Vec<String>> = Vec::new();
    let mut current: Vec<String> = Vec::new();

    for line in text.lines() {
        let t = line.trim().to_string();
        if t.is_empty() {
            if !current.is_empty() {
                blocks.push(std::mem::take(&mut current));
            }
        } else {
            current.push(t);
        }
    }

    if !current.is_empty() {
        blocks.push(current);
    }

    blocks
}

fn build_paragraph(lines: Vec<String>) -> Option<Paragraph> {
    if lines.is_empty() {
        return None;
    }

    let is_bullet = is_bullet_line(&lines[0]) || is_numbered_line(&lines[0]);
    let joined = lines.join(" ");
    let is_heading = !is_bullet && is_heading_block(&joined, &lines);

    let text = if is_bullet {
        merge_bullet_lines(&lines).join("\n")
    } else if is_heading {
        lines.join("\n")
    } else {
        lines.join(" ")
    };

    let text = text.trim().to_string();
    if text.is_empty() {
        return None;
    }

    Some(Paragraph { text, is_heading })
}

fn build_units(paragraphs: Vec<Paragraph>) -> Vec<ChunkUnit> {
    let mut units: Vec<ChunkUnit> = Vec::new();
    let mut current_heading: Option<String> = None;
    let mut current_parts: Vec<String> = Vec::new();

    for para in &paragraphs {
        let text = para.text.clone();
        if text.is_empty() {
            continue;
        }

        if para.is_heading {
            if !current_parts.is_empty() {
                units.push(ChunkUnit {
                    heading: current_heading.take(),
                    parts: std::mem::take(&mut current_parts),
                });
            }
            current_heading = Some(text.clone());
            current_parts = vec![text];
        } else {
            current_parts.push(text);
        }
    }

    if !current_parts.is_empty() {
        units.push(ChunkUnit {
            heading: current_heading,
            parts: current_parts,
        });
    }

    if units.is_empty() && !paragraphs.is_empty() {
        let parts: Vec<String> = paragraphs
            .iter()
            .map(|p| p.text.clone())
            .filter(|s| !s.is_empty())
            .collect();
        if !parts.is_empty() {
            return vec![ChunkUnit {
                heading: None,
                parts,
            }];
        }
    }

    units
}

fn absorb_heading_only_units(units: Vec<ChunkUnit>) -> Vec<ChunkUnit> {
    let mut result: Vec<ChunkUnit> = Vec::new();
    let mut pending_prefix: Option<String> = None;

    for unit in units {
        let heading_only = unit.heading.is_some()
            && unit.parts.len() == 1
            && Some(unit.parts[0].as_str()) == unit.heading.as_deref();

        if heading_only {
            pending_prefix = unit.heading;
            continue;
        }

        let unit = if let Some(prefix) = pending_prefix.take() {
            let mut new_parts = vec![prefix];
            new_parts.extend(unit.parts);
            ChunkUnit {
                heading: unit.heading,
                parts: new_parts,
            }
        } else {
            unit
        };

        result.push(unit);
    }

    if let Some(prefix) = pending_prefix {
        result.push(ChunkUnit {
            heading: Some(prefix.clone()),
            parts: vec![prefix],
        });
    }

    result
}

fn split_unit(unit: &ChunkUnit, max_chars: usize) -> Vec<String> {
    let mut chunks: Vec<String> = Vec::new();
    let mut current_parts: Vec<String> = Vec::new();
    let mut current_len: usize = 0;

    for part in &unit.parts {
        if part.len() > max_chars {
            if !current_parts.is_empty() {
                chunks.push(current_parts.join("\n").trim().to_string());
                current_parts.clear();
                current_len = 0;
            }
            chunks.extend(split_large_text(part, max_chars));
            continue;
        }

        let sep = if current_parts.is_empty() { 0 } else { 1 };
        let candidate = current_len + sep + part.len();

        if candidate <= max_chars {
            current_parts.push(part.clone());
            current_len = candidate;
        } else {
            if !current_parts.is_empty() {
                chunks.push(current_parts.join("\n").trim().to_string());
            }
            current_parts = vec![part.clone()];
            current_len = part.len();
        }
    }

    if !current_parts.is_empty() {
        chunks.push(current_parts.join("\n").trim().to_string());
    }

    chunks.into_iter().filter(|c| !c.is_empty()).collect()
}

fn split_large_text(text: &str, max_chars: usize) -> Vec<String> {
    if text.len() <= max_chars {
        return vec![text.trim().to_string()];
    }

    let sentences = split_sentences(text);
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

fn split_sentences(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        current.push(chars[i]);
        if matches!(chars[i], '.' | '!' | '?') && i + 1 < len && chars[i + 1].is_whitespace() {
            let s = current.trim().to_string();
            if !s.is_empty() {
                out.push(s);
            }
            current.clear();
        }
        i += 1;
    }

    let tail = current.trim().to_string();
    if !tail.is_empty() {
        out.push(tail);
    }

    out
}

fn merge_short_chunks(chunks: Vec<String>, min_chars: usize, max_chars: usize) -> Vec<String> {
    if chunks.is_empty() {
        return Vec::new();
    }

    let soft_max = max_chars + min_chars;
    let mut merged: Vec<String> = Vec::new();

    for chunk in chunks {
        if let Some(prev) = merged.last_mut() {
            if chunk.len() < min_chars {
                let candidate = format!("{prev}\n{chunk}").trim().to_string();
                if candidate.len() <= soft_max {
                    *prev = candidate;
                    continue;
                }
            }
        }
        merged.push(chunk);
    }

    merged
}

fn classify_chunk(text: &str) -> ContentType {
    if text.is_empty() {
        return ContentType::PlainParagraph;
    }

    if text.starts_with("Table:") {
        return ContentType::Table;
    }

    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();

    if !lines.is_empty()
        && lines
            .iter()
            .all(|l| is_bullet_line(l.trim()) || is_numbered_line(l.trim()))
    {
        return ContentType::BulletNumberedList;
    }

    if text.len() <= 200 && lines.len() <= 4 {
        let joined = lines.join(" ");
        let wc = joined.split_whitespace().count();
        if wc <= 12 && is_heading_style(&joined) {
            return ContentType::HeadingSection;
        }
    }

    if text.len() > 900 {
        ContentType::LongSingleParagraph
    } else if text.len() < 90 {
        ContentType::ShortDisconnectedParagraph
    } else {
        ContentType::PlainParagraph
    }
}

fn is_heading_block(joined: &str, lines: &[String]) -> bool {
    let word_count = joined.split_whitespace().count();
    if word_count == 0 || word_count > 10 {
        return false;
    }
    if looks_like_sentence(joined) {
        return false;
    }
    if !joined.chars().any(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    if joined.len() > 150 {
        return false;
    }
    lines.iter().all(|l| is_heading_style(l))
}

fn is_heading_style(text: &str) -> bool {
    let t = text.trim();
    let alpha: String = t.chars().filter(|c| c.is_alphabetic()).collect();
    if alpha.is_empty() {
        return false;
    }

    if alpha.len() >= 2 && alpha == alpha.to_uppercase() {
        return true;
    }

    if t.ends_with(':') {
        return true;
    }

    let words: Vec<&str> = t.split_whitespace().collect();

    if words.len() == 1 {
        let first_upper = words[0]
            .chars()
            .next()
            .map(|c| c.is_uppercase())
            .unwrap_or(false);
        return first_upper && words[0].len() >= 4;
    }

    if words.len() <= 8 {
        let title_cased = words
            .iter()
            .all(|w| w.chars().next().map(|c| c.is_uppercase()).unwrap_or(true));
        if title_cased && !looks_like_sentence(t) {
            return true;
        }
    }

    false
}

fn normalize_line(line: &str) -> String {
    line.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_noise_line(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.starts_with("copyright ")
        || lower.contains("codewithmosh.com")
        || is_standalone_number(line)
        || is_page_marker(line)
}

fn is_standalone_number(line: &str) -> bool {
    let t = line.trim();
    !t.is_empty() && t.len() <= 3 && t.chars().all(|c| c.is_ascii_digit())
}

fn is_page_marker(line: &str) -> bool {
    if let Some(rest) = line.trim().strip_prefix("ML Engineer Roadmap ") {
        return !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit());
    }
    false
}

fn is_toc_heading(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower == "table of content" || lower == "table of contents"
}

fn looks_like_toc_entry(line: &str) -> bool {
    let t = line.trim();
    let mut parts = t.rsplitn(2, ' ');
    let last = parts.next().unwrap_or_default();
    let rest = parts.next().unwrap_or_default();
    !rest.is_empty()
        && last.chars().all(|c| c.is_ascii_digit())
        && rest.chars().any(|c| c.is_ascii_alphabetic())
        && rest.split_whitespace().count() <= 8
}

fn is_bullet_line(line: &str) -> bool {
    let t = line.trim();
    t.starts_with("- ")
        || t.starts_with("* ")
        || t.starts_with("\u{2022} ")
        || t.starts_with('\u{2022}')
        || t.starts_with('\u{25E6}')
        || t.starts_with('\u{25AA}')
        || t.starts_with('\u{25B8}')
        || t.starts_with('\u{25B9}')
        || t.starts_with('\u{25BA}')
        || t.starts_with('\u{2023}')
        || t.starts_with('\u{2043}')
}

fn is_numbered_line(line: &str) -> bool {
    let t = line.trim();
    let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() || digits.len() > 3 {
        return false;
    }
    let rest = &t[digits.len()..];
    matches!(rest.chars().next(), Some('.') | Some(')')) && rest.len() > 2
}

fn looks_like_sentence(line: &str) -> bool {
    let words = line.split_whitespace().count();
    words >= 8 || line.ends_with('.') || line.ends_with('!') || line.ends_with('?')
}

fn merge_bullet_lines(lines: &[String]) -> Vec<String> {
    let mut merged: Vec<String> = Vec::new();

    for line in lines {
        if is_bullet_line(line) || is_numbered_line(line) {
            merged.push(line.clone());
        } else if let Some(prev) = merged.last_mut() {
            if is_bullet_line(prev) || is_numbered_line(prev) {
                prev.push(' ');
                prev.push_str(line);
            } else {
                merged.push(line.clone());
            }
        } else {
            merged.push(line.clone());
        }
    }

    merged
}

fn pdf_metadata() -> Value {
    json!({
        "footnotes_captions": [],
        "page_number": null,
        "section_heading": null,
        "document_metadata": {
            "source_type": "pdf"
        },
    })
}
