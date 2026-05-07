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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MdBlockType {
    Heading,
    Paragraph,
    Code,
    Table,
    List,
}

#[derive(Debug, Clone)]
struct MdBlock {
    block_type: MdBlockType,
    content: String,
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
fn chunk_md(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    if !file_path.to_ascii_lowercase().ends_with(".md") {
        return Err(PyValueError::new_err(format!(
            "Expected .md file path, got: {file_path}"
        )));
    }

    // File I/O is not counted in rust_ms; rust_ms measures decode + chunking.
    let bytes = fs::read(file_path)
        .map_err(|e| PyIOError::new_err(format!("Failed to read MD file: {e}")))?;

    let rust_start = Instant::now();
    let chunks_raw = build_chunks_from_md_bytes(&bytes)
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to parse MD: {e}")))?;
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
    m.add_function(wrap_pyfunction!(chunk_md, m)?)?;
    Ok(())
}

fn build_chunks_from_md_bytes(bytes: &[u8]) -> Result<Vec<ChunkRecordInput>, String> {
    let text = std::str::from_utf8(bytes)
        .map(|v| v.to_string())
        .unwrap_or_else(|_| String::from_utf8_lossy(bytes).to_string());

    if text.trim().is_empty() {
        return Err("Markdown file is empty after decoding".to_string());
    }

    let blocks = parse_markdown_blocks(&text);

    let mut chunks: Vec<ChunkRecordInput> = Vec::new();
    let mut current_heading: Option<String> = None;
    let mut prose_parts: Vec<String> = Vec::new();
    let mut prose_len: usize = 0;

    for block in blocks {
        match block.block_type {
            MdBlockType::Heading => {
                flush_prose(
                    &mut chunks,
                    &current_heading,
                    &mut prose_parts,
                    &mut prose_len,
                );
                let heading_text = extract_heading_text(&block.content);
                current_heading = Some(heading_text.clone());
                chunks.push(ChunkRecordInput {
                    content_type: ContentType::HeadingSection,
                    content: heading_text,
                    metadata: md_metadata(None),
                });
            }
            MdBlockType::Code => {
                flush_prose(
                    &mut chunks,
                    &current_heading,
                    &mut prose_parts,
                    &mut prose_len,
                );
                chunks.push(ChunkRecordInput {
                    content_type: ContentType::CodeBlock,
                    content: block.content,
                    metadata: md_metadata(current_heading.clone()),
                });
            }
            MdBlockType::Table => {
                flush_prose(
                    &mut chunks,
                    &current_heading,
                    &mut prose_parts,
                    &mut prose_len,
                );
                chunks.push(ChunkRecordInput {
                    content_type: ContentType::Table,
                    content: block.content,
                    metadata: md_metadata(current_heading.clone()),
                });
            }
            MdBlockType::List => {
                flush_prose(
                    &mut chunks,
                    &current_heading,
                    &mut prose_parts,
                    &mut prose_len,
                );
                let clean = strip_block_content(&block.content, true);
                if !clean.is_empty() {
                    chunks.push(ChunkRecordInput {
                        content_type: ContentType::BulletNumberedList,
                        content: clean,
                        metadata: md_metadata(current_heading.clone()),
                    });
                }
            }
            MdBlockType::Paragraph => {
                let clean = strip_block_content(&block.content, false);
                if clean.is_empty() {
                    continue;
                }
                let sub_blocks = if clean.len() > MAX_CHUNK_CHARS {
                    split_at_sentences(&clean, MAX_CHUNK_CHARS)
                } else {
                    vec![clean]
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
        return Err("No chunks generated from Markdown document".to_string());
    }

    Ok(chunks)
}

fn parse_markdown_blocks(text: &str) -> Vec<MdBlock> {
    let mut blocks: Vec<MdBlock> = Vec::new();
    let mut lines: Vec<&str> = text.lines().collect();

    if text.ends_with('\n') {
        lines.push("");
    }

    let mut i = 0;
    let mut paragraph: Vec<String> = Vec::new();
    let mut list: Vec<String> = Vec::new();
    let mut table: Vec<String> = Vec::new();
    let mut code: Vec<String> = Vec::new();
    let mut in_code_fence = false;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim_end();
        let compact = trimmed.trim();

        if in_code_fence {
            code.push(trimmed.to_string());
            if compact.starts_with("```") {
                blocks.push(MdBlock {
                    block_type: MdBlockType::Code,
                    content: code.join("\n").trim().to_string(),
                });
                code.clear();
                in_code_fence = false;
            }
            i += 1;
            continue;
        }

        if compact.starts_with("```") {
            flush_text_blocks(&mut blocks, &mut paragraph, &mut list, &mut table);
            in_code_fence = true;
            code.push(trimmed.to_string());
            i += 1;
            continue;
        }

        if compact.is_empty() {
            flush_text_blocks(&mut blocks, &mut paragraph, &mut list, &mut table);
            i += 1;
            continue;
        }

        if is_horizontal_rule(compact) {
            flush_text_blocks(&mut blocks, &mut paragraph, &mut list, &mut table);
            i += 1;
            continue;
        }

        if i + 1 < lines.len() && is_setext_underline(lines[i + 1].trim()) && !compact.contains('|')
        {
            flush_text_blocks(&mut blocks, &mut paragraph, &mut list, &mut table);
            blocks.push(MdBlock {
                block_type: MdBlockType::Heading,
                content: format!("{}\n{}", compact, lines[i + 1].trim()),
            });
            i += 2;
            continue;
        }

        if is_atx_heading(compact) {
            flush_text_blocks(&mut blocks, &mut paragraph, &mut list, &mut table);
            blocks.push(MdBlock {
                block_type: MdBlockType::Heading,
                content: compact.to_string(),
            });
            i += 1;
            continue;
        }

        if is_list_item_line(compact) {
            if !table.is_empty() {
                flush_table(&mut blocks, &mut table);
            }
            if !paragraph.is_empty() {
                flush_paragraph(&mut blocks, &mut paragraph);
            }
            list.push(compact.to_string());
            i += 1;
            continue;
        }

        if looks_like_table_row(compact) {
            if !list.is_empty() {
                flush_list(&mut blocks, &mut list);
            }
            if !paragraph.is_empty() {
                flush_paragraph(&mut blocks, &mut paragraph);
            }
            table.push(compact.to_string());
            i += 1;
            continue;
        }

        if !list.is_empty() {
            flush_list(&mut blocks, &mut list);
        }
        if !table.is_empty() {
            flush_table(&mut blocks, &mut table);
        }
        paragraph.push(compact.to_string());
        i += 1;
    }

    flush_text_blocks(&mut blocks, &mut paragraph, &mut list, &mut table);

    if !code.is_empty() {
        blocks.push(MdBlock {
            block_type: MdBlockType::Code,
            content: code.join("\n").trim().to_string(),
        });
    }

    blocks
}

fn flush_text_blocks(
    blocks: &mut Vec<MdBlock>,
    paragraph: &mut Vec<String>,
    list: &mut Vec<String>,
    table: &mut Vec<String>,
) {
    if !paragraph.is_empty() {
        flush_paragraph(blocks, paragraph);
    }
    if !list.is_empty() {
        flush_list(blocks, list);
    }
    if !table.is_empty() {
        flush_table(blocks, table);
    }
}

fn flush_paragraph(blocks: &mut Vec<MdBlock>, paragraph: &mut Vec<String>) {
    let content = paragraph.join("\n").trim().to_string();
    paragraph.clear();
    if content.is_empty() {
        return;
    }
    blocks.push(MdBlock {
        block_type: MdBlockType::Paragraph,
        content,
    });
}

fn flush_list(blocks: &mut Vec<MdBlock>, list: &mut Vec<String>) {
    let content = list.join("\n").trim().to_string();
    list.clear();
    if content.is_empty() {
        return;
    }
    blocks.push(MdBlock {
        block_type: MdBlockType::List,
        content,
    });
}

fn flush_table(blocks: &mut Vec<MdBlock>, table: &mut Vec<String>) {
    let content = table.join("\n").trim().to_string();
    table.clear();
    if content.is_empty() {
        return;
    }
    blocks.push(MdBlock {
        block_type: MdBlockType::Table,
        content,
    });
}

fn is_atx_heading(line: &str) -> bool {
    if line.is_empty() || !line.starts_with('#') {
        return false;
    }
    let hash_count = line.chars().take_while(|c| *c == '#').count();
    (1..=6).contains(&hash_count) && line.chars().nth(hash_count) == Some(' ')
}

fn is_setext_underline(line: &str) -> bool {
    let t = line.trim();
    t.len() >= 3 && (t.chars().all(|c| c == '=') || t.chars().all(|c| c == '-'))
}

fn looks_like_table_row(line: &str) -> bool {
    if !line.contains('|') {
        return false;
    }

    let non_empty_cells = line
        .split('|')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .count();

    non_empty_cells >= 2
        && line
            .split('|')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .any(|cell| !cell.starts_with('[') || cell.contains(']') && !cell.contains("](#"))
}

fn is_horizontal_rule(line: &str) -> bool {
    let t = line.trim();
    if t.len() < 3 {
        return false;
    }
    t.chars().all(|c| c == '-') || t.chars().all(|c| c == '*') || t.chars().all(|c| c == '_')
}

fn is_list_item_line(line: &str) -> bool {
    let t = line.trim();
    if t.starts_with("- ") || t.starts_with("* ") || t.starts_with("+ ") {
        return true;
    }

    if t.starts_with("[x] ") || t.starts_with("[ ] ") || t.starts_with("[X] ") {
        return true;
    }

    let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    if !digits.is_empty() && digits.len() <= 4 {
        let rest = &t[digits.len()..];
        if matches!(rest.chars().next(), Some('.') | Some(')')) && rest.len() > 1 {
            return true;
        }
    }

    false
}

fn extract_heading_text(heading_block: &str) -> String {
    let lines: Vec<&str> = heading_block.lines().collect();
    if lines.is_empty() {
        return heading_block.trim().to_string();
    }

    let first = lines[0].trim();
    if first.starts_with('#') {
        return first.trim_start_matches('#').trim().to_string();
    }

    first.to_string()
}

fn strip_inline(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    let mut out = String::with_capacity(n);
    let mut i = 0;

    while i < n {
        match chars[i] {
            '\\' if i + 1 < n => {
                out.push(chars[i + 1]);
                i += 2;
            }
            '<' if i + 1 < n && (chars[i + 1].is_ascii_alphabetic() || chars[i + 1] == '/') => {
                while i < n && chars[i] != '>' {
                    i += 1;
                }
                if i < n {
                    i += 1;
                }
            }
            '!' if i + 1 < n && chars[i + 1] == '[' => {
                i += 2;
                let alt_start = i;
                while i < n && chars[i] != ']' {
                    i += 1;
                }
                let alt: String = chars[alt_start..i].iter().collect();
                if i < n {
                    i += 1;
                }
                if i < n && chars[i] == '(' {
                    while i < n && chars[i] != ')' {
                        i += 1;
                    }
                    if i < n {
                        i += 1;
                    }
                }
                if !alt.is_empty() {
                    out.push_str(&alt);
                }
            }
            '[' if i + 1 < n && chars[i + 1] == '^' => {
                while i < n && chars[i] != ']' {
                    i += 1;
                }
                if i < n {
                    i += 1;
                }
            }
            '[' => {
                i += 1;
                let text_start = i;
                let mut depth = 1i32;
                while i < n && depth > 0 {
                    if chars[i] == '[' {
                        depth += 1;
                    } else if chars[i] == ']' {
                        depth -= 1;
                    }
                    if depth > 0 {
                        i += 1;
                    }
                }
                let inner: String = chars[text_start..i].iter().collect();
                if i < n {
                    i += 1;
                }
                if i < n && chars[i] == '(' {
                    while i < n && chars[i] != ')' {
                        i += 1;
                    }
                    if i < n {
                        i += 1;
                    }
                } else if i < n && chars[i] == '[' {
                    while i < n && chars[i] != ']' {
                        i += 1;
                    }
                    if i < n {
                        i += 1;
                    }
                }
                out.push_str(&strip_inline(&inner));
            }
            '~' if i + 1 < n && chars[i + 1] == '~' => {
                i += 2;
                let start = i;
                while i + 1 < n && !(chars[i] == '~' && chars[i + 1] == '~') {
                    i += 1;
                }
                let inner: String = chars[start..i].iter().collect();
                if i + 1 < n {
                    i += 2;
                }
                out.push_str(&strip_inline(&inner));
            }
            '*' => {
                if i + 2 < n && chars[i + 1] == '*' && chars[i + 2] == '*' {
                    let open = i + 3;
                    if let Some(pos) = find_md_marker(&chars, open, &['*', '*', '*']) {
                        let inner: String = chars[open..pos].iter().collect();
                        i = pos + 3;
                        out.push_str(&strip_inline(&inner));
                    } else {
                        out.push('*');
                        i += 1;
                    }
                } else if i + 1 < n && chars[i + 1] == '*' {
                    let open = i + 2;
                    if let Some(pos) = find_md_marker(&chars, open, &['*', '*']) {
                        let inner: String = chars[open..pos].iter().collect();
                        i = pos + 2;
                        out.push_str(&strip_inline(&inner));
                    } else {
                        out.push('*');
                        i += 1;
                    }
                } else if i + 1 < n && chars[i + 1] != ' ' && chars[i + 1] != '\t' {
                    let open = i + 1;
                    if let Some(pos) = find_md_marker(&chars, open, &['*']) {
                        let inner: String = chars[open..pos].iter().collect();
                        i = pos + 1;
                        out.push_str(&strip_inline(&inner));
                    } else {
                        out.push('*');
                        i += 1;
                    }
                } else {
                    out.push('*');
                    i += 1;
                }
            }
            '`' => {
                i += 1;
                let start = i;
                while i < n && chars[i] != '`' {
                    i += 1;
                }
                let inner: String = chars[start..i].iter().collect();
                if i < n {
                    i += 1;
                }
                out.push_str(inner.trim());
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }

    out
}

fn find_md_marker(chars: &[char], from: usize, marker: &[char]) -> Option<usize> {
    let ml = marker.len();
    if ml == 0 {
        return None;
    }
    let end = chars.len().saturating_sub(ml - 1);
    (from..end).find(|&i| &chars[i..i + ml] == marker)
}

fn strip_block_content(text: &str, strip_bullets: bool) -> String {
    text.lines()
        .map(|line| {
            let mut l = line.trim_start();
            while l.starts_with('>') {
                l = l.trim_start_matches('>').trim_start();
            }
            let l = if strip_bullets {
                strip_list_prefix(l)
            } else {
                l
            };
            strip_inline(l).trim().to_string()
        })
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn strip_list_prefix(line: &str) -> &str {
    let t = line.trim_start();
    for prefix in &["- ", "* ", "+ "] {
        if let Some(rest) = t.strip_prefix(prefix) {
            return rest.trim_start();
        }
    }
    for prefix in &["[x] ", "[X] ", "[ ] "] {
        if let Some(rest) = t.strip_prefix(prefix) {
            return rest.trim_start();
        }
    }

    let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    if !digits.is_empty() && digits.len() <= 4 {
        let rest = &t[digits.len()..];
        if rest.starts_with(". ") || rest.starts_with(") ") {
            return rest[2..].trim_start();
        }
    }

    t
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
        metadata: md_metadata(heading.clone()),
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

fn split_at_sentences(text: &str, max_chars: usize) -> Vec<String> {
    if text.len() <= max_chars {
        return vec![text.trim().to_string()];
    }

    let mut out = Vec::new();
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

    let mut i = 0;
    while i < chars.len() {
        current.push(chars[i]);
        if matches!(chars[i], '.' | '!' | '?')
            && i + 1 < chars.len()
            && chars[i + 1].is_whitespace()
        {
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
                    continue;
                }
            }
        }

        result.push(chunk);
    }

    result
}

fn md_metadata(section_heading: Option<String>) -> Value {
    json!({
        "footnotes_captions": [],
        "page_number": Value::Null,
        "section_heading": section_heading,
        "document_metadata": {
            "source_type": "md"
        }
    })
}
