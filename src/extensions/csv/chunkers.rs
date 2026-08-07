//! `.csv` / `.tsv` chunking + markdown pyfunctions.
//!
//! **Migrated to the vendored engine.** Binding only — see CONSOLIDATION_PLAN.md.
//!
//! CSV does not use `bind_format!`: its modes are `row`/`sliding_window`/
//! `page_aware` rather than the markdown family's, and every entry point
//! carries the delimiter/encoding/header options.

use pyo3::prelude::*;
use pyo3::types::PyModule;
use pyo3::wrap_pyfunction;

use chunks_rs::formats::csv;

use crate::engine::{run, to_py_err, ChunkStreamIterator};

#[pyfunction]
#[pyo3(signature = (
    file_path, mode, rows_per_chunk, window_size, overlap, include_headers,
    delimiter=None, encoding="utf-8", skip_empty_rows=true
))]
#[allow(clippy::too_many_arguments)]
fn chunk_csv(
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
    run(py, || {
        csv::chunk(
            file_path,
            mode,
            rows_per_chunk,
            window_size,
            overlap,
            include_headers,
            delimiter,
            encoding,
            skip_empty_rows,
        )
    })
}

#[pyfunction]
#[pyo3(signature = (
    file_path, window_size, overlap, include_headers,
    delimiter=None, encoding="utf-8", skip_empty_rows=true
))]
fn chunk_csv_sliding_window(
    py: Python<'_>,
    file_path: &str,
    window_size: usize,
    overlap: usize,
    include_headers: bool,
    delimiter: Option<u8>,
    encoding: &str,
    skip_empty_rows: bool,
) -> PyResult<PyObject> {
    run(py, || {
        csv::chunk(
            file_path,
            "sliding_window",
            1,
            window_size,
            overlap,
            include_headers,
            delimiter,
            encoding,
            skip_empty_rows,
        )
    })
}

#[pyfunction]
#[pyo3(signature = (file_path, delimiter=None, encoding="utf-8"))]
fn csv_to_markdown(
    py: Python<'_>,
    file_path: &str,
    delimiter: Option<u8>,
    encoding: &str,
) -> PyResult<String> {
    py.allow_threads(|| csv::csv_to_markdown(file_path, delimiter, encoding))
        .map_err(to_py_err)
}

#[pyfunction]
#[pyo3(signature = (
    file_path, mode, rows_per_chunk, window_size, overlap, include_headers,
    delimiter=None, encoding="utf-8", skip_empty_rows=true
))]
#[allow(clippy::too_many_arguments)]
fn stream_csv_chunks(
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
) -> PyResult<Py<ChunkStreamIterator>> {
    let chunks = py
        .allow_threads(|| {
            csv::chunk(
                file_path,
                mode,
                rows_per_chunk,
                window_size,
                overlap,
                include_headers,
                delimiter,
                encoding,
                skip_empty_rows,
            )
        })
        .map_err(to_py_err)?;
    ChunkStreamIterator::build(py, chunks)
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(chunk_csv, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_csv_sliding_window, m)?)?;
    m.add_function(wrap_pyfunction!(csv_to_markdown, m)?)?;
    m.add_function(wrap_pyfunction!(stream_csv_chunks, m)?)?;
    Ok(())
}
