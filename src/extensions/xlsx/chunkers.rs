//! Spreadsheet (`.xlsx` / `.xls` / `.xlsm` / `.xlsb` / `.ods` / `.xltx` /
//! `.xltm`) chunking, streaming, images and markdown pyfunctions.
//!
//! **Migrated to the vendored engine.** Binding only — see CONSOLIDATION_PLAN.md.
//!
//! xlsx is the only format whose ABI is per-mode *with per-mode parameter
//! lists*: `row` and `semantic` take `rows_per_chunk`, `table`/`sheet`/
//! `page_aware` take `max_chunk_chars`, `sliding_window` takes
//! `window_size`/`overlap`. The engine's `chunk` takes all of them at once, so
//! each entry point supplies defaults for the parameters its mode ignores —
//! and those defaults must be *valid*, because `chunk_from_bytes` validates
//! `rows_per_chunk >= 1` and `max_chunk_chars >= 1` unconditionally for every
//! mode. [`DEFAULT_ROWS`] / [`DEFAULT_MAX_CHARS`] exist for exactly that.
//!
//! It is also the only format with genuinely lazy streaming. That now lives in
//! the engine (`chunks_rs::formats::xlsx::stream`), so this binding is a thin
//! `__next__` over `XlsxChunkStream` rather than the state machines it used to
//! own.

use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyModule};
use pyo3::wrap_pyfunction;

use chunks_rs::formats::xlsx as engine;

use crate::engine::{chunk_to_pydict, chunks_to_pylist, images_to_py, run, to_py_err};

/// Placeholder for modes that take no `rows_per_chunk`. Must be >= 1: the
/// engine validates it whatever the mode.
const DEFAULT_ROWS: usize = 1;
/// Placeholder for modes that take no `max_chunk_chars`, same reason. 2000 is
/// the value the engine's own dispatch entry uses.
const DEFAULT_MAX_CHARS: usize = 2000;
/// Placeholders for modes that take no window. `overlap < window_size` must
/// hold even when unused.
const DEFAULT_WINDOW: usize = 3;
const DEFAULT_OVERLAP: usize = 1;

#[allow(clippy::too_many_arguments)]
fn run_mode(
    py: Python<'_>,
    file_path: &str,
    mode: &str,
    rows_per_chunk: usize,
    window_size: usize,
    overlap: usize,
    include_headers: bool,
    sheet_names: Vec<String>,
    skip_empty_rows: bool,
    max_chunk_chars: usize,
) -> PyResult<PyObject> {
    run(py, move || {
        engine::chunk(
            file_path,
            mode,
            rows_per_chunk,
            window_size,
            overlap,
            include_headers,
            sheet_names,
            skip_empty_rows,
            max_chunk_chars,
        )
    })
}

// ── Per-mode chunkers ─────────────────────────────────────────────────────────

#[pyfunction]
fn chunk_xlsx_row(
    py: Python<'_>,
    file_path: &str,
    rows_per_chunk: usize,
    include_headers: bool,
    sheet_names: Vec<String>,
    skip_empty_rows: bool,
) -> PyResult<PyObject> {
    run_mode(py, file_path, "row", rows_per_chunk, DEFAULT_WINDOW, DEFAULT_OVERLAP,
             include_headers, sheet_names, skip_empty_rows, DEFAULT_MAX_CHARS)
}

#[pyfunction]
fn chunk_xlsx_semantic(
    py: Python<'_>,
    file_path: &str,
    rows_per_chunk: usize,
    include_headers: bool,
    sheet_names: Vec<String>,
    skip_empty_rows: bool,
) -> PyResult<PyObject> {
    run_mode(py, file_path, "semantic", rows_per_chunk, DEFAULT_WINDOW, DEFAULT_OVERLAP,
             include_headers, sheet_names, skip_empty_rows, DEFAULT_MAX_CHARS)
}

#[pyfunction]
fn chunk_xlsx_table(
    py: Python<'_>,
    file_path: &str,
    include_headers: bool,
    sheet_names: Vec<String>,
    skip_empty_rows: bool,
    max_chunk_chars: usize,
) -> PyResult<PyObject> {
    run_mode(py, file_path, "table", DEFAULT_ROWS, DEFAULT_WINDOW, DEFAULT_OVERLAP,
             include_headers, sheet_names, skip_empty_rows, max_chunk_chars)
}

#[pyfunction]
fn chunk_xlsx_sheet(
    py: Python<'_>,
    file_path: &str,
    include_headers: bool,
    sheet_names: Vec<String>,
    skip_empty_rows: bool,
    max_chunk_chars: usize,
) -> PyResult<PyObject> {
    run_mode(py, file_path, "sheet", DEFAULT_ROWS, DEFAULT_WINDOW, DEFAULT_OVERLAP,
             include_headers, sheet_names, skip_empty_rows, max_chunk_chars)
}

#[pyfunction]
fn chunk_xlsx_page_aware(
    py: Python<'_>,
    file_path: &str,
    include_headers: bool,
    sheet_names: Vec<String>,
    skip_empty_rows: bool,
    max_chunk_chars: usize,
) -> PyResult<PyObject> {
    run_mode(py, file_path, "page_aware", DEFAULT_ROWS, DEFAULT_WINDOW, DEFAULT_OVERLAP,
             include_headers, sheet_names, skip_empty_rows, max_chunk_chars)
}

#[pyfunction]
fn chunk_xlsx_sliding_window(
    py: Python<'_>,
    file_path: &str,
    window_size: usize,
    overlap: usize,
    include_headers: bool,
    sheet_names: Vec<String>,
    skip_empty_rows: bool,
) -> PyResult<PyObject> {
    run_mode(py, file_path, "sliding_window", DEFAULT_ROWS, window_size, overlap,
             include_headers, sheet_names, skip_empty_rows, DEFAULT_MAX_CHARS)
}

// ── Streaming ─────────────────────────────────────────────────────────────────

/// Wraps the engine's lazy [`engine::XlsxChunkStream`]. For `row` and
/// `sliding_window` nothing beyond the next chunk is built until `__next__` is
/// called; the other modes drain a pre-built list.
#[pyclass]
pub struct XlsxStreamIterator {
    stream: engine::XlsxChunkStream,
}

#[pymethods]
impl XlsxStreamIterator {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(&mut self, py: Python<'_>) -> PyResult<Option<PyObject>> {
        // The lazy backends parse on pull — release the GIL for the engine
        // work, convert to a dict after reacquisition.
        let stream = &mut self.stream;
        match py.allow_threads(|| stream.next()) {
            None => Ok(None),
            Some(Ok(chunk)) => chunk_to_pydict(py, &chunk).map(Some),
            Some(Err(e)) => Err(to_py_err(e)),
        }
    }
}

#[pyfunction]
#[allow(clippy::too_many_arguments)]
#[pyo3(signature = (file_path, mode, rows_per_chunk, window_size, overlap, include_headers, sheet_names, skip_empty_rows, max_chunk_chars))]
fn stream_xlsx_chunks(
    py: Python<'_>,
    file_path: &str,
    mode: &str,
    rows_per_chunk: usize,
    window_size: usize,
    overlap: usize,
    include_headers: bool,
    sheet_names: Vec<String>,
    skip_empty_rows: bool,
    max_chunk_chars: usize,
) -> PyResult<XlsxStreamIterator> {
    let stream = py
        .allow_threads(move || {
            engine::stream(
                file_path,
                mode,
                rows_per_chunk,
                window_size,
                overlap,
                include_headers,
                sheet_names,
                skip_empty_rows,
                max_chunk_chars,
            )
        })
        .map_err(to_py_err)?;
    Ok(XlsxStreamIterator { stream })
}

// ── Images + markdown ─────────────────────────────────────────────────────────

#[pyfunction]
#[allow(clippy::too_many_arguments)]
#[pyo3(signature = (file_path, mode, rows_per_chunk, window_size, overlap, include_headers, sheet_names, skip_empty_rows, max_chunk_chars))]
fn chunk_xlsx_with_images(
    py: Python<'_>,
    file_path: &str,
    mode: &str,
    rows_per_chunk: usize,
    window_size: usize,
    overlap: usize,
    include_headers: bool,
    sheet_names: Vec<String>,
    skip_empty_rows: bool,
    max_chunk_chars: usize,
) -> PyResult<(Vec<PyObject>, Vec<(String, Py<PyBytes>)>)> {
    let (chunks, images) = py
        .allow_threads(move || {
            engine::chunk_with_images(
                file_path,
                mode,
                rows_per_chunk,
                window_size,
                overlap,
                include_headers,
                sheet_names,
                skip_empty_rows,
                max_chunk_chars,
            )
        })
        .map_err(to_py_err)?;
    Ok((chunks_to_pylist(py, &chunks)?, images_to_py(py, images)))
}

/// Kept `py`-less in the Python signature (pyo3 hides the `py` token); part of
/// the shipped surface alongside `pptx_to_markdown`.
#[pyfunction]
fn xlsx_to_markdown(py: Python<'_>, file_path: &str) -> PyResult<String> {
    py.allow_threads(|| engine::to_markdown(file_path)).map_err(to_py_err)
}

#[pyfunction]
fn xlsx_to_markdown_with_images(
    py: Python<'_>,
    file_path: &str,
) -> PyResult<(String, Vec<(String, Py<PyBytes>)>)> {
    let (md, images) = py
        .allow_threads(|| engine::to_markdown_with_images(file_path))
        .map_err(to_py_err)?;
    Ok((md, images_to_py(py, images)))
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(chunk_xlsx_row, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_xlsx_semantic, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_xlsx_table, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_xlsx_sheet, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_xlsx_page_aware, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_xlsx_sliding_window, m)?)?;
    m.add_function(wrap_pyfunction!(stream_xlsx_chunks, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_xlsx_with_images, m)?)?;
    m.add_function(wrap_pyfunction!(xlsx_to_markdown, m)?)?;
    m.add_function(wrap_pyfunction!(xlsx_to_markdown_with_images, m)?)?;
    m.add_class::<XlsxStreamIterator>()?;
    Ok(())
}
