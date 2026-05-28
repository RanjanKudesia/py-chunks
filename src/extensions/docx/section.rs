use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyModule};
use pyo3::wrap_pyfunction;
use pythonize::pythonize;
use serde_json::{json, Value};

use super::common::{
    image_hash_name, image_placeholder, parse_docx_blocks, parse_rels_xml_images, DocxBlock,
    DocxBlockKind,
};
use std::collections::HashMap;
use std::fs;
use std::io::{Cursor, Read};
use std::time::Instant;
use zip::ZipArchive;

const MAX_SECTION_CHARS: usize = 2000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockType {
    Paragraph,
    BulletList,
    Table,
    Image,
}

#[derive(Debug, Clone)]
struct DocumentBlock {
    block_type: BlockType,
    text: String,
    heading_level: Option<u32>,
    image_rid: Option<String>,
}

#[derive(Debug, Clone)]
struct SectionState {
    heading: String,
    level: u32,
    path: Vec<String>,
    lines: Vec<String>,
}

#[derive(Debug, Clone)]
struct ChunkRecordInput {
    content: String,
    metadata: Value,
}

#[pyfunction]
fn chunk_docx_section(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    if !file_path.to_ascii_lowercase().ends_with(".docx") {
        return Err(PyValueError::new_err(format!(
            "Expected .docx file path, got: {file_path}"
        )));
    }

    let bytes = fs::read(file_path)
        .map_err(|e| PyIOError::new_err(format!("Failed to read DOCX file: {e}")))?;

    let rust_start = Instant::now();
    let raw_blocks = parse_docx_blocks(&bytes)
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to parse DOCX: {e}")))?;
    let blocks = lower_blocks(raw_blocks);
    let chunks_raw = build_section_chunks(blocks);
    let rust_ms = rust_start.elapsed().as_secs_f64() * 1000.0;

    let chunk_list: Vec<PyObject> = chunks_raw
        .into_iter()
        .map(|c| {
            let dict = PyDict::new_bound(py);
            dict.set_item("content", &c.content)?;
            dict.set_item("content_type", "section")?;
            dict.set_item("metadata", pythonize(py, &c.metadata)?)?;
            Ok(dict.into_any().unbind())
        })
        .collect::<PyResult<_>>()?;

    let result = PyDict::new_bound(py);
    result.set_item("chunks", chunk_list)?;
    result.set_item("rust_ms", (rust_ms * 1000.0).round() / 1000.0)?;
    Ok(result.into_any().unbind())
}

fn build_section_chunks_with_images(
    blocks: Vec<DocumentBlock>,
    image_rids_map: &HashMap<String, String>,
    archive: &mut ZipArchive<Cursor<Vec<u8>>>,
    image_out: &mut Vec<(String, Vec<u8>)>,
) -> Vec<(String, String, serde_json::Value)> {
    let mut result: Vec<(String, String, serde_json::Value)> = Vec::new();
    let section_chunks = build_section_chunks(blocks.clone());

    let mut image_results: Vec<(String, String, serde_json::Value)> = Vec::new();
    for block in &blocks {
        if block.block_type != BlockType::Image {
            continue;
        }
        if let Some(rid) = &block.image_rid {
            if let Some(zip_path) = image_rids_map.get(rid) {
                if let Ok(mut entry) = archive.by_name(zip_path) {
                    let mut bytes = Vec::new();
                    if entry.read_to_end(&mut bytes).is_ok() {
                        if let Some(hash_name) = image_hash_name(&bytes, zip_path) {
                            if !image_out.iter().any(|(n, _)| n == &hash_name) {
                                image_out.push((hash_name.clone(), bytes));
                            }
                            let alt = block
                                .text
                                .strip_prefix("[Image: ")
                                .and_then(|s| s.strip_suffix(']'))
                                .unwrap_or("");
                            image_results.push((
                                "image".to_string(),
                                hash_name.clone(),
                                json!({ "image_name": hash_name, "alt_text": alt }),
                            ));
                        }
                    }
                }
            }
        }
    }

    result.extend(image_results);
    for chunk in section_chunks {
        result.push(("section".to_string(), chunk.content, chunk.metadata));
    }
    result
}

#[pyfunction]
fn chunk_docx_section_with_images(
    py: Python<'_>,
    file_path: &str,
) -> PyResult<(Vec<PyObject>, Vec<(String, Py<PyBytes>)>)> {
    if !file_path.to_ascii_lowercase().ends_with(".docx") {
        return Err(PyValueError::new_err(format!(
            "Expected .docx file path, got: {file_path}"
        )));
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

    let raw_blocks = parse_docx_blocks(&bytes)
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to parse DOCX: {e}")))?;
    let blocks = lower_blocks(raw_blocks);

    let mut image_out: Vec<(String, Vec<u8>)> = Vec::new();
    let combined =
        build_section_chunks_with_images(blocks, &image_rids_map, &mut archive, &mut image_out);

    let chunk_list: Vec<PyObject> = combined
        .iter()
        .map(|(content_type, content, metadata)| {
            let dict = PyDict::new_bound(py);
            dict.set_item("content", content)?;
            dict.set_item("content_type", content_type)?;
            dict.set_item("metadata", pythonize(py, metadata)?)?;
            Ok(dict.into_any().unbind())
        })
        .collect::<PyResult<_>>()?;

    let image_out_py: Vec<(String, Py<PyBytes>)> = image_out
        .into_iter()
        .map(|(name, data)| (name, PyBytes::new_bound(py, &data).unbind()))
        .collect();

    Ok((chunk_list, image_out_py))
}

fn lower_blocks(raw: Vec<DocxBlock>) -> Vec<DocumentBlock> {
    let mut out: Vec<DocumentBlock> = Vec::with_capacity(raw.len());

    for block in raw {
        match block.kind {
            DocxBlockKind::Table => {
                let table_text = block.text.trim().to_string();
                if !table_text.is_empty() {
                    out.push(DocumentBlock {
                        block_type: BlockType::Table,
                        text: table_text,
                        heading_level: None,
                        image_rid: None,
                    });
                }
            }
            DocxBlockKind::Paragraph => {
                let text = block.text.trim().to_string();
                let has_text = !text.is_empty();

                if !has_text && !block.has_drawing {
                    continue;
                }

                let heading_level =
                    parse_heading_level(block.heading_style.as_deref(), block.outline_level);

                let normalized = if has_text {
                    text
                } else {
                    image_placeholder(block.image_alt.as_deref())
                };

                let block_type = if heading_level.is_some() {
                    BlockType::Paragraph
                } else if block.is_list {
                    BlockType::BulletList
                } else if block.has_drawing {
                    BlockType::Image
                } else {
                    BlockType::Paragraph
                };

                out.push(DocumentBlock {
                    block_type,
                    text: normalized,
                    heading_level,
                    image_rid: if block.has_drawing {
                        block.image_rid.clone()
                    } else {
                        None
                    },
                });
            }
        }
    }

    out
}

fn parse_heading_level(style_val: Option<&str>, outline_level: Option<u32>) -> Option<u32> {
    if let Some(style) = style_val {
        let style_trimmed = style.trim();
        let style_lc = style_trimmed.to_ascii_lowercase();

        if style_lc.starts_with("heading") {
            let level_part = style_trimmed
                .chars()
                .skip_while(|c| !c.is_ascii_digit())
                .collect::<String>();

            if level_part.is_empty() {
                // Malformed heading style data is treated as non-heading.
                return None;
            }

            if let Ok(level) = level_part.parse::<u32>() {
                if level > 0 {
                    return Some(level);
                }
            }

            // Malformed heading style data is treated as non-heading.
            return None;
        }

        // Not a heading style: fallback to outline level when available.
        if let Some(outline) = outline_level {
            return Some(outline.saturating_add(1));
        }
        return None;
    }

    outline_level.map(|v| v.saturating_add(1))
}
fn build_section_chunks(blocks: Vec<DocumentBlock>) -> Vec<ChunkRecordInput> {
    let mut chunks = Vec::new();
    let mut preamble_lines: Vec<String> = Vec::new();
    let mut sections: Vec<SectionState> = Vec::new();
    let mut seen_heading = false;

    for block in blocks {
        if let Some(level) = block.heading_level {
            seen_heading = true;

            while let Some(last) = sections.last() {
                if last.level >= level {
                    let closing = sections.pop().expect("section stack not empty");
                    emit_section_chunks(
                        &mut chunks,
                        &closing.heading,
                        closing.level,
                        &closing.path,
                        &closing.lines,
                    );
                } else {
                    break;
                }
            }

            let heading = block.text.clone();
            let mut path = if let Some(parent) = sections.last() {
                parent.path.clone()
            } else {
                Vec::new()
            };
            path.push(heading.clone());

            sections.push(SectionState {
                heading: heading.clone(),
                level,
                path,
                lines: vec![heading],
            });
            continue;
        }

        let line = normalize_block_line(&block);
        if line.is_empty() {
            continue;
        }

        if let Some(current) = sections.last_mut() {
            current.lines.push(line);
        } else if !seen_heading {
            preamble_lines.push(line);
        }
    }

    if !preamble_lines.is_empty() {
        let preamble_path = vec!["Preamble".to_string()];
        emit_section_chunks(&mut chunks, "Preamble", 0, &preamble_path, &preamble_lines);
    }

    for section in sections {
        emit_section_chunks(
            &mut chunks,
            &section.heading,
            section.level,
            &section.path,
            &section.lines,
        );
    }

    chunks
}

fn normalize_block_line(block: &DocumentBlock) -> String {
    match block.block_type {
        BlockType::Table | BlockType::BulletList => block.text.trim().to_string(),
        BlockType::Paragraph | BlockType::Image => block.text.trim().to_string(),
    }
}

fn emit_section_chunks(
    out: &mut Vec<ChunkRecordInput>,
    heading: &str,
    level: u32,
    path: &[String],
    lines: &[String],
) {
    let full_content = lines.join("\n").trim().to_string();
    if full_content.is_empty() {
        return;
    }

    let splits = split_text_by_max_chars(&full_content, MAX_SECTION_CHARS);
    let total = splits.len();

    for (idx, content) in splits.into_iter().enumerate() {
        let mut metadata = json!({
            "section_heading": heading,
            "section_level": level,
            "section_heading_level": level,
            "heading_path": path.join(" > "),
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

        out.push(ChunkRecordInput { content, metadata });
    }
}

fn split_text_by_max_chars(text: &str, max_chars: usize) -> Vec<String> {
    if text.len() <= max_chars {
        return vec![text.to_string()];
    }

    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();

    for line in text.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }

        if line.len() > max_chars {
            if !current.trim().is_empty() {
                out.push(current.trim().to_string());
                current.clear();
            }

            let mut start = 0usize;
            while start < line.len() {
                let end = (start + max_chars).min(line.len());
                out.push(line[start..end].trim().to_string());
                start = end;
            }
            continue;
        }

        let candidate = if current.is_empty() {
            line.to_string()
        } else {
            format!("{}\n{}", current, line)
        };

        if candidate.len() > max_chars {
            if !current.trim().is_empty() {
                out.push(current.trim().to_string());
            }
            current = line.to_string();
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

#[pyclass]
pub struct DocxSectionIterator {
    chunks: Vec<ChunkRecordInput>,
    index: usize,
}

#[pymethods]
impl DocxSectionIterator {
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
        dict.set_item("content_type", "section")?;
        dict.set_item("metadata", pythonize(py, &chunk.metadata)?)?;
        Ok(Some(dict.into_any().unbind()))
    }
}

#[pyfunction]
fn chunk_docx_section_stream(file_path: &str) -> PyResult<DocxSectionIterator> {
    if !file_path.to_ascii_lowercase().ends_with(".docx") {
        return Err(PyValueError::new_err(format!(
            "Expected .docx file path, got: {file_path}"
        )));
    }

    let bytes = fs::read(file_path)
        .map_err(|e| PyIOError::new_err(format!("Failed to read DOCX file: {e}")))?;

    let raw_blocks = parse_docx_blocks(&bytes)
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to parse DOCX: {e}")))?;
    let blocks = lower_blocks(raw_blocks);
    let chunks = build_section_chunks(blocks);

    Ok(DocxSectionIterator { chunks, index: 0 })
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(chunk_docx_section, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_docx_section_with_images, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_docx_section_stream, m)?)?;
    m.add_class::<DocxSectionIterator>()?;
    Ok(())
}
