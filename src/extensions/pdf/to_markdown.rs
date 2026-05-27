use std::path::Path;

use pdfium_render::prelude::*;
use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::wrap_pyfunction;

use super::common::{is_bullet_line, is_numbered_line, ParagraphKind};
use super::structural::{
    collect_paragraph_records, extract_spans_from_doc, get_pdfium, group_spans_into_paragraphs,
    infer_section_level,
};

#[pyfunction]
pub fn pdf_to_markdown(file_path: &str) -> PyResult<String> {
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

    let pdfium = get_pdfium().map_err(PyRuntimeError::new_err)?;
    let doc = pdfium
        .load_pdf_from_file(file_path, None)
        .map_err(|e| PyIOError::new_err(format!("Failed to open PDF: {e}")))?;

    let spans = extract_spans_from_doc(&doc);
    if spans.is_empty() {
        return Ok(String::new());
    }
    let grouped = group_spans_into_paragraphs(spans);
    let records = collect_paragraph_records(grouped);

    let doc_avg_font_size = if records.is_empty() {
        12.0f32
    } else {
        records.iter().map(|r| r.avg_font_size).sum::<f32>() / records.len() as f32
    };

    let mut out = String::new();
    let mut prev_page: Option<usize> = None;

    for record in &records {
        let page = record.page_number;
        let separator = match prev_page {
            None => "",
            Some(p) if p == page => "\n\n",
            _ => "\n\n---\n\n",
        };
        out.push_str(separator);
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

    Ok(out.trim().to_string())
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
    Ok(())
}
