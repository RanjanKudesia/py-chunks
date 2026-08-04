//! The boundary between the vendored engine and Python.
//!
//! Everything below this module is `chunks_rs` — the single core, vendored at
//! `crates/rs-chunks` and overwritten by `../sync_engine.sh`. Everything above
//! it is PyO3. A migrated format's binding does three things and nothing else:
//! call the engine, map its error, convert its chunks. Any logic that creeps in
//! here is the fork growing back (see `CONSOLIDATION_PLAN.md`).

use std::time::Instant;

use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use pythonize::pythonize;

use chunks_rs::chunk::Chunk;
use chunks_rs::error::ChunkError;

/// Map an engine error onto the exception type the Python API has always
/// raised for that situation. The mapping is part of the public contract:
/// a bad extension is a `ValueError`, an unreadable file an `IOError`.
pub fn to_py_err(e: ChunkError) -> PyErr {
    match e {
        ChunkError::InvalidArg(m) => PyValueError::new_err(m),
        ChunkError::Unsupported(m) => PyValueError::new_err(m),
        ChunkError::Io(e) => PyIOError::new_err(e.to_string()),
        ChunkError::Parse(m) => PyRuntimeError::new_err(m),
    }
}

/// One chunk as the `{content, content_type, metadata}` dict Python expects.
pub fn chunk_to_pydict(py: Python<'_>, c: &Chunk) -> PyResult<PyObject> {
    let dict = PyDict::new_bound(py);
    dict.set_item("content", &c.content)?;
    dict.set_item("content_type", &c.content_type)?;
    dict.set_item("metadata", pythonize(py, &c.metadata)?)?;
    Ok(dict.into_any().unbind())
}

pub fn chunks_to_pylist(py: Python<'_>, chunks: &[Chunk]) -> PyResult<Vec<PyObject>> {
    chunks.iter().map(|c| chunk_to_pydict(py, c)).collect()
}

/// The `{chunks, rust_ms}` envelope every `chunk_*` pyfunction returns.
pub fn to_result_dict(py: Python<'_>, chunks: &[Chunk], started: Instant) -> PyResult<PyObject> {
    let rust_ms = started.elapsed().as_secs_f64() * 1000.0;
    let result = PyDict::new_bound(py);
    result.set_item("chunks", chunks_to_pylist(py, chunks)?)?;
    result.set_item("rust_ms", (rust_ms * 1000.0).round() / 1000.0)?;
    Ok(result.into_any().unbind())
}

/// Run an engine chunker and return the Python envelope, timing included.
pub fn run<F>(py: Python<'_>, call: F) -> PyResult<PyObject>
where
    F: FnOnce() -> chunks_rs::error::Result<Vec<Chunk>>,
{
    let started = Instant::now();
    let chunks = call().map_err(to_py_err)?;
    to_result_dict(py, &chunks, started)
}

/// A `__next__`-style iterator over already-materialised chunks.
///
/// The engine's `stream` returns an iterator, but every format currently
/// materialises it; this keeps the Python-visible behaviour identical while
/// leaving room for a genuinely lazy engine iterator later.
#[pyclass]
pub struct ChunkStreamIterator {
    chunks: std::vec::IntoIter<PyObject>,
}

#[pymethods]
impl ChunkStreamIterator {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }
    fn __next__(&mut self, _py: Python<'_>) -> Option<PyObject> {
        self.chunks.next()
    }
}

impl ChunkStreamIterator {
    pub fn build(py: Python<'_>, chunks: &[Chunk]) -> PyResult<Py<ChunkStreamIterator>> {
        Py::new(
            py,
            ChunkStreamIterator {
                chunks: chunks_to_pylist(py, chunks)?.into_iter(),
            },
        )
    }
}
