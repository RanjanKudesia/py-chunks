//! OpenDocument Format support: `.odt` (text) and `.odp` (presentation). Both
//! are zip+XML (same container family as `.ods`/`.epub`). A shared `content.xml`
//! walker assembles markdown, then chunks through the Markdown chunker.

pub mod chunkers;
pub mod container;
pub mod text;

use pyo3::prelude::*;
use pyo3::types::PyModule;

pub(crate) fn register(m: &pyo3::Bound<'_, PyModule>) -> PyResult<()> {
    chunkers::register(m)?;
    Ok(())
}
