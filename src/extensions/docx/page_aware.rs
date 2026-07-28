use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyModule};
use pyo3::wrap_pyfunction;
use pythonize::pythonize;
use serde_json::{json, Value};

use super::common::{
    image_hash_name, parse_docx_paragraph_events, parse_docx_paragraph_events_with_images,
    parse_rels_xml_images, PageBreakSignal, ParaOrImage, ParagraphEvent,
};
use std::collections::HashMap;
use std::fs;
use std::io::{Cursor, Read};
use std::time::Instant;
use zip::ZipArchive;

const MAX_PAGE_AWARE_CHUNK_CHARS: usize = 2000;

fn page_break_signal_str(signal: PageBreakSignal) -> &'static str {
    match signal {
        PageBreakSignal::Explicit => "explicit",
        PageBreakSignal::Section => "section",
        PageBreakSignal::Rendered => "rendered",
        PageBreakSignal::None => "estimated",
    }
}

#[derive(Debug, Clone)]
struct ChunkRecordInput {
    content: String,
    metadata: Value,
}

#[pyfunction]
fn chunk_docx_page_aware(
    py: Python<'_>,
    file_path: &str,
    paragraphs_per_page: usize,
) -> PyResult<PyObject> {
    if !crate::extensions::docx::common::is_word_ooxml(file_path) {
        return Err(PyValueError::new_err(format!(
            "Expected a Word OOXML file ({}), got: {file_path}",
            crate::extensions::docx::common::word_exts_display()
        )));
    }
    if paragraphs_per_page == 0 {
        return Err(PyValueError::new_err(
            "paragraphs_per_page must be greater than 0",
        ));
    }

    let bytes = fs::read(file_path)
        .map_err(|e| PyIOError::new_err(format!("Failed to read DOCX file: {e}")))?;

    let rust_start = Instant::now();
    let events = parse_docx_paragraph_events(&bytes)
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to parse DOCX: {e}")))?;
    let chunks_raw = build_page_aware_chunks(events, paragraphs_per_page);
    let rust_ms = rust_start.elapsed().as_secs_f64() * 1000.0;

    let chunk_list: Vec<PyObject> = chunks_raw
        .into_iter()
        .map(|c| {
            let dict = PyDict::new_bound(py);
            dict.set_item("content", &c.content)?;
            dict.set_item("content_type", "page_aware")?;
            dict.set_item("metadata", pythonize(py, &c.metadata)?)?;
            Ok(dict.into_any().unbind())
        })
        .collect::<PyResult<_>>()?;

    let result = PyDict::new_bound(py);
    result.set_item("chunks", chunk_list)?;
    result.set_item("rust_ms", (rust_ms * 1000.0).round() / 1000.0)?;
    Ok(result.into_any().unbind())
}

#[pyfunction]
fn chunk_docx_page_aware_with_images(
    py: Python<'_>,
    file_path: &str,
    paragraphs_per_page: usize,
) -> PyResult<(Vec<PyObject>, Vec<(String, Py<PyBytes>)>)> {
    if !crate::extensions::docx::common::is_word_ooxml(file_path) {
        return Err(PyValueError::new_err(format!(
            "Expected a Word OOXML file ({}), got: {file_path}",
            crate::extensions::docx::common::word_exts_display()
        )));
    }
    if paragraphs_per_page == 0 {
        return Err(PyValueError::new_err(
            "paragraphs_per_page must be greater than 0",
        ));
    }

    let bytes = fs::read(file_path)
        .map_err(|e| PyIOError::new_err(format!("Failed to read DOCX file: {e}")))?;

    let cursor = Cursor::new(bytes.clone());
    let mut archive = ZipArchive::new(cursor)
        .map_err(|e| PyRuntimeError::new_err(format!("Not a valid DOCX ZIP: {e}")))?;

    let image_rids_map = match archive.by_name("word/_rels/document.xml.rels") {
        Ok(mut f) => {
            let mut xml = String::new();
            let _ = f.read_to_string(&mut xml);
            parse_rels_xml_images(&xml)
        }
        Err(_) => HashMap::new(),
    };

    let items = parse_docx_paragraph_events_with_images(&bytes)
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to parse DOCX: {e}")))?;

    let mut text_events: Vec<ParagraphEvent> = Vec::new();
    let mut image_items: Vec<(Option<String>, Option<String>)> = Vec::new();

    for item in items {
        match item {
            ParaOrImage::Para(ev) => text_events.push(ev),
            ParaOrImage::Image { rid, alt, signal } => {
                if !matches!(signal, PageBreakSignal::None) {
                    if let Some(last) = text_events.last_mut() {
                        if matches!(last.signal, PageBreakSignal::None) {
                            last.signal = signal;
                        }
                    }
                }
                image_items.push((rid, alt));
            }
        }
    }

    let text_chunks = build_page_aware_chunks(text_events, paragraphs_per_page);

    let mut image_out: Vec<(String, Vec<u8>)> = Vec::new();
    let mut image_chunk_entries: Vec<(String, serde_json::Value)> = Vec::new();
    for (rid, alt) in image_items {
        if let Some(rid) = rid {
            if let Some(zip_path) = image_rids_map.get(&rid) {
                if let Ok(mut entry) = archive.by_name(zip_path) {
                    let mut img_bytes = Vec::new();
                    if entry.read_to_end(&mut img_bytes).is_ok() {
                        if let Some(hash_name) = image_hash_name(&img_bytes, zip_path) {
                            if !image_out.iter().any(|(n, _)| n == &hash_name) {
                                image_out.push((hash_name.clone(), img_bytes));
                            }
                            let alt_str = alt.as_deref().unwrap_or("");
                            image_chunk_entries.push((
                                hash_name.clone(),
                                serde_json::json!({
                                    "image_name": hash_name,
                                    "alt_text": alt_str,
                                }),
                            ));
                        }
                    }
                }
            }
        }
    }

    let mut chunk_list: Vec<PyObject> = Vec::new();
    for (hash_name, metadata) in image_chunk_entries {
        let dict = PyDict::new_bound(py);
        dict.set_item("content", &hash_name)?;
        dict.set_item("content_type", "image")?;
        dict.set_item("metadata", pythonize(py, &metadata)?)?;
        chunk_list.push(dict.into_any().unbind());
    }
    for chunk in text_chunks {
        let dict = PyDict::new_bound(py);
        dict.set_item("content", &chunk.content)?;
        dict.set_item("content_type", "page_aware")?;
        dict.set_item("metadata", pythonize(py, &chunk.metadata)?)?;
        chunk_list.push(dict.into_any().unbind());
    }

    let image_out_py: Vec<(String, Py<PyBytes>)> = image_out
        .into_iter()
        .map(|(name, data)| (name, PyBytes::new_bound(py, &data).unbind()))
        .collect();

    Ok((chunk_list, image_out_py))
}

fn build_page_aware_chunks(
    events: Vec<ParagraphEvent>,
    paragraphs_per_page: usize,
) -> Vec<ChunkRecordInput> {
    if events.is_empty() {
        return Vec::new();
    }

    let mut chunks = Vec::new();
    let mut page_number = 1usize;
    let mut current_paragraphs: Vec<String> = Vec::new();
    let mut current_headings: Vec<Value> = Vec::new();
    let mut current_section_heading_level: Option<u32> = None;
    let mut current_list_items = 0usize;
    let mut current_table_count = 0usize;
    let mut paragraph_count = 0usize;

    for event in events {
        if event.is_heading {
            current_headings.push(json!({
                "level": event.heading_level,
                "text": event.text,
            }));
            if current_section_heading_level.is_none() {
                current_section_heading_level = event.heading_level;
            }
        }
        if event.is_list {
            current_list_items += 1;
        }
        if event.is_table {
            current_table_count += 1;
        }
        current_paragraphs.push(event.text);
        paragraph_count += 1;

        let boundary_signal = match event.signal {
            PageBreakSignal::Explicit => Some(PageBreakSignal::Explicit),
            PageBreakSignal::Section => Some(PageBreakSignal::Section),
            PageBreakSignal::Rendered => Some(PageBreakSignal::Rendered),
            PageBreakSignal::None if paragraph_count >= paragraphs_per_page => {
                Some(PageBreakSignal::None)
            }
            PageBreakSignal::None => None,
        };

        if let Some(signal) = boundary_signal {
            chunks.extend(build_page_chunk(
                &current_paragraphs,
                page_number,
                signal,
                paragraph_count,
                &current_headings,
                current_section_heading_level,
                current_list_items,
                current_table_count,
            ));
            page_number += 1;
            current_paragraphs.clear();
            current_headings.clear();
            current_section_heading_level = None;
            current_list_items = 0;
            current_table_count = 0;
            paragraph_count = 0;
        }
    }

    if !current_paragraphs.is_empty() {
        chunks.extend(build_page_chunk(
            &current_paragraphs,
            page_number,
            PageBreakSignal::None,
            paragraph_count,
            &current_headings,
            current_section_heading_level,
            current_list_items,
            current_table_count,
        ));
    }

    chunks
}

fn build_page_chunk(
    paragraphs: &[String],
    page_number: usize,
    break_type: PageBreakSignal,
    paragraph_count: usize,
    headings: &[Value],
    section_heading_level: Option<u32>,
    list_item_count: usize,
    table_count: usize,
) -> Vec<ChunkRecordInput> {
    let content = paragraphs.join("\n\n");
    let pieces = split_page_content(&content, MAX_PAGE_AWARE_CHUNK_CHARS);
    let total = pieces.len();

    pieces
        .into_iter()
        .enumerate()
        .map(|(idx, piece)| {
            let mut metadata = json!({
                "page_number": page_number,
                "page_break_type": page_break_signal_str(break_type),
                "paragraph_count": paragraph_count,
                "headings": headings,
                "section_heading_level": section_heading_level,
                "list_item_count": list_item_count,
                "table_count": table_count,
                "document_metadata": {
                    "source_type": "docx"
                }
            });

            if total > 1 {
                if let Some(meta_obj) = metadata.as_object_mut() {
                    meta_obj.insert("chunk_index".to_string(), json!(idx + 1));
                    meta_obj.insert("total_chunks".to_string(), json!(total));
                }
            }

            ChunkRecordInput {
                content: piece,
                metadata,
            }
        })
        .collect()
}

fn split_page_content(text: &str, max_chars: usize) -> Vec<String> {
    if text.len() <= max_chars {
        return vec![text.to_string()];
    }

    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();

    for paragraph in text.split("\n\n") {
        let paragraph = paragraph.trim();
        if paragraph.is_empty() {
            continue;
        }

        if paragraph.len() > max_chars {
            if !current.trim().is_empty() {
                out.push(current.trim().to_string());
                current.clear();
            }
            out.extend(split_long_text(paragraph, max_chars));
            continue;
        }

        let candidate = if current.is_empty() {
            paragraph.to_string()
        } else {
            format!("{}\n\n{}", current, paragraph)
        };

        if candidate.len() > max_chars {
            if !current.trim().is_empty() {
                out.push(current.trim().to_string());
            }
            current = paragraph.to_string();
        } else {
            current = candidate;
        }
    }

    if !current.trim().is_empty() {
        out.push(current.trim().to_string());
    }

    if out.is_empty() {
        vec![text.to_string()]
    } else {
        out
    }
}

fn split_long_text(text: &str, max_chars: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();

    for ch in text.chars() {
        if !current.is_empty() && current.len() + ch.len_utf8() > max_chars {
            out.push(current.trim().to_string());
            current.clear();
        }
        current.push(ch);
    }

    if !current.trim().is_empty() {
        out.push(current.trim().to_string());
    }

    if out.is_empty() {
        vec![text.to_string()]
    } else {
        out
    }
}

#[pyclass]
pub struct DocxPageAwareIterator {
    chunks: Vec<ChunkRecordInput>,
    index: usize,
}

#[pymethods]
impl DocxPageAwareIterator {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(&mut self, py: Python<'_>) -> PyResult<Option<PyObject>> {
        if self.index >= self.chunks.len() {
            return Ok(None);
        }
        let chunk = &self.chunks[self.index];
        self.index += 1;
        let dict = PyDict::new_bound(py);
        dict.set_item("content", &chunk.content)?;
        dict.set_item("content_type", "page_aware")?;
        dict.set_item("metadata", pythonize(py, &chunk.metadata)?)?;
        Ok(Some(dict.into_any().unbind()))
    }
}

#[pyfunction]
fn chunk_docx_page_aware_stream(
    file_path: &str,
    paragraphs_per_page: usize,
) -> PyResult<DocxPageAwareIterator> {
    if !crate::extensions::docx::common::is_word_ooxml(file_path) {
        return Err(PyValueError::new_err(format!(
            "Expected a Word OOXML file ({}), got: {file_path}",
            crate::extensions::docx::common::word_exts_display()
        )));
    }
    if paragraphs_per_page == 0 {
        return Err(PyValueError::new_err(
            "paragraphs_per_page must be greater than 0",
        ));
    }

    let bytes = fs::read(file_path)
        .map_err(|e| PyIOError::new_err(format!("Failed to read DOCX file: {e}")))?;

    let events = parse_docx_paragraph_events(&bytes)
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to parse DOCX: {e}")))?;
    let chunks = build_page_aware_chunks(events, paragraphs_per_page);

    Ok(DocxPageAwareIterator { chunks, index: 0 })
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(chunk_docx_page_aware, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_docx_page_aware_with_images, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_docx_page_aware_stream, m)?)?;
    m.add_class::<DocxPageAwareIterator>()?;
    Ok(())
}
