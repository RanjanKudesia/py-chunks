use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};
use pyo3::wrap_pyfunction;
use pythonize::pythonize;
use quick_xml::events::attributes::Attributes;
use quick_xml::events::Event;
use quick_xml::name::QName;
use quick_xml::Reader;
use serde_json::{json, Value};
use std::fs;
use std::io::{BufReader, Cursor, Read};
use std::time::Instant;
use zip::ZipArchive;

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

#[derive(Debug, Default)]
struct SlideContent {
    title: Option<String>,
    body_paragraphs: Vec<String>,
    has_table: bool,
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
fn chunk_pptx(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    if !file_path.to_ascii_lowercase().ends_with(".pptx") {
        return Err(PyValueError::new_err(format!(
            "Expected .pptx file path, got: {file_path}"
        )));
    }

    // File I/O is not counted in rust_ms; rust_ms measures parse + chunking.
    let bytes = fs::read(file_path)
        .map_err(|e| PyIOError::new_err(format!("Failed to read PPTX file: {e}")))?;

    let rust_start = Instant::now();
    let chunks_raw = build_chunks_from_pptx_bytes(&bytes)
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to parse PPTX: {e}")))?;
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
    m.add_function(wrap_pyfunction!(chunk_pptx, m)?)?;
    Ok(())
}

fn build_chunks_from_pptx_bytes(bytes: &[u8]) -> Result<Vec<ChunkRecordInput>, String> {
    let cursor = Cursor::new(bytes);
    let mut archive =
        ZipArchive::new(cursor).map_err(|e| format!("PPTX is not a valid zip archive: {e}"))?;

    let slide_names = collect_slide_names(&archive);
    if slide_names.is_empty() {
        return Err("No slides found in PPTX archive".to_string());
    }

    let mut raw_chunks: Vec<(String, usize, Option<String>)> = Vec::new();
    for (slide_num, name) in &slide_names {
        let xml_bytes = read_zip_entry_bytes(&mut archive, name)?;
        let slide = parse_slide_xml(&xml_bytes)?;
        let title = slide.title.clone();

        for (text, _) in build_slide_chunks(&slide, *slide_num) {
            raw_chunks.push((text, *slide_num, title.clone()));
        }
    }

    if raw_chunks.is_empty() {
        return Err("No text content found in PPTX document".to_string());
    }

    let texts: Vec<String> = raw_chunks.iter().map(|(t, _, _)| t.clone()).collect();
    let merged = merge_short_chunks(texts, MIN_CHUNK_CHARS, MAX_CHUNK_CHARS);

    if merged.is_empty() {
        return Err("No chunks generated from PPTX document".to_string());
    }

    let result: Vec<ChunkRecordInput> = merged
        .into_iter()
        .enumerate()
        .map(|(i, text)| {
            let (slide_num, title) = raw_chunks
                .get(i)
                .map(|(_, n, t)| (*n, t.clone()))
                .unwrap_or((0, None));

            ChunkRecordInput {
                content_type: classify_chunk(&text),
                content: text,
                metadata: pptx_metadata(slide_num, title),
            }
        })
        .collect();

    Ok(result)
}

fn collect_slide_names(archive: &ZipArchive<Cursor<&[u8]>>) -> Vec<(usize, String)> {
    let mut slides: Vec<(usize, String)> = (0..archive.len())
        .filter_map(|i| {
            let name = archive.name_for_index(i)?.to_string();
            parse_slide_number(&name).map(|n| (n, name))
        })
        .collect();

    slides.sort_by_key(|(n, _)| *n);
    slides
}

fn parse_slide_number(name: &str) -> Option<usize> {
    let stem = name
        .strip_prefix("ppt/slides/slide")?
        .strip_suffix(".xml")?;

    if stem.contains("Layout")
        || stem.contains("Master")
        || stem.contains("layout")
        || stem.contains("master")
    {
        return None;
    }

    stem.parse::<usize>().ok()
}

fn read_zip_entry_bytes(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    name: &str,
) -> Result<Vec<u8>, String> {
    let mut entry = archive
        .by_name(name)
        .map_err(|_| format!("Entry '{name}' not found in PPTX archive"))?;

    let mut buf = Vec::new();
    entry
        .read_to_end(&mut buf)
        .map_err(|e| format!("Failed to read '{name}': {e}"))?;
    Ok(buf)
}

fn parse_slide_xml(xml_bytes: &[u8]) -> Result<SlideContent, String> {
    let mut reader = Reader::from_reader(BufReader::new(xml_bytes));
    let mut buf = Vec::new();

    let mut slide = SlideContent::default();

    let mut sp_depth: i32 = 0;
    let mut sp_is_title = false;
    let mut sp_ph_checked = false;
    let mut in_txbody = false;
    let mut in_para = false;
    let mut para_text = String::new();
    let mut shape_paragraphs: Vec<String> = Vec::new();
    let mut t_buf = String::new();
    let mut in_t = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("XML parse error in slide: {e}")),

            Ok(Event::Start(ref e)) => {
                let local = local_name(e.name());
                match local.as_slice() {
                    b"sp" => {
                        sp_depth += 1;
                        if sp_depth == 1 {
                            sp_is_title = false;
                            sp_ph_checked = false;
                            shape_paragraphs.clear();
                        }
                    }
                    b"ph" if sp_depth > 0 && !sp_ph_checked => {
                        sp_ph_checked = true;
                        if let Some(ph_type) = attr_value(e.attributes(), b"type") {
                            let t = ph_type.to_ascii_lowercase();
                            sp_is_title = matches!(t.as_str(), "title" | "ctrtitle" | "subtitle");
                        }
                    }
                    b"txBody" if sp_depth > 0 => {
                        in_txbody = true;
                    }
                    b"p" if in_txbody => {
                        in_para = true;
                        para_text.clear();
                    }
                    b"t" if in_para => {
                        in_t = true;
                        t_buf.clear();
                    }
                    b"tbl" => {
                        slide.has_table = true;
                    }
                    _ => {}
                }
            }

            Ok(Event::Empty(ref e)) => {
                let local = local_name(e.name());
                if local.as_slice() == b"ph" && sp_depth > 0 && !sp_ph_checked {
                    sp_ph_checked = true;
                    if let Some(ph_type) = attr_value(e.attributes(), b"type") {
                        let t = ph_type.to_ascii_lowercase();
                        sp_is_title = matches!(t.as_str(), "title" | "ctrtitle" | "subtitle");
                    }
                }
            }

            Ok(Event::Text(ref e)) => {
                if in_t {
                    let txt = e.decode().unwrap_or_default().trim().to_string();
                    if !txt.is_empty() {
                        if !t_buf.is_empty() {
                            t_buf.push(' ');
                        }
                        t_buf.push_str(&txt);
                    }
                }
            }

            Ok(Event::End(ref e)) => {
                let local = local_name(e.name());
                match local.as_slice() {
                    b"t" if in_t => {
                        in_t = false;
                        if !t_buf.is_empty() {
                            if !para_text.is_empty() {
                                para_text.push(' ');
                            }
                            para_text.push_str(&t_buf);
                            t_buf.clear();
                        }
                    }
                    b"p" if in_para => {
                        in_para = false;
                        let trimmed = para_text.trim().to_string();
                        if !trimmed.is_empty() {
                            shape_paragraphs.push(trimmed);
                        }
                        para_text.clear();
                    }
                    b"txBody" if in_txbody => {
                        in_txbody = false;
                    }
                    b"sp" if sp_depth > 0 => {
                        sp_depth -= 1;
                        if sp_depth == 0 {
                            let combined = shape_paragraphs.join("\n");
                            let trimmed = combined.trim().to_string();
                            if trimmed.is_empty() {
                                // no text for this shape
                            } else if sp_is_title && slide.title.is_none() {
                                slide.title = Some(trimmed);
                            } else {
                                slide.body_paragraphs.push(trimmed);
                            }

                            shape_paragraphs.clear();
                            sp_is_title = false;
                            sp_ph_checked = false;
                        }
                    }
                    _ => {}
                }
            }

            _ => {}
        }

        buf.clear();
    }

    Ok(slide)
}

fn build_slide_chunks(slide: &SlideContent, slide_num: usize) -> Vec<(String, usize)> {
    let mut lines: Vec<String> = Vec::new();

    if let Some(ref title) = slide.title {
        lines.push(title.clone());
    }
    for para in &slide.body_paragraphs {
        lines.push(para.clone());
    }

    if slide.has_table && !lines.is_empty() {
        lines[0] = format!("Table: {}", lines[0]);
    }

    let full_text = lines.join("\n").trim().to_string();
    if full_text.is_empty() {
        return Vec::new();
    }

    if full_text.len() <= MAX_CHUNK_CHARS {
        return vec![(full_text, slide_num)];
    }

    split_large_text(&full_text, MAX_CHUNK_CHARS)
        .into_iter()
        .map(|t| (t, slide_num))
        .collect()
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

fn classify_chunk(text: &str) -> ContentType {
    if text.is_empty() {
        return ContentType::PlainParagraph;
    }

    if text.lines().any(|l| l.starts_with("Table:")) {
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
        return words[0]
            .chars()
            .next()
            .map(|c| c.is_uppercase())
            .unwrap_or(false)
            && words[0].len() >= 4;
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

fn looks_like_sentence(line: &str) -> bool {
    let words = line.split_whitespace().count();
    words >= 8 || line.ends_with('.') || line.ends_with('!') || line.ends_with('?')
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

fn local_name(name: QName<'_>) -> Vec<u8> {
    name.as_ref()
        .rsplit(|b| *b == b':')
        .next()
        .unwrap_or(name.as_ref())
        .to_vec()
}

fn attr_value(attrs: Attributes<'_>, key: &[u8]) -> Option<String> {
    for attr in attrs.flatten() {
        let aname = attr.key.as_ref();
        let local = aname.rsplit(|b| *b == b':').next().unwrap_or(aname);
        if local == key {
            return attr.unescape_value().ok().map(|v| v.trim().to_string());
        }
    }
    None
}

fn pptx_metadata(slide_number: usize, section_heading: Option<String>) -> Value {
    json!({
        "footnotes_captions": [],
        "page_number": Value::Null,
        "section_heading": section_heading,
        "document_metadata": {
            "source_type": "pptx",
            "slide_number": slide_number,
        },
    })
}
