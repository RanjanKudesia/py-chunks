//! JSON family support: `.json` (single document), `.jsonl` / `.ndjson`
//! (line-delimited records). Records are rendered to markdown, then chunked via
//! the Markdown chunker. No new dependency (serde_json is already used).

pub mod chunkers;
pub mod extract;

use pyo3::prelude::*;
use pyo3::types::PyModule;

pub(crate) fn register(m: &pyo3::Bound<'_, PyModule>) -> PyResult<()> {
    chunkers::register(m)?;
    Ok(())
}
