//! No-filesystem bytes entry points over the engine's dispatch layer.
//!
//! The Python source-agnostic helpers (`get_chunks_from_bytes` and everything
//! that funnels into it — file objects, uploads, pre-signed URLs) used to
//! round-trip through a `tempfile.NamedTemporaryFile`. The engine has had a
//! full bytes API since the WASM port (`chunks_rs::dispatch`); these bindings
//! expose it so the Python layer can skip the extra copy and disk I/O.
//!
//! `csv`/`tsv` get their own entry points because the Python layer honours
//! caller-supplied `delimiter`/`encoding` overrides, which the generic
//! dispatch entry hard-codes.

use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyModule};
use pyo3::wrap_pyfunction;

use chunks_rs::formats::csv;

use crate::engine::{chunks_to_pylist, images_to_py, run, to_py_err};

/// `{chunks, rust_ms}` for any supported extension, routed like the engine's
/// path-based dispatch. `filename` is used only for extension detection.
#[pyfunction]
fn get_chunks_from_bytes(
    py: Python<'_>,
    data: &[u8],
    filename: &str,
    mode: &str,
    window_size: usize,
    overlap: usize,
    sentences_per_chunk: usize,
    paragraphs_per_page: usize,
) -> PyResult<PyObject> {
    run(py, || {
        chunks_rs::get_chunks_from_bytes(
            data,
            filename,
            mode,
            window_size,
            overlap,
            sentences_per_chunk,
            paragraphs_per_page,
        )
    })
}

/// Chunks plus extracted images for any supported extension.
#[pyfunction]
fn get_chunks_with_images_from_bytes(
    py: Python<'_>,
    data: &[u8],
    filename: &str,
    mode: &str,
    window_size: usize,
    overlap: usize,
    sentences_per_chunk: usize,
    paragraphs_per_page: usize,
) -> PyResult<(Vec<PyObject>, Vec<(String, Py<PyBytes>)>)> {
    let (chunks, images) = py
        .allow_threads(|| {
            chunks_rs::get_chunks_with_images_from_bytes(
                data,
                filename,
                mode,
                window_size,
                overlap,
                sentences_per_chunk,
                paragraphs_per_page,
            )
        })
        .map_err(to_py_err)?;
    Ok((chunks_to_pylist(py, &chunks)?, images_to_py(py, images)))
}

/// Markdown conversion for any supported extension.
#[pyfunction]
fn get_markdown_from_bytes(py: Python<'_>, data: &[u8], filename: &str) -> PyResult<String> {
    py.allow_threads(|| chunks_rs::get_markdown_from_bytes(data, filename))
        .map_err(to_py_err)
}

/// Markdown plus extracted images for any supported extension.
#[pyfunction]
fn get_markdown_with_images_from_bytes(
    py: Python<'_>,
    data: &[u8],
    filename: &str,
) -> PyResult<(String, Vec<(String, Py<PyBytes>)>)> {
    let (md, images) = py
        .allow_threads(|| chunks_rs::get_markdown_with_images_from_bytes(data, filename))
        .map_err(to_py_err)?;
    Ok((md, images_to_py(py, images)))
}

/// CSV/TSV bytes chunking with the full delimiter/encoding surface of
/// `chunk_csv` — used by the Python bytes path so overrides keep working.
#[pyfunction]
#[pyo3(signature = (
    data, mode, rows_per_chunk, window_size, overlap, include_headers,
    delimiter=None, encoding="utf-8", skip_empty_rows=true
))]
#[allow(clippy::too_many_arguments)]
fn chunk_csv_from_bytes(
    py: Python<'_>,
    data: &[u8],
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
        csv::chunk_from_bytes(
            data,
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

/// CSV/TSV bytes → Markdown pipe table, honouring delimiter/encoding.
#[pyfunction]
#[pyo3(signature = (data, delimiter=None, encoding="utf-8"))]
fn csv_to_markdown_from_bytes(
    py: Python<'_>,
    data: &[u8],
    delimiter: Option<u8>,
    encoding: &str,
) -> PyResult<String> {
    py.allow_threads(|| csv::to_markdown_from_bytes(data, delimiter, encoding))
        .map_err(to_py_err)
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(get_chunks_from_bytes, m)?)?;
    m.add_function(wrap_pyfunction!(get_chunks_with_images_from_bytes, m)?)?;
    m.add_function(wrap_pyfunction!(get_markdown_from_bytes, m)?)?;
    m.add_function(wrap_pyfunction!(get_markdown_with_images_from_bytes, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_csv_from_bytes, m)?)?;
    m.add_function(wrap_pyfunction!(csv_to_markdown_from_bytes, m)?)?;
    Ok(())
}
