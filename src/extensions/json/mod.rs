//! JSON / JSONL / NDJSON support.
//!
//! **Migrated to the vendored engine** (`chunks_rs::formats::json`); this module
//! is the PyO3 binding only. See CONSOLIDATION_PLAN.md.

pub mod chunkers;

use pyo3::prelude::*;
use pyo3::types::PyModule;

pub(crate) fn register(m: &pyo3::Bound<'_, PyModule>) -> PyResult<()> {
    chunkers::register(m)?;
    Ok(())
}
