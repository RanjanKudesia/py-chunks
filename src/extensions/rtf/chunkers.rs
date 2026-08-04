//! `.rtf` chunking + markdown pyfunctions.
//!
//! **Migrated to the vendored engine** — all extraction and chunking lives in
//! `chunks_rs::formats::rtf`. This file is binding only: call, map the error,
//! convert. Do not reintroduce logic here (`CONSOLIDATION_PLAN.md`).

use pyo3::prelude::*;
use pyo3::types::PyModule;
use pyo3::wrap_pyfunction;

use chunks_rs::formats::rtf;

use crate::engine::{run, to_py_err, ChunkStreamIterator};

fn run_mode(
    py: Python<'_>,
    file_path: &str,
    mode: &str,
    window_size: usize,
    overlap: usize,
    sentences_per_chunk: usize,
    paragraphs_per_page: usize,
) -> PyResult<PyObject> {
    run(py, || {
        rtf::chunk(
            file_path,
            mode,
            window_size,
            overlap,
            sentences_per_chunk,
            paragraphs_per_page,
        )
    })
}

#[pyfunction]
fn chunk_rtf(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    run_mode(py, file_path, "default", 3, 1, 3, 15)
}
#[pyfunction]
fn chunk_rtf_section(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    run_mode(py, file_path, "section", 3, 1, 3, 15)
}
#[pyfunction]
fn chunk_rtf_semantic(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    run_mode(py, file_path, "semantic", 3, 1, 3, 15)
}
#[pyfunction]
fn chunk_rtf_sentence(
    py: Python<'_>,
    file_path: &str,
    sentences_per_chunk: usize,
) -> PyResult<PyObject> {
    run_mode(py, file_path, "sentence", 3, 1, sentences_per_chunk, 15)
}
#[pyfunction]
fn chunk_rtf_page_aware(
    py: Python<'_>,
    file_path: &str,
    paragraphs_per_page: usize,
) -> PyResult<PyObject> {
    run_mode(py, file_path, "page_aware", 3, 1, 3, paragraphs_per_page)
}
#[pyfunction]
fn chunk_rtf_sliding_window(
    py: Python<'_>,
    file_path: &str,
    window_size: usize,
    overlap: usize,
) -> PyResult<PyObject> {
    run_mode(py, file_path, "sliding_window", window_size, overlap, 3, 15)
}

#[pyfunction]
fn rtf_to_markdown(file_path: &str) -> PyResult<String> {
    rtf::to_markdown(file_path).map_err(to_py_err)
}

#[pyfunction]
#[pyo3(signature = (file_path, mode, window_size, overlap, sentences_per_chunk, paragraphs_per_page))]
fn stream_rtf_chunks(
    py: Python<'_>,
    file_path: &str,
    mode: &str,
    window_size: usize,
    overlap: usize,
    sentences_per_chunk: usize,
    paragraphs_per_page: usize,
) -> PyResult<Py<ChunkStreamIterator>> {
    let chunks = rtf::chunk(
        file_path,
        mode,
        window_size,
        overlap,
        sentences_per_chunk,
        paragraphs_per_page,
    )
    .map_err(to_py_err)?;
    ChunkStreamIterator::build(py, &chunks)
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(chunk_rtf, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_rtf_section, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_rtf_semantic, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_rtf_sentence, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_rtf_page_aware, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_rtf_sliding_window, m)?)?;
    m.add_function(wrap_pyfunction!(rtf_to_markdown, m)?)?;
    m.add_function(wrap_pyfunction!(stream_rtf_chunks, m)?)?;
    Ok(())
}
