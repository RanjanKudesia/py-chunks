use csv::{ReaderBuilder, Trim};
use pyo3::exceptions::{PyIOError, PyValueError};
use pyo3::prelude::*;
use pyo3::wrap_pyfunction;

use super::chunker::{delimiter_byte, is_empty_row, normalize_headers};
use super::common::{detect_delimiter, read_first_data_line, read_to_utf8};

#[pyfunction]
#[pyo3(signature = (file_path, delimiter=None, encoding="utf-8"))]
pub fn csv_to_markdown(file_path: &str, delimiter: Option<u8>, encoding: &str) -> PyResult<String> {
    let path = std::path::Path::new(file_path);
    if !path.exists() {
        return Err(PyIOError::new_err(format!(
            "CSV file not found: {file_path}"
        )));
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    if ext.as_deref() != Some("csv") {
        return Err(PyValueError::new_err(format!(
            "Expected a .csv file, got: {file_path}"
        )));
    }

    let delim_byte = match delimiter {
        Some(b) => b,
        None => {
            let first_line =
                read_first_data_line(file_path, encoding).map_err(PyIOError::new_err)?;
            match first_line {
                Some(line) => detect_delimiter(&line),
                None => return Ok(String::new()),
            }
        }
    };

    let text = read_to_utf8(file_path, encoding).map_err(PyIOError::new_err)?;

    let mut reader = ReaderBuilder::new()
        .delimiter(delim_byte)
        .trim(Trim::None)
        .flexible(true)
        .has_headers(false)
        .comment(Some(b'#'))
        .from_reader(text.as_bytes());

    let mut records = reader.records();

    let header_record = match records.next() {
        Some(Ok(r)) => r,
        Some(Err(e)) => {
            return Err(PyIOError::new_err(format!(
                "Failed to read CSV header: {e}"
            )))
        }
        None => return Ok(String::new()),
    };

    let mut headers: Vec<String> = header_record.iter().map(|v| v.to_string()).collect();
    let mut data_rows: Vec<Vec<String>> = Vec::new();
    let mut max_width = headers.len();

    for result in records {
        let record =
            result.map_err(|e| PyIOError::new_err(format!("Failed to read CSV row: {e}")))?;
        let row: Vec<String> = record.iter().map(|v| v.to_string()).collect();
        if is_empty_row(&row) {
            continue;
        }
        max_width = max_width.max(row.len());
        data_rows.push(row);
    }

    headers = normalize_headers(headers, max_width);

    for row in &mut data_rows {
        while row.len() < max_width {
            row.push(String::new());
        }
    }

    if max_width == 0 {
        return Ok(String::new());
    }

    let escape = |s: &str| s.replace('|', "\\|");

    let header_line = format!(
        "| {} |",
        headers
            .iter()
            .map(|h| escape(h))
            .collect::<Vec<_>>()
            .join(" | ")
    );
    let sep_line = format!("| {} |", vec!["---"; max_width].join(" | "));

    let mut lines: Vec<String> = vec![header_line, sep_line];
    for row in &data_rows {
        let cell_line = format!(
            "| {} |",
            row.iter()
                .map(|c| escape(c))
                .collect::<Vec<_>>()
                .join(" | ")
        );
        lines.push(cell_line);
    }

    Ok(lines.join("\n"))
}

pub(crate) fn register(m: &pyo3::Bound<'_, PyModule>) -> pyo3::PyResult<()> {
    m.add_function(wrap_pyfunction!(csv_to_markdown, m)?)?;
    Ok(())
}
