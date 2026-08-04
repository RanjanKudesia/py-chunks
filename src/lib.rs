mod engine;
mod engine_per_mode;
mod extensions;

use pyo3::prelude::*;
use pyo3::types::PyModule;

#[pymodule]
fn _rust(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Register each extension independently so new formats (e.g. PDF) stay isolated.
    extensions::docx::register(m)?;
    extensions::doc::register(m)?;
    extensions::csv::register(m)?;
    extensions::html::register(m)?;
    extensions::md::register(m)?;
    extensions::msg::register(m)?;
    extensions::eml::register(m)?;
    extensions::odf::register(m)?;
    extensions::json::register(m)?;
    extensions::epub::register(m)?;
    extensions::ipynb::register(m)?;
    extensions::rtf::register(m)?;
    extensions::pdf::register(m)?;
    extensions::ppt::register(m)?;
    extensions::pptx::register(m)?;
    extensions::txt::register(m)?;
    extensions::xlsx::register(m)?;
    Ok(())
}
