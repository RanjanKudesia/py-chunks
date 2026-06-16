pub mod common;
pub mod images;
pub mod stream_iter;
pub mod structural;
pub mod to_markdown;
pub mod with_images;

use pyo3::prelude::*;
use pyo3::types::PyModule;

// Register the pdf module
pub(crate) fn register(m: &pyo3::Bound<'_, PyModule>) -> PyResult<()> {
    structural::register(m)?;
    stream_iter::register(m)?;
    to_markdown::register(m)?;
    with_images::register(m)?;
    Ok(())
}
