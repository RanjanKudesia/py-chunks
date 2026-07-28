//! Rich Text Format (.rtf) support — spec-correct hand-rolled text extraction,
//! chunked via the Markdown chunker.

pub mod chunkers;
pub mod extract;

use pyo3::prelude::*;
use pyo3::types::PyModule;

pub(crate) fn register(m: &pyo3::Bound<'_, PyModule>) -> PyResult<()> {
    chunkers::register(m)?;
    Ok(())
}
