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
    CodeBlock,
    LongSingleParagraph,
    ShortDisconnectedParagraph,
}

#[derive(Debug, Clone)]
struct ChunkRecordInput {
    content_type: ContentType,
    content: String,
    metadata: Value,
}

impl ContentType {
    fn as_str(self) -> &'static str {
        match self {
            ContentType::PlainParagraph => "plain_paragraph",
            ContentType::HeadingSection => "heading",
            ContentType::BulletNumberedList => "bullet_list",
            ContentType::Table => "table",
            ContentType::CodeBlock => "code_block",
            ContentType::LongSingleParagraph => "long_single_paragraph",
            ContentType::ShortDisconnectedParagraph => "short_disconnected_paragraph",
        }
    }
}

#[pyfunction]
fn chunk_txt(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    if !file_path.to_ascii_lowercase().ends_with(".txt") {
        return Err(PyValueError::new_err(format!(
            "Expected .txt file path, got: {file_path}"
        )));
    }

    // File I/O is not counted in rust_ms; rust_ms measures decode + chunking.
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

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(chunk_txt, m)?)?;
    Ok(())
}

fn build_chunks_from_txt_bytes(bytes: &[u8]) -> Result<Vec<ChunkRecordInput>, String> {
    let text = std::str::from_utf8(bytes)
        .map(|v| v.to_string())
        .unwrap_or_else(|_| String::from_utf8_lossy(bytes).to_string());

    if text.trim().is_empty() {
        return Err("TXT file is empty after decoding".to_string());
    }

    let blocks = split_blocks(&text);

    let mut chunks: Vec<ChunkRecordInput> = Vec::new();
    let mut current_heading: Option<String> = None;
    let mut prose_parts: Vec<String> = Vec::new();
    let mut prose_len: usize = 0;

    for block in &blocks {
        let block_type = classify_block(block);

        match block_type {
            ContentType::HeadingSection => {
                flush_prose(
                    &mut chunks,
                    &current_heading,
                    &mut prose_parts,
                    &mut prose_len,
                );
                let heading_text = extract_heading_text(block);
                current_heading = Some(heading_text.clone());
                chunks.push(ChunkRecordInput {
                    content_type: ContentType::HeadingSection,
                    content: heading_text,
                    metadata: txt_metadata(None),
                });
            }
            ContentType::Table | ContentType::CodeBlock | ContentType::BulletNumberedList => {
                flush_prose(
                    &mut chunks,
                    &current_heading,
                    &mut prose_parts,
                    &mut prose_len,
                );
                chunks.push(ChunkRecordInput {
                    content_type: block_type,
                    content: block.clone(),
                    metadata: txt_metadata(current_heading.clone()),
                });
            }
            _ => {
                let sub_blocks = if block.len() > MAX_CHUNK_CHARS {
                    split_at_sentences(block, MAX_CHUNK_CHARS)
                } else {
                    vec![block.clone()]
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

    flush_prose(
        &mut chunks,
        &current_heading,
        &mut prose_parts,
        &mut prose_len,
    );

    let chunks = merge_short_prose(chunks, MIN_CHUNK_CHARS);

    if chunks.is_empty() {
        return Err("No chunks generated from TXT document".to_string());
    }

    Ok(chunks)
}

fn split_blocks(text: &str) -> Vec<String> {
    let mut result: Vec<String> = Vec::new();
    for raw in text.split("\n\n") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            result.extend(split_at_setext_boundaries(trimmed));
        }
    }
    result
}

fn split_at_setext_boundaries(block: &str) -> Vec<String> {
    let lines: Vec<&str> = block.lines().collect();
    if lines.len() < 2 {
        return vec![block.trim().to_string()];
    }

    let mut result: Vec<String> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        if i + 1 < lines.len() && !lines[i].trim().is_empty() && is_setext_underline(lines[i + 1]) {
            if !current.is_empty() {
                let s = current.join("\n").trim().to_string();
                if !s.is_empty() {
                    result.push(s);
                }
                current.clear();
            }
            result.push(format!("{}\n{}", lines[i].trim(), lines[i + 1].trim()));
            i += 2;
            continue;
        }
        current.push(lines[i]);
        i += 1;
    }

    if !current.is_empty() {
        let s = current.join("\n").trim().to_string();
        if !s.is_empty() {
            result.push(s);
        }
    }

    result
}

fn is_setext_underline(line: &str) -> bool {
    let t = line.trim();
    t.len() >= 4 && !t.contains('|') && (t.chars().all(|c| c == '=') || t.chars().all(|c| c == '-'))
}

fn classify_block(block: &str) -> ContentType {
    let lines: Vec<&str> = block
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();

    if lines.is_empty() {
        return ContentType::ShortDisconnectedParagraph;
    }

    if block.trim_start().starts_with("```") {
        return ContentType::CodeBlock;
    }

    if (lines[0] == "{" || lines[0].starts_with("{\"")) && block.trim_end().ends_with('}') {
        return ContentType::CodeBlock;
    }

    if looks_like_command_block(&lines) {
        return ContentType::CodeBlock;
    }

    if lines.len() == 2 && is_setext_underline(lines[1]) {
        return ContentType::HeadingSection;
    }

    if lines.len() == 1 && (lines[0].starts_with("# ") || lines[0].starts_with("## ")) {
        return ContentType::HeadingSection;
    }

    {
        let alpha: String = lines[0].chars().filter(|c| c.is_alphabetic()).collect();
        if !alpha.is_empty()
            && alpha == alpha.to_uppercase()
            && lines[0].len() <= 80
            && !lines[0].contains('|')
            && lines.len() == 1
        {
            return ContentType::HeadingSection;
        }
    }

    {
        let pipe_count = lines.iter().filter(|l| l.contains('|')).count();
        if lines.len() >= 2 && pipe_count * 2 >= lines.len() {
            return ContentType::Table;
        }
    }

    if is_list_block(&lines) {
        return ContentType::BulletNumberedList;
    }

    if block.len() > 500 {
        ContentType::LongSingleParagraph
    } else if block.len() < 80 {
        ContentType::ShortDisconnectedParagraph
    } else {
        ContentType::PlainParagraph
    }
}

fn is_list_block(lines: &[&str]) -> bool {
    if lines.is_empty() {
        return false;
    }
    let list_count = lines.iter().filter(|l| is_list_item_line(l)).count();
    is_list_item_line(lines[0]) && list_count * 2 >= lines.len()
}

fn is_list_item_line(line: &str) -> bool {
    let t = line.trim();
    if t.starts_with("- ") || t.starts_with("* ") || t.starts_with("+ ") {
        return true;
    }

    if t.starts_with("[x]") || t.starts_with("[ ]") || t.starts_with("[X]") {
        return true;
    }

    let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    if !digits.is_empty() && digits.len() <= 3 {
        let rest = &t[digits.len()..];
        if matches!(rest.chars().next(), Some('.') | Some(')')) && rest.len() > 1 {
            return true;
        }
    }

    if t.len() >= 3 {
        let mut chars = t.chars();
        let first = chars.next().unwrap_or_default();
        if first.is_ascii_alphabetic() {
            if let Some(sep) = chars.next() {
                if sep == ')' || sep == '.' {
                    return true;
                }
            }
        }
    }

    false
}

fn looks_like_command_block(lines: &[&str]) -> bool {
    const COMMAND_STARTS: &[&str] = &[
        "source ", "python ", "python3 ", "cargo ", "npm ", "yarn ", "cd ", "git ", "export ",
        "pip ", "pip3 ", "brew ", "apt ", "sudo ", "bash ", "sh ", "make ", "./", "docker ",
        "kubectl ",
    ];

    lines.iter().any(|l| {
        let t = if l.starts_with('#') {
            l.trim_start_matches('#').trim()
        } else {
            l.trim()
        };
        COMMAND_STARTS.iter().any(|p| t.starts_with(p))
    })
}

fn extract_heading_text(block: &str) -> String {
    let lines: Vec<&str> = block.lines().collect();
    if lines.is_empty() {
        return block.trim().to_string();
    }

    if lines.len() >= 2 && is_setext_underline(lines[1].trim()) {
        return lines[0].trim().to_string();
    }

    if lines[0].trim().starts_with('#') {
        return lines[0].trim().trim_start_matches('#').trim().to_string();
    }

    lines[0].trim().to_string()
}

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

fn classify_prose(text: &str) -> ContentType {
    if text.len() > 900 {
        ContentType::LongSingleParagraph
    } else if text.len() < 90 {
        ContentType::ShortDisconnectedParagraph
    } else {
        ContentType::PlainParagraph
    }
}

fn merge_short_prose(chunks: Vec<ChunkRecordInput>, min_chars: usize) -> Vec<ChunkRecordInput> {
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

                if prev_prose && prev.content.len() + chunk.content.len() + 1 <= soft_max {
                    prev.content = format!("{}\n{}", prev.content, chunk.content)
                        .trim()
                        .to_string();
                    prev.content_type = classify_prose(&prev.content);
                    continue;
                }
            }
        }

        result.push(chunk);
    }

    result
}

fn split_at_sentences(text: &str, max_chars: usize) -> Vec<String> {
    if text.len() <= max_chars {
        return vec![text.trim().to_string()];
    }

    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();

    for sentence in split_sentences(text) {
        let candidate = if current.is_empty() {
            sentence.clone()
        } else {
            format!("{} {}", current, sentence)
        };

        if candidate.len() <= max_chars {
            current = candidate;
        } else {
            if !current.is_empty() {
                out.push(current.trim().to_string());
            }
            current = sentence;
        }
    }

    if !current.trim().is_empty() {
        out.push(current.trim().to_string());
    }

    if out.is_empty() {
        vec![text.trim().to_string()]
    } else {
        out.into_iter().filter(|c| !c.is_empty()).collect()
    }
}

fn split_sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
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

fn txt_metadata(section_heading: Option<String>) -> Value {
    json!({
        "footnotes_captions": [],
        "page_number": Value::Null,
        "section_heading": section_heading,
        "document_metadata": {
            "source_type": "txt"
        },
    })
}
