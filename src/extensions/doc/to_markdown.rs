use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyModule};
use pyo3::wrap_pyfunction;

use super::images::extract_doc_images;
use super::structural::{load_doc_paragraphs, load_doc_paragraphs_indexed, validate_doc_path};
use super::text_extractor::{DocParagraph, ParagraphType};

fn render_table_row(content: &str) -> String {
    let cells: Vec<String> = content.split('|').map(|c| c.trim().to_string()).collect();

    if cells.is_empty() {
        return String::new();
    }

    let row = format!("| {} |", cells.join(" | "));
    let sep = format!("| {} |", vec!["---"; cells.len()].join(" | "));
    format!("{row}\n{sep}")
}

/// Render a paragraph list to Markdown. Shared by `.doc` and `.ppt`, both of
/// which produce `Vec<DocParagraph>`.
pub(crate) fn render_paragraphs_markdown(paragraphs: Vec<DocParagraph>) -> String {
    let mut rendered: Vec<String> = Vec::new();

    for p in paragraphs {
        match p.paragraph_type {
            ParagraphType::PageBreak => rendered.push("\n\n---\n\n".to_string()),
            ParagraphType::Heading(level) => {
                let content = p.content.trim();
                if content.is_empty() {
                    continue;
                }
                let line = match level {
                    1 => format!("# {content}"),
                    2 | 3 => format!("## {content}"),
                    _ => format!("### {content}"),
                };
                rendered.push(line);
            }
            ParagraphType::ListItem => {
                let content = p.content.trim();
                if content.is_empty() {
                    continue;
                }
                rendered.push(format!("- {content}"));
            }
            ParagraphType::Table => {
                let content = p.content.trim();
                if content.is_empty() {
                    continue;
                }
                let table = render_table_row(content);
                if !table.trim().is_empty() {
                    rendered.push(table);
                }
            }
            ParagraphType::Normal => {
                let content = p.content.trim();
                if content.is_empty() {
                    continue;
                }
                rendered.push(content.to_string());
            }
        }
    }

    let mut out = rendered.join("\n\n").trim().to_string();

    while out.ends_with("\n\n---") {
        out.truncate(out.len() - "\n\n---".len());
        out = out.trim_end().to_string();
    }

    out.trim().to_string()
}

#[pyfunction]
pub fn doc_to_markdown(file_path: &str) -> PyResult<String> {
    validate_doc_path(file_path)?;
    let paragraphs = load_doc_paragraphs(file_path).map_err(PyRuntimeError::new_err)?;
    Ok(render_paragraphs_markdown(paragraphs))
}

#[pyfunction]
pub fn doc_to_markdown_with_images(
    py: Python<'_>,
    file_path: &str,
) -> PyResult<(String, Vec<(String, Py<PyBytes>)>)> {
    validate_doc_path(file_path)?;
    let indexed = load_doc_paragraphs_indexed(file_path).map_err(PyRuntimeError::new_err)?;
    // Image extraction must never break markdown conversion: degrade to none.
    let images = extract_doc_images(file_path).unwrap_or_default();

    // Interleave image markers by raw paragraph ordinal: an image anchored in
    // raw paragraph N is emitted right after the last surviving paragraph
    // whose raw index is <= N. Unanchored images land at the end.
    let mut anchored: Vec<(usize, &str)> = images
        .iter()
        .filter_map(|img| img.raw_para.map(|rp| (rp, img.hash_name.as_str())))
        .collect();
    anchored.sort_by_key(|(rp, _)| *rp);
    let mut anchored_iter = anchored.into_iter().peekable();

    let image_paragraph = |hash_name: &str| DocParagraph {
        content: format!("![]({hash_name})"),
        paragraph_type: ParagraphType::Normal,
        heading_level: None,
    };

    let mut paragraphs: Vec<DocParagraph> = Vec::with_capacity(indexed.len() + images.len());
    for (raw_idx, para) in indexed {
        while let Some((rp, _)) = anchored_iter.peek() {
            if *rp < raw_idx {
                let (_, hash) = anchored_iter.next().unwrap();
                paragraphs.push(image_paragraph(hash));
            } else {
                break;
            }
        }
        paragraphs.push(para);
    }
    for (_, hash) in anchored_iter {
        paragraphs.push(image_paragraph(hash));
    }
    for img in images.iter().filter(|i| i.raw_para.is_none()) {
        paragraphs.push(image_paragraph(&img.hash_name));
    }

    let mut image_out: Vec<(String, Py<PyBytes>)> = Vec::new();
    for img in &images {
        if !image_out.iter().any(|(n, _)| n == &img.hash_name) {
            image_out.push((
                img.hash_name.clone(),
                PyBytes::new_bound(py, &img.bytes).unbind(),
            ));
        }
    }

    Ok((render_paragraphs_markdown(paragraphs), image_out))
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(doc_to_markdown, m)?)?;
    m.add_function(wrap_pyfunction!(doc_to_markdown_with_images, m)?)?;
    Ok(())
}
