//! MIME email support: `.eml` (single RFC822 message) and `.mbox` (mailbox of
//! messages). Parsed via `mail-parser`, assembled into markdown, then chunked
//! through the Markdown chunker — mirroring the `.msg` pipeline.

pub mod chunkers;
pub mod extract;
pub mod mbox;

use pyo3::prelude::*;
use pyo3::types::PyModule;

pub(crate) fn register(m: &pyo3::Bound<'_, PyModule>) -> PyResult<()> {
    chunkers::register(m)?;
    Ok(())
}
