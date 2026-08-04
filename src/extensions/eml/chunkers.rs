//! `.eml` / `.mbox` chunking + markdown + image pyfunctions.
//!
//! Pipeline mirrors `.msg`: email → assembled markdown → the Markdown chunker.
//! One code path serves both extensions; `.mbox` is a multi-message document.

use std::time::Instant;

use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyModule};
use pyo3::wrap_pyfunction;
use pythonize::pythonize;
use serde_json::json;

use super::extract::{document_to_markdown, parse_message_bytes};
use super::mbox::{mbox_to_markdown, MboxMessageInfo};
use crate::extensions::md::common::ChunkRecordInput;
use crate::extensions::md::page_aware::build_page_aware_chunks;
use crate::extensions::md::section::build_section_chunks;
use crate::extensions::md::semantic::build_semantic_chunks;
use crate::extensions::md::sentence::build_sentence_chunks;
use crate::extensions::md::sliding_window::build_sliding_window_chunks;
use crate::extensions::md::structural::build_chunks_from_md_bytes;

fn ensure_email(file_path: &str) -> PyResult<()> {
    let lower = file_path.to_ascii_lowercase();
    if lower.ends_with(".eml") || lower.ends_with(".mbox") {
        Ok(())
    } else {
        Err(PyValueError::new_err(format!(
            "Expected .eml or .mbox file path, got: {file_path}"
        )))
    }
}

fn is_mbox(file_path: &str) -> bool {
    file_path.to_ascii_lowercase().ends_with(".mbox")
}

struct Loaded {
    markdown: String,
    images: Vec<(String, Vec<u8>)>,
    metadata: serde_json::Value,
}

/// Read the file and assemble markdown + images + document metadata, dispatching
/// on `.eml` vs `.mbox`.
fn load_email(file_path: &str) -> PyResult<(Loaded, Vec<MboxMessageInfo>)> {
    let raw = std::fs::read(file_path)
        .map_err(|e| PyIOError::new_err(format!("Failed to read email file: {e}")))?;

    if is_mbox(file_path) {
        let (markdown, images, count, infos) = mbox_to_markdown(&raw);
        let metadata = json!({
            "source_type": "mbox",
            "message_count": count,
        });
        Ok((Loaded { markdown, images, metadata }, infos))
    } else {
        let doc = parse_message_bytes(&raw);
        let markdown = document_to_markdown(&doc, 1);
        let metadata = json!({
            "source_type": "eml",
            "subject": doc.subject,
            "from": doc.from,
            "to": doc.to,
            "cc": doc.cc,
            "bcc": doc.bcc,
            "date": doc.date,
            "message_id": doc.message_id,
            "in_reply_to": doc.in_reply_to,
            "references": doc.references,
            "has_attachments": !doc.attachments.is_empty(),
            "attachment_count": doc.attachments.len(),
        });
        Ok((Loaded { markdown, images: doc.images, metadata }, Vec::new()))
    }
}

fn chunks_for_mode(
    markdown: &[u8],
    mode: &str,
    window_size: usize,
    overlap: usize,
    sentences_per_chunk: usize,
    paragraphs_per_page: usize,
) -> Result<Vec<ChunkRecordInput>, String> {
    match mode {
        "default" | "structural" => build_chunks_from_md_bytes(markdown),
        "section" => build_section_chunks(markdown),
        "semantic" => build_semantic_chunks(markdown),
        "sentence" => build_sentence_chunks(markdown, sentences_per_chunk),
        "page_aware" => build_page_aware_chunks(markdown, paragraphs_per_page),
        "sliding_window" => build_sliding_window_chunks(markdown, window_size, overlap),
        other => Err(format!("Unknown email mode: {other}")),
    }
}

fn inject_metadata(records: &mut [ChunkRecordInput], meta: &serde_json::Value) {
    for rec in records.iter_mut() {
        if let serde_json::Value::Object(map) = &mut rec.metadata {
            map.insert("document_metadata".to_string(), meta.clone());
        }
    }
}

fn record_to_pydict(py: Python<'_>, rec: &ChunkRecordInput) -> PyResult<PyObject> {
    let dict = PyDict::new_bound(py);
    dict.set_item("content", &rec.content)?;
    dict.set_item("content_type", rec.content_type.as_str())?;
    dict.set_item("metadata", pythonize(py, &rec.metadata)?)?;
    Ok(dict.into_any().unbind())
}

/// Give each `.mbox` chunk the identity of the message it came from.
///
/// Same shape as the `.odp` slide pass: the `## Message N` heading marks the
/// boundary. A 152-message mailbox used to give every chunk the same
/// `{source_type, message_count}` — the per-message envelope was parsed and
/// thrown away, so "which message is this?" was unanswerable. (#36)
fn inject_message_metadata(chunks: &mut [ChunkRecordInput], infos: &[MboxMessageInfo]) {
    let by_index: std::collections::HashMap<u64, &MboxMessageInfo> =
        infos.iter().map(|i| (i.index as u64, i)).collect();
    let mut current: Option<u64> = None;

    for chunk in chunks.iter_mut() {
        if let Some(n) = message_number_of(chunk.content.trim()) {
            current = Some(n);
        } else if let Some(n) = chunk
            .metadata
            .get("section_heading")
            .and_then(|v| v.as_str())
            .and_then(message_number_of)
        {
            current = Some(n);
        }
        let Some(n) = current else { continue };
        let Some(info) = by_index.get(&n) else { continue };
        if let Some(map) = chunk.metadata.as_object_mut() {
            map.insert("message_index".into(), serde_json::json!(n));
            map.insert("message_subject".into(), serde_json::json!(info.subject));
            map.insert("message_from".into(), serde_json::json!(info.from));
            map.insert("message_date".into(), serde_json::json!(info.date));
            map.insert("message_id".into(), serde_json::json!(info.message_id));
            map.insert("in_reply_to".into(), serde_json::json!(info.in_reply_to));
            map.insert("references".into(), serde_json::json!(info.references));
        }
    }
}

/// `"Message 7"` -> `Some(7)`.
fn message_number_of(text: &str) -> Option<u64> {
    text.strip_prefix("Message ")?.trim().parse().ok()
}

fn build_records(
    loaded_and_infos: &(Loaded, Vec<MboxMessageInfo>),
    mode: &str,
    window_size: usize,
    overlap: usize,
    sentences_per_chunk: usize,
    paragraphs_per_page: usize,
) -> PyResult<Vec<ChunkRecordInput>> {
    // A genuinely empty message (e.g. an adversarial mail with empty MIME parts
    // and no headers) yields no markdown. Return zero chunks rather than erroring,
    // matching how the library treats other empty documents (e.g. image-only
    // notebooks) — and keeping batch/stream/bytes consistent.
    let (loaded, infos) = loaded_and_infos;
    if loaded.markdown.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut records = chunks_for_mode(
        loaded.markdown.as_bytes(),
        mode,
        window_size,
        overlap,
        sentences_per_chunk,
        paragraphs_per_page,
    )
    .map_err(PyRuntimeError::new_err)?;
    inject_metadata(&mut records, &loaded.metadata);
    if !infos.is_empty() {
        inject_message_metadata(&mut records, infos);
    }
    Ok(records)
}

fn run_mode(
    py: Python<'_>,
    file_path: &str,
    mode: &str,
    window_size: usize,
    overlap: usize,
    sentences_per_chunk: usize,
    paragraphs_per_page: usize,
) -> PyResult<PyObject> {
    ensure_email(file_path)?;
    let start = Instant::now();
    let loaded = load_email(file_path)?;
    let records = build_records(
        &loaded,
        mode,
        window_size,
        overlap,
        sentences_per_chunk,
        paragraphs_per_page,
    )?;
    let rust_ms = start.elapsed().as_secs_f64() * 1000.0;

    let chunk_list: Vec<PyObject> = records
        .iter()
        .map(|r| record_to_pydict(py, r))
        .collect::<PyResult<_>>()?;
    let result = PyDict::new_bound(py);
    result.set_item("chunks", chunk_list)?;
    result.set_item("rust_ms", (rust_ms * 1000.0).round() / 1000.0)?;
    Ok(result.into_any().unbind())
}

#[pyfunction]
fn chunk_eml(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    run_mode(py, file_path, "default", 3, 1, 3, 15)
}
#[pyfunction]
fn chunk_eml_section(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    run_mode(py, file_path, "section", 3, 1, 3, 15)
}
#[pyfunction]
fn chunk_eml_semantic(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    run_mode(py, file_path, "semantic", 3, 1, 3, 15)
}
#[pyfunction]
fn chunk_eml_sentence(py: Python<'_>, file_path: &str, sentences_per_chunk: usize) -> PyResult<PyObject> {
    run_mode(py, file_path, "sentence", 3, 1, sentences_per_chunk, 15)
}
#[pyfunction]
fn chunk_eml_page_aware(py: Python<'_>, file_path: &str, paragraphs_per_page: usize) -> PyResult<PyObject> {
    run_mode(py, file_path, "page_aware", 3, 1, 3, paragraphs_per_page)
}
#[pyfunction]
fn chunk_eml_sliding_window(py: Python<'_>, file_path: &str, window_size: usize, overlap: usize) -> PyResult<PyObject> {
    run_mode(py, file_path, "sliding_window", window_size, overlap, 3, 15)
}

#[pyfunction]
fn eml_to_markdown(file_path: &str) -> PyResult<String> {
    ensure_email(file_path)?;
    Ok(load_email(file_path)?.0.markdown)
}

#[pyfunction]
fn eml_to_markdown_with_images(
    py: Python<'_>,
    file_path: &str,
) -> PyResult<(String, Vec<(String, Py<PyBytes>)>)> {
    ensure_email(file_path)?;
    let loaded = load_email(file_path)?.0;
    let images = loaded
        .images
        .iter()
        .map(|(name, bytes)| (name.clone(), PyBytes::new_bound(py, bytes).unbind()))
        .collect();
    Ok((loaded.markdown, images))
}

#[pyfunction]
#[pyo3(signature = (file_path, mode, rows_per_chunk=1, window_size=3, overlap=1, sentences_per_chunk=3, paragraphs_per_page=15, max_chunk_chars=2000))]
#[allow(clippy::too_many_arguments)]
fn chunk_eml_with_images(
    py: Python<'_>,
    file_path: &str,
    mode: &str,
    rows_per_chunk: usize,
    window_size: usize,
    overlap: usize,
    sentences_per_chunk: usize,
    paragraphs_per_page: usize,
    max_chunk_chars: usize,
) -> PyResult<(Vec<PyObject>, Vec<(String, Py<PyBytes>)>)> {
    ensure_email(file_path)?;
    let _ = (rows_per_chunk, max_chunk_chars);
    let loaded = load_email(file_path)?;
    let records = build_records(
        &loaded,
        mode,
        window_size,
        overlap,
        sentences_per_chunk,
        paragraphs_per_page,
    )?;

    // Image chunks first, then text chunks — matching the other formats.
    let mut chunk_list: Vec<PyObject> = loaded
        .0
        .images
        .iter()
        .map(|(name, _)| {
            let dict = PyDict::new_bound(py);
            dict.set_item("content", name)?;
            dict.set_item("content_type", "image")?;
            let meta = PyDict::new_bound(py);
            meta.set_item("image_name", name)?;
            dict.set_item("metadata", meta)?;
            Ok(dict.into_any().unbind())
        })
        .collect::<PyResult<_>>()?;
    for rec in &records {
        chunk_list.push(record_to_pydict(py, rec)?);
    }

    let images = loaded
        .0
        .images
        .iter()
        .map(|(name, bytes)| (name.clone(), PyBytes::new_bound(py, bytes).unbind()))
        .collect();
    Ok((chunk_list, images))
}

#[pyclass]
pub struct EmlStreamIterator {
    chunks: std::vec::IntoIter<PyObject>,
}

#[pymethods]
impl EmlStreamIterator {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }
    fn __next__(&mut self, _py: Python<'_>) -> Option<PyObject> {
        self.chunks.next()
    }
}

#[pyfunction]
#[pyo3(signature = (file_path, mode, window_size, overlap, sentences_per_chunk, paragraphs_per_page))]
fn stream_eml_chunks(
    py: Python<'_>,
    file_path: &str,
    mode: &str,
    window_size: usize,
    overlap: usize,
    sentences_per_chunk: usize,
    paragraphs_per_page: usize,
) -> PyResult<Py<EmlStreamIterator>> {
    ensure_email(file_path)?;
    let loaded = load_email(file_path)?;
    let records = build_records(
        &loaded,
        mode,
        window_size,
        overlap,
        sentences_per_chunk,
        paragraphs_per_page,
    )?;
    let chunk_vec: Vec<PyObject> = records
        .iter()
        .map(|r| record_to_pydict(py, r))
        .collect::<PyResult<_>>()?;
    Py::new(py, EmlStreamIterator { chunks: chunk_vec.into_iter() })
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(chunk_eml, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_eml_section, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_eml_semantic, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_eml_sentence, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_eml_page_aware, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_eml_sliding_window, m)?)?;
    m.add_function(wrap_pyfunction!(eml_to_markdown, m)?)?;
    m.add_function(wrap_pyfunction!(eml_to_markdown_with_images, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_eml_with_images, m)?)?;
    m.add_function(wrap_pyfunction!(stream_eml_chunks, m)?)?;
    Ok(())
}
