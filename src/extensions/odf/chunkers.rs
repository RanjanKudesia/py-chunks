//! `.odt` / `.odp` chunking + markdown + image pyfunctions.
//! Pipeline mirrors `.eml`/`.msg`: ODF → assembled markdown → Markdown chunker.

use std::time::Instant;

use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyModule};
use pyo3::wrap_pyfunction;
use pythonize::pythonize;
use serde_json::json;

use super::container::{load, parse_meta, OdfKind};
use super::text::content_to_markdown;
use crate::extensions::md::common::ChunkRecordInput;
use crate::extensions::md::page_aware::build_page_aware_chunks;
use crate::extensions::md::section::build_section_chunks;
use crate::extensions::md::semantic::build_semantic_chunks;
use crate::extensions::md::sentence::build_sentence_chunks;
use crate::extensions::md::sliding_window::build_sliding_window_chunks;
use crate::extensions::md::structural::build_chunks_from_md_bytes;

fn kind_for(file_path: &str) -> PyResult<OdfKind> {
    let lower = file_path.to_ascii_lowercase();
    if lower.ends_with(".odt") {
        Ok(OdfKind::Text)
    } else if lower.ends_with(".odp") {
        Ok(OdfKind::Presentation)
    } else {
        Err(PyValueError::new_err(format!(
            "Expected .odt or .odp file path, got: {file_path}"
        )))
    }
}

struct Loaded {
    markdown: String,
    images: Vec<(String, Vec<u8>)>,
    metadata: serde_json::Value,
}

fn load_odf(file_path: &str) -> PyResult<Loaded> {
    let kind = kind_for(file_path)?;
    let bytes = std::fs::read(file_path)
        .map_err(|e| PyIOError::new_err(format!("Failed to read ODF file: {e}")))?;
    let container = load(&bytes, kind).map_err(PyRuntimeError::new_err)?;
    let (markdown, slide_count) = content_to_markdown(&container.content_xml, kind, &container.image_names);
    let (title, creator) = container
        .meta_xml
        .as_deref()
        .map(parse_meta)
        .unwrap_or((None, None));

    let metadata = match kind {
        OdfKind::Text => json!({
            "source_type": "odt",
            "title": title,
            "creator": creator,
        }),
        OdfKind::Presentation => json!({
            "source_type": "odp",
            "title": title,
            "creator": creator,
            "slide_count": slide_count,
        }),
    };
    Ok(Loaded { markdown, images: container.images, metadata })
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
        other => Err(format!("Unknown ODF mode: {other}")),
    }
}

fn inject_metadata(records: &mut [ChunkRecordInput], meta: &serde_json::Value) {
    for rec in records.iter_mut() {
        if let serde_json::Value::Object(map) = &mut rec.metadata {
            map.insert("document_metadata".to_string(), meta.clone());
        }
    }
}

/// Give `.odp` chunks the slide identity `.pptx` chunks already carry.
///
/// Slide identity existed only as an unstructured `## Slide N` markdown
/// heading, so a consumer had to string-parse `section_heading` to answer
/// "which slide is this?" — while the same question on a `.pptx` is a metadata
/// lookup. (#52)
///
/// The title is the slide's first line of text, which is what a reader would
/// call its title; `.odp` has no title element the way `.pptx` does.
fn inject_slide_metadata(chunks: &mut [ChunkRecordInput]) {
    let mut slide: Option<u64> = None;
    let mut title: Option<String> = None;
    let mut titles: std::collections::HashMap<u64, String> = std::collections::HashMap::new();

    for chunk in chunks.iter_mut() {
        let heading_here = slide_number_of(chunk.content.trim());
        let heading_ctx = chunk
            .metadata
            .get("section_heading")
            .and_then(|v| v.as_str())
            .and_then(slide_number_of);

        if let Some(n) = heading_here {
            // The `## Slide N` heading chunk itself starts a new slide.
            slide = Some(n);
            title = None;
        } else if let Some(n) = heading_ctx {
            if slide != Some(n) {
                slide = Some(n);
                title = None;
            }
            if title.is_none() {
                title = chunk
                    .content
                    .lines()
                    .map(str::trim)
                    .find(|l| !l.is_empty())
                    .map(str::to_string);
            }
        }

        if let Some(n) = slide {
            if let Some(t) = &title {
                titles.entry(n).or_insert_with(|| t.clone());
            }
            if let Some(map) = chunk.metadata.as_object_mut() {
                map.insert("slide_number".into(), serde_json::json!(n));
            }
        }
    }

    // Second pass: the `## Slide N` heading chunk is emitted before the slide's
    // first line of text, so its title is not known yet on the way through.
    // Backfill it, rather than leave one chunk per slide with a null title
    // while its siblings have one.
    for chunk in chunks.iter_mut() {
        let Some(n) = chunk
            .metadata
            .get("slide_number")
            .and_then(|v| v.as_u64())
        else {
            continue;
        };
        let t = titles.get(&n).cloned();
        if let Some(map) = chunk.metadata.as_object_mut() {
            map.insert("slide_title".into(), serde_json::json!(t));
        }
    }
}

/// `"Slide 4"` -> `Some(4)`.
fn slide_number_of(text: &str) -> Option<u64> {
    text.strip_prefix("Slide ")?.trim().parse().ok()
}

fn record_to_pydict(py: Python<'_>, rec: &ChunkRecordInput) -> PyResult<PyObject> {
    let dict = PyDict::new_bound(py);
    dict.set_item("content", &rec.content)?;
    dict.set_item("content_type", rec.content_type.as_str())?;
    dict.set_item("metadata", pythonize(py, &rec.metadata)?)?;
    Ok(dict.into_any().unbind())
}

fn build_records(
    loaded: &Loaded,
    mode: &str,
    window_size: usize,
    overlap: usize,
    sentences_per_chunk: usize,
    paragraphs_per_page: usize,
) -> PyResult<Vec<ChunkRecordInput>> {
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
    // `.odt` has no slides, so this is a no-op there. (#52)
    if loaded.metadata.get("source_type").and_then(|v| v.as_str()) == Some("odp") {
        inject_slide_metadata(&mut records);
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
    let start = Instant::now();
    let loaded = load_odf(file_path)?;
    let records = build_records(
        &loaded, mode, window_size, overlap, sentences_per_chunk, paragraphs_per_page,
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
fn chunk_odf(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    run_mode(py, file_path, "default", 3, 1, 3, 15)
}
#[pyfunction]
fn chunk_odf_section(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    run_mode(py, file_path, "section", 3, 1, 3, 15)
}
#[pyfunction]
fn chunk_odf_semantic(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    run_mode(py, file_path, "semantic", 3, 1, 3, 15)
}
#[pyfunction]
fn chunk_odf_sentence(py: Python<'_>, file_path: &str, sentences_per_chunk: usize) -> PyResult<PyObject> {
    run_mode(py, file_path, "sentence", 3, 1, sentences_per_chunk, 15)
}
#[pyfunction]
fn chunk_odf_page_aware(py: Python<'_>, file_path: &str, paragraphs_per_page: usize) -> PyResult<PyObject> {
    run_mode(py, file_path, "page_aware", 3, 1, 3, paragraphs_per_page)
}
#[pyfunction]
fn chunk_odf_sliding_window(py: Python<'_>, file_path: &str, window_size: usize, overlap: usize) -> PyResult<PyObject> {
    run_mode(py, file_path, "sliding_window", window_size, overlap, 3, 15)
}

#[pyfunction]
fn odf_to_markdown(file_path: &str) -> PyResult<String> {
    Ok(load_odf(file_path)?.markdown)
}

#[pyfunction]
fn odf_to_markdown_with_images(
    py: Python<'_>,
    file_path: &str,
) -> PyResult<(String, Vec<(String, Py<PyBytes>)>)> {
    let loaded = load_odf(file_path)?;
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
fn chunk_odf_with_images(
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
    let _ = (rows_per_chunk, max_chunk_chars);
    let loaded = load_odf(file_path)?;
    let records = build_records(
        &loaded, mode, window_size, overlap, sentences_per_chunk, paragraphs_per_page,
    )?;

    let mut chunk_list: Vec<PyObject> = loaded
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
        .images
        .iter()
        .map(|(name, bytes)| (name.clone(), PyBytes::new_bound(py, bytes).unbind()))
        .collect();
    Ok((chunk_list, images))
}

#[pyclass]
pub struct OdfStreamIterator {
    chunks: std::vec::IntoIter<PyObject>,
}

#[pymethods]
impl OdfStreamIterator {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }
    fn __next__(&mut self, _py: Python<'_>) -> Option<PyObject> {
        self.chunks.next()
    }
}

#[pyfunction]
#[pyo3(signature = (file_path, mode, window_size, overlap, sentences_per_chunk, paragraphs_per_page))]
fn stream_odf_chunks(
    py: Python<'_>,
    file_path: &str,
    mode: &str,
    window_size: usize,
    overlap: usize,
    sentences_per_chunk: usize,
    paragraphs_per_page: usize,
) -> PyResult<Py<OdfStreamIterator>> {
    let loaded = load_odf(file_path)?;
    let records = build_records(
        &loaded, mode, window_size, overlap, sentences_per_chunk, paragraphs_per_page,
    )?;
    let chunk_vec: Vec<PyObject> = records
        .iter()
        .map(|r| record_to_pydict(py, r))
        .collect::<PyResult<_>>()?;
    Py::new(py, OdfStreamIterator { chunks: chunk_vec.into_iter() })
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(chunk_odf, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_odf_section, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_odf_semantic, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_odf_sentence, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_odf_page_aware, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_odf_sliding_window, m)?)?;
    m.add_function(wrap_pyfunction!(odf_to_markdown, m)?)?;
    m.add_function(wrap_pyfunction!(odf_to_markdown_with_images, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_odf_with_images, m)?)?;
    m.add_function(wrap_pyfunction!(stream_odf_chunks, m)?)?;
    Ok(())
}
