//! PDF support, backed by the `liteparse` crate for high-quality PDF → markdown
//! conversion, with chunking delegated to the Markdown chunker.

pub mod chunkers;
pub mod liteparse_backend;

use pyo3::prelude::*;
use pyo3::types::PyModule;

pub(crate) fn register(m: &pyo3::Bound<'_, PyModule>) -> PyResult<()> {
    chunkers::register(m)?;
    Ok(())
}
