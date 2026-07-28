use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};
use pyo3::wrap_pyfunction;
use pythonize::pythonize;
use std::time::Instant;

use super::common::{build_row_chunks, XlsxChunkRecord};

fn chunk_to_pydict(py: Python<'_>, chunk: &XlsxChunkRecord) -> PyResult<PyObject> {
    let dict = PyDict::new_bound(py);
    dict.set_item("content", &chunk.content)?;
    dict.set_item("content_type", &chunk.content_type)?;
    dict.set_item("metadata", pythonize(py, &chunk.metadata)?)?;
    Ok(dict.into_any().unbind())
}

pub(crate) fn map_build_error(err: String) -> PyErr {
    if err.starts_with("Sheet '") && err.ends_with("' not found") {
        PyValueError::new_err(err)
    } else {
        PyRuntimeError::new_err(err)
    }
}

#[pyfunction]
pub fn chunk_xlsx_row(
    py: Python<'_>,
    file_path: &str,
    rows_per_chunk: usize,
    include_headers: bool,
    sheet_names: Vec<String>,
    skip_empty_rows: bool,
) -> PyResult<PyObject> {
    if !super::common::is_supported_spreadsheet(file_path) {
        return Err(PyValueError::new_err(format!(
            "Expected a spreadsheet file ({}), got: {file_path}",
            super::common::supported_spreadsheet_exts_display()
        )));
    }
    if rows_per_chunk == 0 {
        return Err(PyValueError::new_err("rows_per_chunk must be > 0"));
    }

    let rust_start = Instant::now();
    let chunks_raw = build_row_chunks(
        file_path,
        rows_per_chunk,
        include_headers,
        sheet_names,
        skip_empty_rows,
    )
    .map_err(map_build_error)?;
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
    m.add_function(wrap_pyfunction!(chunk_xlsx_row, m)?)?;
    Ok(())
}