use std::collections::HashMap;
use std::path::Path;

use pdfium_render::prelude::*;
use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyBytes;
use pyo3::wrap_pyfunction;

use super::common::{is_bullet_line, is_numbered_line, ParagraphKind};
use super::images::extract_pdf_images;
use super::structural::{
    collect_paragraph_records, extract_spans_from_doc, get_pdfium, group_spans_into_paragraphs,
    infer_section_level, ParagraphRecord,
};

fn validate_pdf_path(file_path: &str) -> PyResult<()> {
    let path = Path::new(file_path);
    if !path.exists() {
        return Err(PyIOError::new_err(format!(
            "PDF file not found: {file_path}"
        )));
    }
    if path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
        != Some("pdf")
    {
        return Err(PyValueError::new_err(format!(
            "Expected a .pdf file, got: {file_path}"
        )));
    }
    Ok(())
}

/// Group the document's text spans into paragraph records and compute the
/// document-wide average font size used for heading-level inference.
fn collect_records(doc: &PdfDocument<'_>) -> (Vec<ParagraphRecord>, f32) {
    let spans = extract_spans_from_doc(doc);
    if spans.is_empty() {
        return (Vec::new(), 12.0);
    }
    let grouped = group_spans_into_paragraphs(spans);
    let records = collect_paragraph_records(grouped);

    let doc_avg_font_size = if records.is_empty() {
        12.0f32
    } else {
        records.iter().map(|r| r.avg_font_size).sum::<f32>() / records.len() as f32
    };

    (records, doc_avg_font_size)
}

/// Render paragraph records to Markdown. When `images_by_page` is non-empty,
/// image placeholders (`![name](name)`) are emitted at the top of each page's
/// content, immediately after the page separator.
fn render_records_markdown(
    records: &[ParagraphRecord],
    doc_avg_font_size: f32,
    images_by_page: &HashMap<usize, Vec<String>>,
) -> String {
    let mut out = String::new();
    let mut prev_page: Option<usize> = None;

    for record in records {
        let page = record.page_number;
        let separator = match prev_page {
            None => "",
            Some(p) if p == page => "\n\n",
            _ => "\n\n---\n\n",
        };
        out.push_str(separator);

        // On entering a new page, emit that page's images before its text.
        if prev_page != Some(page) {
            if let Some(names) = images_by_page.get(&page) {
                for name in names {
                    out.push_str(&format!("![{name}]({name})\n\n"));
                }
            }
        }
        prev_page = Some(page);

        let para = &record.paragraph;

        if para.is_heading {
            let level = infer_section_level(record.avg_font_size, doc_avg_font_size);
            let hashes = "#".repeat(level as usize);
            out.push_str(&format!("{} {}", hashes, para.text.trim()));
            continue;
        }

        match para.kind {
            ParagraphKind::BulletList => {
                // Keep recognized list markers as-is; normalize plain lines to bullets.
                let mut lines: Vec<String> = Vec::new();
                for line in para.text.lines() {
                    let t = line.trim();
                    if t.is_empty() {
                        continue;
                    }
                    if is_bullet_line(t) || is_numbered_line(t) {
                        lines.push(t.to_string());
                    } else {
                        lines.push(format!("- {t}"));
                    }
                }
                out.push_str(lines.join("\n").trim());
            }
            ParagraphKind::Table => {
                out.push_str(&table_text_to_markdown_table(&para.text));
            }
            ParagraphKind::Plain | ParagraphKind::Heading => {
                out.push_str(para.text.trim());
            }
        }
    }

    out.trim().to_string()
}

#[pyfunction]
pub fn pdf_to_markdown(file_path: &str) -> PyResult<String> {
    validate_pdf_path(file_path)?;

    let pdfium = get_pdfium().map_err(PyRuntimeError::new_err)?;
    let doc = pdfium
        .load_pdf_from_file(file_path, None)
        .map_err(|e| PyIOError::new_err(format!("Failed to open PDF: {e}")))?;

    let (records, doc_avg_font_size) = collect_records(&doc);
    if records.is_empty() {
        return Ok(String::new());
    }

    Ok(render_records_markdown(
        &records,
        doc_avg_font_size,
        &HashMap::new(),
    ))
}

#[pyfunction]
pub fn pdf_to_markdown_with_images(
    py: Python<'_>,
    file_path: &str,
) -> PyResult<(String, Vec<(String, Py<PyBytes>)>)> {
    validate_pdf_path(file_path)?;

    let pdfium = get_pdfium().map_err(PyRuntimeError::new_err)?;
    let doc = pdfium
        .load_pdf_from_file(file_path, None)
        .map_err(|e| PyIOError::new_err(format!("Failed to open PDF: {e}")))?;

    let (records, doc_avg_font_size) = collect_records(&doc);
    let (image_infos, image_out) = extract_pdf_images(&doc);

    // Map each page to the (deduplicated) images that appear on it, preserving
    // first-seen order.
    let mut images_by_page: HashMap<usize, Vec<String>> = HashMap::new();
    for info in &image_infos {
        let entry = images_by_page.entry(info.page_number).or_default();
        if !entry.contains(&info.hash_name) {
            entry.push(info.hash_name.clone());
        }
    }

    let markdown = render_records_markdown(&records, doc_avg_font_size, &images_by_page);

    let image_out_py: Vec<(String, Py<PyBytes>)> = image_out
        .into_iter()
        .map(|(name, data)| (name, PyBytes::new_bound(py, &data).unbind()))
        .collect();

    Ok((markdown, image_out_py))
}

fn split_table_row(line: &str) -> Vec<String> {
    if line.contains('\t') {
        let cols: Vec<String> = line
            .split('\t')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
            .collect();
        if cols.len() >= 2 {
            return cols;
        }
    }

    // Split on runs of 2+ spaces.
    let chars: Vec<char> = line.chars().collect();
    let mut cols: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut i = 0usize;

    while i < chars.len() {
        if chars[i] == ' ' {
            let mut j = i;
            while j < chars.len() && chars[j] == ' ' {
                j += 1;
            }
            if j - i >= 2 {
                let cell = current.trim();
                if !cell.is_empty() {
                    cols.push(cell.to_string());
                }
                current.clear();
            } else {
                current.push(' ');
            }
            i = j;
            continue;
        }

        current.push(chars[i]);
        i += 1;
    }

    let tail = current.trim();
    if !tail.is_empty() {
        cols.push(tail.to_string());
    }

    cols
}

fn table_text_to_markdown_table(text: &str) -> String {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() {
        return String::new();
    }

    if lines.iter().any(|l| l.contains('|')) {
        return text.trim().to_string();
    }

    let rows: Vec<Vec<String>> = lines.iter().map(|line| split_table_row(line)).collect();
    let max_cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if max_cols < 2 {
        return text.trim().to_string();
    }

    let has_consistent_cols = rows.iter().all(|r| r.len() == max_cols);
    if !has_consistent_cols {
        return text.trim().to_string();
    }

    let mut md = String::new();
    for (i, row) in rows.iter().enumerate() {
        let mut cells: Vec<String> = row.iter().map(|c| c.replace('|', "\\|")).collect();
        while cells.len() < max_cols {
            cells.push(String::new());
        }
        md.push_str(&format!("| {} |\n", cells.join(" | ")));
        if i == 0 && rows.len() > 1 {
            let sep = vec!["---"; max_cols];
            md.push_str(&format!("| {} |\n", sep.join(" | ")));
        }
    }

    md.trim_end_matches('\n').to_string()
}

pub(crate) fn register(m: &pyo3::Bound<'_, PyModule>) -> pyo3::PyResult<()> {
    m.add_function(wrap_pyfunction!(pdf_to_markdown, m)?)?;
    m.add_function(wrap_pyfunction!(pdf_to_markdown_with_images, m)?)?;
    Ok(())
}
