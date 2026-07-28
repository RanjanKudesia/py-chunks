//! EPUB (.epub) support — OCF/OPF navigation, chunked via the HTML chunker in
//! spine (reading) order.

pub mod chunkers;
pub mod extract;
pub mod package;

use pyo3::prelude::*;
use pyo3::types::PyModule;

pub(crate) fn register(m: &pyo3::Bound<'_, PyModule>) -> PyResult<()> {
    chunkers::register(m)?;
    Ok(())
}
