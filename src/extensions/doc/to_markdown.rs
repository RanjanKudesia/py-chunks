use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::PyModule;
use pyo3::wrap_pyfunction;

use super::structural::{load_doc_paragraphs, validate_doc_path};
use super::text_extractor::ParagraphType;

fn render_table_row(content: &str) -> String {
    let cells: Vec<String> = content.split('|').map(|c| c.trim().to_string()).collect();

    if cells.is_empty() {
        return String::new();
    }

    let row = format!("| {} |", cells.join(" | "));
    let sep = format!("| {} |", vec!["---"; cells.len()].join(" | "));
    format!("{row}\n{sep}")
}

#[pyfunction]
pub fn doc_to_markdown(file_path: &str) -> PyResult<String> {
    validate_doc_path(file_path)?;
    let paragraphs = load_doc_paragraphs(file_path).map_err(PyRuntimeError::new_err)?;

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

    out = out.trim().to_string();
    Ok(out)
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(doc_to_markdown, m)?)?;
    Ok(())
}
