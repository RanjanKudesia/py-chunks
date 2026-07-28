//! Jupyter notebook (.ipynb) support — JSON cell assembly → Markdown chunker.

pub mod chunkers;
pub mod extract;

use pyo3::prelude::*;
use pyo3::types::PyModule;

pub(crate) fn register(m: &pyo3::Bound<'_, PyModule>) -> PyResult<()> {
    chunkers::register(m)?;
    Ok(())
}
