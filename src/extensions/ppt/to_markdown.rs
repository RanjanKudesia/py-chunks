use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::PyModule;
use pyo3::wrap_pyfunction;

use crate::extensions::doc::to_markdown::render_paragraphs_markdown;

use super::structural::{load_ppt_paragraphs, validate_ppt_path};

#[pyfunction]
pub fn ppt_to_markdown(file_path: &str) -> PyResult<String> {
    validate_ppt_path(file_path)?;
    let paragraphs = load_ppt_paragraphs(file_path).map_err(PyRuntimeError::new_err)?;
    Ok(render_paragraphs_markdown(paragraphs))
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(ppt_to_markdown, m)?)?;
    Ok(())
}
