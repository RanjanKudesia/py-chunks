pub mod common;
pub mod structural;

use pyo3::prelude::*;
use pyo3::types::PyModule;

// Register the pdf module
pub(crate) fn register(m: &pyo3::Bound<'_, PyModule>) -> PyResult<()> {
    structural::register(m)?;
    Ok(())
}
