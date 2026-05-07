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

#[derive(Debug, Clone)]
struct ChunkRecordInput {
    content_type: ContentType,
    content: String,
    metadata: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HtmlBlockType {
    Heading,
    Paragraph,
    Code,
    Table,
    List,
}

#[derive(Debug, Clone)]
struct HtmlBlock {
    block_type: HtmlBlockType,
    content: String,
}

#[derive(Debug)]
struct ParsedTag {
    name: String,
    is_closing: bool,
    is_self_closing: bool,
    end: usize,
}

// ─── pyo3 entry point ────────────────────────────────────────────────────────

#[pyfunction]
fn chunk_html(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    if !file_path.to_ascii_lowercase().ends_with(".html")
        && !file_path.to_ascii_lowercase().ends_with(".htm")
    {
        return Err(PyValueError::new_err(format!(
            "Expected .html / .htm file path, got: {file_path}"
        )));
    }

    let bytes = fs::read(file_path)
        .map_err(|e| PyIOError::new_err(format!("Failed to read HTML file: {e}")))?;

    let rust_start = Instant::now();
    let chunks_raw = build_chunks_from_html_bytes(&bytes)
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to parse HTML: {e}")))?;
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
    m.add_function(wrap_pyfunction!(chunk_html, m)?)?;
    Ok(())
}

// ─── chunking pipeline ────────────────────────────────────────────────────────

fn build_chunks_from_html_bytes(bytes: &[u8]) -> Result<Vec<ChunkRecordInput>, String> {
    let text = std::str::from_utf8(bytes)
        .map(|v| v.to_string())
        .unwrap_or_else(|_| String::from_utf8_lossy(bytes).to_string());

    if text.trim().is_empty() {
        return Err("HTML file is empty after decoding".to_string());
    }

    let blocks = parse_html_blocks(&remove_comments(&text));

    let mut chunks = Vec::new();
    let mut current_heading: Option<String> = None;
    let mut prose_parts: Vec<String> = Vec::new();
    let mut prose_len = 0usize;

    for block in blocks {
        match block.block_type {
            HtmlBlockType::Heading => {
                flush_prose(
                    &mut chunks,
                    &current_heading,
                    &mut prose_parts,
                    &mut prose_len,
                );
                current_heading = Some(block.content.clone());
                chunks.push(ChunkRecordInput {
                    content_type: ContentType::HeadingSection,
                    content: block.content,
                    metadata: html_metadata(None),
                });
            }
            HtmlBlockType::Code => {
                flush_prose(
                    &mut chunks,
                    &current_heading,
                    &mut prose_parts,
                    &mut prose_len,
                );
                chunks.push(ChunkRecordInput {
                    content_type: ContentType::CodeBlock,
                    content: block.content,
                    metadata: html_metadata(current_heading.clone()),
                });
            }
            HtmlBlockType::Table => {
                flush_prose(
                    &mut chunks,
                    &current_heading,
                    &mut prose_parts,
                    &mut prose_len,
                );
                chunks.push(ChunkRecordInput {
                    content_type: ContentType::Table,
                    content: block.content,
                    metadata: html_metadata(current_heading.clone()),
                });
            }
            HtmlBlockType::List => {
                flush_prose(
                    &mut chunks,
                    &current_heading,
                    &mut prose_parts,
                    &mut prose_len,
                );
                if !block.content.is_empty() {
                    chunks.push(ChunkRecordInput {
                        content_type: ContentType::BulletNumberedList,
                        content: block.content,
                        metadata: html_metadata(current_heading.clone()),
                    });
                }
            }
            HtmlBlockType::Paragraph => {
                if block.content.is_empty() {
                    continue;
                }
                let parts = if block.content.len() > MAX_CHUNK_CHARS {
                    split_at_sentences(&block.content, MAX_CHUNK_CHARS)
                } else {
                    vec![block.content]
                };

                for part in parts {
                    let add = part.len() + 1;
                    if prose_len + add > MAX_CHUNK_CHARS && !prose_parts.is_empty() {
                        flush_prose(
                            &mut chunks,
                            &current_heading,
                            &mut prose_parts,
                            &mut prose_len,
                        );
                    }
                    prose_len += add;
                    prose_parts.push(part);
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
        return Err("No chunks generated from HTML document".to_string());
    }

    Ok(chunks)
}

// ─── HTML parser ─────────────────────────────────────────────────────────────

fn parse_html_blocks(html: &str) -> Vec<HtmlBlock> {
    let mut blocks = Vec::new();
    let mut i = 0usize;
    let bytes = html.as_bytes();

    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }

        let Some(tag) = parse_tag_at(html, i) else {
            i += 1;
            continue;
        };

        if tag.is_closing {
            i = tag.end;
            continue;
        }

        if is_ignored_container(&tag.name) {
            i = find_matching_tag_end(html, tag.end, &tag.name).unwrap_or(tag.end);
            continue;
        }

        if tag.name == "hr" {
            i = tag.end;
            continue;
        }

        let Some(block_type) = tag_to_block_type(&tag.name) else {
            i = tag.end;
            continue;
        };

        let Some(end) = find_matching_tag_end(html, tag.end, &tag.name) else {
            i = tag.end;
            continue;
        };

        if let Some(content) = extract_block_content(&html[i..end], &tag.name, block_type) {
            if !content.is_empty() {
                blocks.push(HtmlBlock {
                    block_type,
                    content,
                });
            }
        }

        i = end;
    }

    blocks
}

fn tag_to_block_type(tag: &str) -> Option<HtmlBlockType> {
    match tag {
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => Some(HtmlBlockType::Heading),
        "ul" | "ol" => Some(HtmlBlockType::List),
        "table" => Some(HtmlBlockType::Table),
        "pre" => Some(HtmlBlockType::Code),
        "p" | "blockquote" | "figcaption" | "summary" | "legend" | "label" | "textarea"
        | "option" | "button" | "address" | "dd" | "dt" | "aside" | "nav" => {
            Some(HtmlBlockType::Paragraph)
        }
        _ => None,
    }
}

fn is_ignored_container(tag: &str) -> bool {
    matches!(
        tag,
        "script" | "style" | "noscript" | "head" | "svg" | "canvas"
    )
}

fn extract_block_content(raw: &str, tag: &str, block_type: HtmlBlockType) -> Option<String> {
    match block_type {
        HtmlBlockType::Heading => {
            let inner = extract_inner_html(raw, tag)?;
            let text = normalize_inline_text(&strip_tags(&inner, true));
            if text.is_empty() {
                None
            } else {
                Some(text)
            }
        }
        HtmlBlockType::Paragraph => {
            let inner = extract_inner_html(raw, tag)?;
            let text = normalize_inline_text(&strip_tags(&inner, true));
            if is_noise_text(&text) {
                None
            } else {
                Some(text)
            }
        }
        HtmlBlockType::Code => {
            let inner = extract_inner_html(raw, tag)?;
            let text = normalize_code_text(&strip_tags(&inner, false));
            if text.is_empty() {
                None
            } else {
                Some(text)
            }
        }
        HtmlBlockType::List => {
            let mut items = Vec::new();
            for li_raw in extract_tag_blocks(raw, "li") {
                if let Some(inner) = extract_inner_html(li_raw, "li") {
                    let item = normalize_inline_text(&strip_tags(&inner, true));
                    if !item.is_empty() {
                        items.push(item);
                    }
                }
            }
            if items.is_empty() {
                None
            } else {
                Some(items.join("\n"))
            }
        }
        HtmlBlockType::Table => {
            let mut rows = Vec::new();
            for tr_raw in extract_tag_blocks(raw, "tr") {
                let mut cells = Vec::new();
                for cell_tag in ["th", "td"] {
                    for cell_raw in extract_tag_blocks(tr_raw, cell_tag) {
                        if let Some(inner) = extract_inner_html(cell_raw, cell_tag) {
                            let cell = normalize_inline_text(&strip_tags(&inner, true));
                            if !cell.is_empty() {
                                cells.push(cell);
                            }
                        }
                    }
                }
                if !cells.is_empty() {
                    rows.push(cells.join(" | "));
                }
            }
            if rows.is_empty() {
                None
            } else {
                Some(rows.join("\n"))
            }
        }
    }
}

fn extract_tag_blocks<'a>(html: &'a str, tag: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut i = 0usize;
    let bytes = html.as_bytes();

    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        let Some(parsed) = parse_tag_at(html, i) else {
            i += 1;
            continue;
        };

        if parsed.is_closing || parsed.name != tag {
            i = parsed.end;
            continue;
        }

        let Some(end) = find_matching_tag_end(html, parsed.end, tag) else {
            i = parsed.end;
            continue;
        };

        out.push(&html[i..end]);
        i = end;
    }

    out
}

fn extract_inner_html(raw: &str, tag: &str) -> Option<String> {
    let open = parse_tag_at(raw, 0)?;
    if open.is_closing || open.name != tag {
        return None;
    }
    let close = find_last_close_tag(raw, tag)?;
    Some(raw[open.end..close].to_string())
}

fn find_last_close_tag(html: &str, tag: &str) -> Option<usize> {
    let close = format!("</{tag}");
    html.to_ascii_lowercase().rfind(&close)
}

fn strip_tags(input: &str, newline_for_breaks: bool) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0usize;

    while i < bytes.len() {
        if bytes[i] == b'<' {
            if let Some(tag) = parse_tag_at(input, i) {
                if newline_for_breaks && matches!(tag.name.as_str(), "br" | "p" | "li" | "tr") {
                    out.push('\n');
                }
                i = tag.end;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }

    decode_html_entities(&out)
}

fn normalize_inline_text(input: &str) -> String {
    input
        .lines()
        .map(collapse_whitespace)
        .map(|line| strip_angle_wrapped_tokens(&line))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn strip_angle_wrapped_tokens(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());
    let mut i = 0usize;

    while i < chars.len() {
        if chars[i] == '<' {
            let mut j = i + 1;
            while j < chars.len() && chars[j] != '>' {
                j += 1;
            }

            if j < chars.len() && j > i + 1 {
                let inner: String = chars[i + 1..j].iter().collect();
                if inner
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_'))
                {
                    out.push_str(inner.trim_matches('/'));
                    i = j + 1;
                    continue;
                }
            }
        }

        out.push(chars[i]);
        i += 1;
    }

    collapse_whitespace(&out)
}

fn normalize_code_text(input: &str) -> String {
    input
        .lines()
        .map(|line| line.trim_end().to_string())
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn collapse_whitespace(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_space = false;

    for c in input.chars() {
        if c.is_whitespace() {
            if !in_space {
                out.push(' ');
                in_space = true;
            }
        } else {
            out.push(c);
            in_space = false;
        }
    }

    out.trim().to_string()
}

fn is_noise_text(text: &str) -> bool {
    let t = text.trim();
    t.is_empty()
        || t.chars()
            .all(|c| matches!(c, '|' | '-' | '_' | '*' | '=' | '.'))
}

fn decode_html_entities(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0usize;

    while i < chars.len() {
        if chars[i] != '&' {
            out.push(chars[i]);
            i += 1;
            continue;
        }

        let mut j = i + 1;
        while j < chars.len() && j - i <= 12 && chars[j] != ';' {
            j += 1;
        }

        if j < chars.len() && chars[j] == ';' {
            let entity: String = chars[i + 1..j].iter().collect();
            if let Some(decoded) = decode_entity(&entity) {
                out.push_str(&decoded);
                i = j + 1;
                continue;
            }
        }

        out.push(chars[i]);
        i += 1;
    }

    out
}

fn decode_entity(entity: &str) -> Option<String> {
    match entity {
        "amp" => Some("&".to_string()),
        "lt" => Some("<".to_string()),
        "gt" => Some(">".to_string()),
        "quot" => Some("\"".to_string()),
        "apos" | "#39" => Some("'".to_string()),
        "nbsp" => Some(" ".to_string()),
        "bull" => Some("*".to_string()),
        "mdash" | "ndash" | "minus" => Some("-".to_string()),
        _ => {
            if let Some(hex) = entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
            {
                let value = u32::from_str_radix(hex, 16).ok()?;
                return char::from_u32(value).map(|c| c.to_string());
            }
            if let Some(dec) = entity.strip_prefix('#') {
                let value = dec.parse::<u32>().ok()?;
                return char::from_u32(value).map(|c| c.to_string());
            }
            None
        }
    }
}

fn parse_tag_at(s: &str, start: usize) -> Option<ParsedTag> {
    let bytes = s.as_bytes();
    if start >= bytes.len() || bytes[start] != b'<' {
        return None;
    }

    let mut i = start + 1;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    if i >= bytes.len() {
        return None;
    }

    let mut is_closing = false;
    if bytes[i] == b'/' {
        is_closing = true;
        i += 1;
    }

    let name_start = i;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-') {
        i += 1;
    }
    if i == name_start {
        return None;
    }

    let name = s[name_start..i].to_ascii_lowercase();

    while i < bytes.len() && bytes[i] != b'>' {
        i += 1;
    }
    if i >= bytes.len() {
        return None;
    }

    let mut is_self_closing = i > start + 1 && bytes[i - 1] == b'/';
    if is_void_tag(&name) {
        is_self_closing = true;
    }

    Some(ParsedTag {
        name,
        is_closing,
        is_self_closing,
        end: i + 1,
    })
}

fn is_void_tag(tag: &str) -> bool {
    matches!(
        tag,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

fn find_matching_tag_end(s: &str, from: usize, tag: &str) -> Option<usize> {
    let mut depth = 1usize;
    let mut i = from;
    let bytes = s.as_bytes();

    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }

        let parsed = parse_tag_at(s, i)?;
        if parsed.name == tag {
            if parsed.is_closing {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(parsed.end);
                }
            } else if !parsed.is_self_closing {
                depth += 1;
            }
        }

        i = parsed.end;
    }

    None
}

fn remove_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut i = 0usize;
    let bytes = input.as_bytes();

    while i < bytes.len() {
        if i + 3 < bytes.len() && &bytes[i..i + 4] == b"<!--" {
            i += 4;
            while i + 2 < bytes.len() && &bytes[i..i + 3] != b"-->" {
                i += 1;
            }
            if i + 2 < bytes.len() {
                i += 3;
            }
            continue;
        }
        out.push(bytes[i] as char);
        i += 1;
    }

    out
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
        metadata: html_metadata(heading.clone()),
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
    let mut i = 0usize;

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

fn html_metadata(heading: Option<String>) -> Value {
    let mut map = serde_json::Map::new();
    map.insert("section_heading".to_string(), json!(null));
    map.insert("footnotes_captions".to_string(), json!([]));
    map.insert("page_number".to_string(), json!(null));
    let mut doc_meta = serde_json::Map::new();
    doc_meta.insert("source_type".to_string(), json!("html"));
    if let Some(h) = heading.filter(|h| !h.trim().is_empty()) {
        doc_meta.insert("heading_context".to_string(), json!(h));
    }
    map.insert("document_metadata".to_string(), Value::Object(doc_meta));
    Value::Object(map)
}
