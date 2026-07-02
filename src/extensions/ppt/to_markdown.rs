use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyModule};
use pyo3::wrap_pyfunction;

use crate::extensions::doc::text_extractor::{DocParagraph, ParagraphType};
use crate::extensions::doc::to_markdown::render_paragraphs_markdown;

use super::cfb_reader;
use super::images::extract_ppt_images;
use super::structural::{load_ppt_paragraphs, validate_ppt_path};
use super::text_extractor;

#[pyfunction]
pub fn ppt_to_markdown(file_path: &str) -> PyResult<String> {
    validate_ppt_path(file_path)?;
    let paragraphs = load_ppt_paragraphs(file_path).map_err(PyRuntimeError::new_err)?;
    Ok(render_paragraphs_markdown(paragraphs))
}

fn image_paragraph(hash_name: &str) -> DocParagraph {
    DocParagraph {
        content: format!("![]({hash_name})"),
        paragraph_type: ParagraphType::Normal,
        heading_level: None,
    }
}

#[pyfunction]
pub fn ppt_to_markdown_with_images(
    py: Python<'_>,
    file_path: &str,
) -> PyResult<(String, Vec<(String, Py<PyBytes>)>)> {
    validate_ppt_path(file_path)?;
    let stream =
        cfb_reader::read_powerpoint_document_stream(file_path).map_err(PyRuntimeError::new_err)?;
    let slides = text_extractor::extract_slides(&stream);
    // Image extraction must never break markdown conversion: degrade to none.
    let images = extract_ppt_images(file_path).unwrap_or_default();

    // Flatten slides exactly like extract_paragraphs, but append each slide's
    // image markers to that slide so they land in the right section. A slide
    // with no text but with images still gets emitted (markers only).
    let mut paragraphs: Vec<DocParagraph> = Vec::new();
    for (idx, slide) in slides.into_iter().enumerate() {
        let mut slide_paras = slide;
        for img in images.iter().filter(|i| i.slide_idx == Some(idx)) {
            slide_paras.push(image_paragraph(&img.hash_name));
        }
        if slide_paras.is_empty() {
            continue;
        }
        paragraphs.extend(slide_paras);
        paragraphs.push(DocParagraph {
            content: String::new(),
            paragraph_type: ParagraphType::PageBreak,
            heading_level: None,
        });
    }
    // Unanchored images go at the end of the document.
    for img in images.iter().filter(|i| i.slide_idx.is_none()) {
        paragraphs.push(image_paragraph(&img.hash_name));
    }
    while matches!(
        paragraphs.last().map(|p| &p.paragraph_type),
        Some(ParagraphType::PageBreak)
    ) {
        paragraphs.pop();
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
    m.add_function(wrap_pyfunction!(ppt_to_markdown, m)?)?;
    m.add_function(wrap_pyfunction!(ppt_to_markdown_with_images, m)?)?;
    Ok(())
}
