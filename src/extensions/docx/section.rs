use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};
use pyo3::wrap_pyfunction;
use pythonize::pythonize;
use quick_xml::events::Event;
use quick_xml::name::QName;
use quick_xml::Reader;
use serde_json::{json, Value};
use std::fs;
use std::io::{BufReader, Cursor, Read};
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
    let blocks = parse_docx_blocks(&bytes)
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to parse DOCX: {e}")))?;
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

fn parse_docx_blocks(bytes: &[u8]) -> Result<Vec<DocumentBlock>, String> {
    let cursor = Cursor::new(bytes);
    let mut archive =
        ZipArchive::new(cursor).map_err(|e| format!("DOCX is not a valid zip archive: {e}"))?;

    let mut document_xml_file = archive
        .by_name("word/document.xml")
        .map_err(|_| "word/document.xml not found in DOCX".to_string())?;

    parse_document_xml_blocks_streaming(&mut document_xml_file)
}

fn qname_eq(name: QName<'_>, expected: &[u8]) -> bool {
    let n = name.as_ref();
    n == expected || n.rsplit(|b| *b == b':').next() == Some(expected)
}

fn push_text(target: &mut String, piece: &str) {
    let trimmed = piece.trim();
    if trimmed.is_empty() {
        return;
    }
    if !target.is_empty() {
        target.push(' ');
    }
    target.push_str(trimmed);
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

fn parse_document_xml_blocks_streaming<R: Read>(
    reader_src: R,
) -> Result<Vec<DocumentBlock>, String> {
    let mut reader = Reader::from_reader(BufReader::new(reader_src));

    let mut buf = Vec::new();
    let mut blocks = Vec::new();

    let mut in_table = false;
    let mut in_cell = false;
    let mut in_text = false;
    let mut in_paragraph = false;

    let mut para_text = String::new();
    let mut para_style: Option<String> = None;
    let mut para_outline_lvl: Option<u32> = None;
    let mut para_is_list = false;
    let mut para_has_drawing = false;

    let mut table_rows: Vec<String> = Vec::new();
    let mut row_cells: Vec<String> = Vec::new();
    let mut cell_text = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let name = e.name();
                if qname_eq(name, b"tbl") {
                    in_table = true;
                    table_rows.clear();
                    row_cells.clear();
                    cell_text.clear();
                } else if qname_eq(name, b"tr") && in_table {
                    row_cells.clear();
                } else if qname_eq(name, b"tc") && in_table {
                    in_cell = true;
                    cell_text.clear();
                } else if qname_eq(name, b"p") {
                    if !in_table {
                        in_paragraph = true;
                        para_text.clear();
                        para_style = None;
                        para_outline_lvl = None;
                        para_is_list = false;
                        para_has_drawing = false;
                    }
                } else if qname_eq(name, b"numPr") && in_paragraph {
                    para_is_list = true;
                } else if qname_eq(name, b"drawing") && in_paragraph {
                    para_has_drawing = true;
                } else if qname_eq(name, b"pStyle") && in_paragraph {
                    for attr in e.attributes().flatten() {
                        if qname_eq(attr.key, b"val") {
                            let v = String::from_utf8_lossy(attr.value.as_ref()).to_string();
                            para_style = Some(v);
                            break;
                        }
                    }
                } else if qname_eq(name, b"outlineLvl") && in_paragraph {
                    for attr in e.attributes().flatten() {
                        if qname_eq(attr.key, b"val") {
                            let raw = String::from_utf8_lossy(attr.value.as_ref()).to_string();
                            if let Ok(v) = raw.trim().parse::<u32>() {
                                para_outline_lvl = Some(v);
                            }
                            break;
                        }
                    }
                } else if qname_eq(name, b"t") {
                    in_text = true;
                }
            }
            Ok(Event::Empty(e)) => {
                let name = e.name();
                if qname_eq(name, b"numPr") && in_paragraph {
                    para_is_list = true;
                } else if qname_eq(name, b"drawing") && in_paragraph {
                    para_has_drawing = true;
                } else if qname_eq(name, b"pStyle") && in_paragraph {
                    for attr in e.attributes().flatten() {
                        if qname_eq(attr.key, b"val") {
                            let v = String::from_utf8_lossy(attr.value.as_ref()).to_string();
                            para_style = Some(v);
                            break;
                        }
                    }
                } else if qname_eq(name, b"outlineLvl") && in_paragraph {
                    for attr in e.attributes().flatten() {
                        if qname_eq(attr.key, b"val") {
                            let raw = String::from_utf8_lossy(attr.value.as_ref()).to_string();
                            if let Ok(v) = raw.trim().parse::<u32>() {
                                para_outline_lvl = Some(v);
                            }
                            break;
                        }
                    }
                }
            }
            Ok(Event::End(e)) => {
                let name = e.name();
                if qname_eq(name, b"t") {
                    in_text = false;
                } else if qname_eq(name, b"tc") && in_table {
                    let cell = cell_text.trim().to_string();
                    if !cell.is_empty() {
                        row_cells.push(cell);
                    }
                    cell_text.clear();
                    in_cell = false;
                } else if qname_eq(name, b"tr") && in_table {
                    if !row_cells.is_empty() {
                        table_rows.push(row_cells.join(" | "));
                    }
                    row_cells.clear();
                } else if qname_eq(name, b"tbl") && in_table {
                    let table_text = table_rows.join("\n").trim().to_string();
                    if !table_text.is_empty() {
                        blocks.push(DocumentBlock {
                            block_type: BlockType::Table,
                            text: table_text,
                            heading_level: None,
                        });
                    }
                    in_table = false;
                    in_cell = false;
                    table_rows.clear();
                    row_cells.clear();
                    cell_text.clear();
                } else if qname_eq(name, b"p") && in_paragraph {
                    let text = para_text.trim().to_string();
                    let has_text = !text.is_empty();
                    let heading_level =
                        parse_heading_level(para_style.as_deref(), para_outline_lvl);

                    if has_text || para_has_drawing {
                        let normalized = if has_text {
                            text
                        } else {
                            "[Image]".to_string()
                        };

                        let block_type = if heading_level.is_some() {
                            BlockType::Paragraph
                        } else if para_is_list {
                            BlockType::BulletList
                        } else if para_has_drawing {
                            BlockType::Image
                        } else {
                            BlockType::Paragraph
                        };

                        blocks.push(DocumentBlock {
                            block_type,
                            text: normalized,
                            heading_level,
                        });
                    }

                    in_paragraph = false;
                    para_text.clear();
                    para_style = None;
                    para_outline_lvl = None;
                    para_is_list = false;
                    para_has_drawing = false;
                }
            }
            Ok(Event::Text(t)) => {
                if in_text {
                    let txt = match t.decode() {
                        Ok(v) => v.into_owned(),
                        Err(_) => String::new(),
                    };
                    if in_table && in_cell {
                        push_text(&mut cell_text, &txt);
                    } else if in_paragraph {
                        push_text(&mut para_text, &txt);
                    }
                }
            }
            Ok(Event::CData(t)) => {
                if in_text {
                    let txt = String::from_utf8_lossy(t.as_ref());
                    if in_table && in_cell {
                        push_text(&mut cell_text, &txt);
                    } else if in_paragraph {
                        push_text(&mut para_text, &txt);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("Failed to parse word/document.xml stream: {e}")),
            _ => {}
        }
        buf.clear();
    }

    Ok(blocks)
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

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(chunk_docx_section, m)?)?;
    Ok(())
}
