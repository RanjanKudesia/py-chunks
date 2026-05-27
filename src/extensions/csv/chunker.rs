use std::time::Instant;

use csv::{ReaderBuilder, Trim};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};
use pyo3::wrap_pyfunction;
use pythonize::pythonize;
use serde_json::json;

use super::common::{
    detect_delimiter, read_first_data_line, read_to_utf8, serialize_row_kv, serialize_row_values,
    CsvChunkRecord, CT_ROW_GROUP,
};
use super::sliding_window::build_sliding_window_chunks;

fn chunk_to_pydict(py: Python<'_>, chunk: &CsvChunkRecord) -> PyResult<PyObject> {
    let dict = PyDict::new_bound(py);
    dict.set_item("content", &chunk.content)?;
    dict.set_item("content_type", &chunk.content_type)?;
    dict.set_item("metadata", pythonize(py, &chunk.metadata)?)?;
    Ok(dict.into_any().unbind())
}

pub(crate) fn delimiter_byte(
    delimiter: Option<u8>,
    file_path: &str,
    encoding: &str,
) -> Result<u8, String> {
    match delimiter {
        Some(byte) => Ok(byte),
        None => {
            let first_line = read_first_data_line(file_path, encoding)?
                .ok_or_else(|| "CSV file is empty".to_string())?;
            Ok(detect_delimiter(&first_line))
        }
    }
}

pub(crate) fn normalize_headers(mut headers: Vec<String>, width: usize) -> Vec<String> {
    if headers.len() < width {
        headers.extend((headers.len()..width).map(|idx| format!("Column {}", idx + 1)));
    }
    headers
}

pub(crate) fn is_empty_row(row: &[String]) -> bool {
    row.iter().all(|value| value.trim().is_empty())
}

pub(crate) fn parse_csv_to_rows(
    file_path: &str,
    delimiter: Option<u8>,
    encoding: &str,
    skip_empty_rows: bool,
) -> Result<(Vec<String>, Vec<Vec<String>>, u8), String> {
    let delimiter = delimiter_byte(delimiter, file_path, encoding)?;
    let text = read_to_utf8(file_path, encoding)?;
    let mut reader = ReaderBuilder::new()
        .delimiter(delimiter)
        .trim(Trim::None)
        .flexible(true)
        .has_headers(false)
        .comment(Some(b'#'))
        .from_reader(text.as_bytes());

    let mut records = reader.records();
    let header_record = match records.next() {
        Some(Ok(record)) => record,
        Some(Err(err)) => return Err(format!("Failed to read CSV header: {err}")),
        None => return Ok((Vec::new(), Vec::new(), delimiter)),
    };

    let mut headers: Vec<String> = header_record
        .iter()
        .map(|value| value.to_string())
        .collect();
    let mut data_rows: Vec<Vec<String>> = Vec::new();
    let mut max_width = headers.len();

    for record in records {
        let record = record.map_err(|err| format!("Failed to read CSV row: {err}"))?;
        let row: Vec<String> = record.iter().map(|value| value.to_string()).collect();
        if skip_empty_rows && is_empty_row(&row) {
            continue;
        }
        max_width = max_width.max(row.len());
        data_rows.push(row);
    }

    headers = normalize_headers(headers, max_width);
    for row in &mut data_rows {
        if row.len() < max_width {
            row.extend(std::iter::repeat_with(String::new).take(max_width - row.len()));
        }
    }

    Ok((headers, data_rows, delimiter))
}

fn build_content(headers: &[String], rows: &[Vec<String>], include_headers: bool) -> String {
    rows.iter()
        .map(|row| {
            if include_headers {
                serialize_row_kv(headers, row)
            } else {
                serialize_row_values(row)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn delimiter_str(delimiter: u8) -> String {
    char::from(delimiter).to_string()
}

pub fn build_row_chunks(
    file_path: &str,
    rows_per_chunk: usize,
    include_headers: bool,
    delimiter: Option<u8>,
    encoding: &str,
    skip_empty_rows: bool,
) -> Result<Vec<CsvChunkRecord>, String> {
    if rows_per_chunk == 0 {
        return Err("rows_per_chunk must be greater than 0".to_string());
    }

    let (headers, data_rows, delimiter) =
        parse_csv_to_rows(file_path, delimiter, encoding, skip_empty_rows)?;

    let mut chunks = Vec::new();
    let mut row_start = 1usize;

    for (chunk_index, group) in data_rows.chunks(rows_per_chunk).enumerate() {
        let row_count = group.len();
        let row_end = row_start + row_count - 1;
        chunks.push(CsvChunkRecord {
            content: build_content(&headers, group, include_headers),
            content_type: CT_ROW_GROUP.to_string(),
            metadata: json!({
                "row_start": row_start,
                "row_end": row_end,
                "row_count": row_count,
                "col_count": headers.len(),
                "header_row": headers,
                "delimiter_detected": delimiter_str(delimiter),
                "encoding": encoding.to_ascii_lowercase(),
                "chunk_index": chunk_index,
            }),
        });
        row_start = row_end + 1;
    }

    Ok(chunks)
}

pub fn build_page_aware_chunks(
    file_path: &str,
    rows_per_page: usize,
    include_headers: bool,
    delimiter: Option<u8>,
    encoding: &str,
    skip_empty_rows: bool,
) -> Result<Vec<CsvChunkRecord>, String> {
    build_row_chunks(
        file_path,
        rows_per_page,
        include_headers,
        delimiter,
        encoding,
        skip_empty_rows,
    )
}

#[pyfunction]
#[pyo3(signature = (
    file_path,
    mode,
    rows_per_chunk,
    window_size,
    overlap,
    include_headers,
    delimiter=None,
    encoding="utf-8",
    skip_empty_rows=true
))]
pub fn chunk_csv(
    py: Python<'_>,
    file_path: &str,
    mode: &str,
    rows_per_chunk: usize,
    window_size: usize,
    overlap: usize,
    include_headers: bool,
    delimiter: Option<u8>,
    encoding: &str,
    skip_empty_rows: bool,
) -> PyResult<PyObject> {
    if !file_path.to_ascii_lowercase().ends_with(".csv") {
        return Err(PyValueError::new_err(format!(
            "Expected .csv file path, got: {file_path}"
        )));
    }

    match mode {
        "row" | "default" | "sliding_window" | "page_aware" => {}
        _ => {
            return Err(PyValueError::new_err(
                "mode must be 'row', 'default', 'sliding_window', or 'page_aware' for CSV",
            ));
        }
    }

    if rows_per_chunk == 0 {
        return Err(PyValueError::new_err(
            "rows_per_chunk must be greater than 0",
        ));
    }
    if window_size == 0 {
        return Err(PyValueError::new_err("window_size must be greater than 0"));
    }
    if overlap >= window_size {
        return Err(PyValueError::new_err(
            "overlap must be less than window_size",
        ));
    }

    let rust_start = Instant::now();
    let chunks_raw = if mode == "page_aware" {
        build_page_aware_chunks(
            file_path,
            rows_per_chunk,
            include_headers,
            delimiter,
            encoding,
            skip_empty_rows,
        )
    } else if mode == "sliding_window" {
        build_sliding_window_chunks(
            file_path,
            window_size,
            overlap,
            include_headers,
            delimiter,
            encoding,
            skip_empty_rows,
        )
    } else {
        build_row_chunks(
            file_path,
            rows_per_chunk,
            include_headers,
            delimiter,
            encoding,
            skip_empty_rows,
        )
    }
    .map_err(|err| PyRuntimeError::new_err(err))?;
    let rust_ms = (rust_start.elapsed().as_secs_f64() * 1000.0).max(0.001);

    let chunk_list: Vec<PyObject> = chunks_raw
        .iter()
        .map(|chunk| chunk_to_pydict(py, chunk))
        .collect::<PyResult<_>>()?;

    let result = PyDict::new_bound(py);
    result.set_item("chunks", chunk_list)?;
    result.set_item("rust_ms", rust_ms)?;
    Ok(result.into_any().unbind())
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(chunk_csv, m)?)?;
    Ok(())
}
