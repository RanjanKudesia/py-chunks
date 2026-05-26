mod extensions;

use pyo3::prelude::*;
use pyo3::types::PyModule;

#[pymodule]
fn _rust(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Register each extension independently so new formats (e.g. PDF) stay isolated.
    extensions::docx::register(m)?;
    extensions::csv::register(m)?;
    extensions::html::register(m)?;
    extensions::md::register(m)?;
    extensions::pdf::register(m)?;
    extensions::pptx::register(m)?;
    extensions::txt::register(m)?;
    extensions::xlsx::register(m)?;
    Ok(())
}
