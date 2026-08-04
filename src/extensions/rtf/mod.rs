//! Rich Text Format (.rtf) support — spec-correct hand-rolled text extraction,
//! chunked via the Markdown chunker.

pub mod chunkers;
pub mod encoding;
pub mod extract;
pub mod fonts;
pub mod lists;
pub mod meta;
pub mod scan;
pub mod styles;
pub mod writer;

use pyo3::prelude::*;
use pyo3::types::PyModule;

pub(crate) fn register(m: &pyo3::Bound<'_, PyModule>) -> PyResult<()> {
    chunkers::register(m)?;
    Ok(())
}
