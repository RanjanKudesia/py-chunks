//! Plain-text (.txt) support.
//!
//! **Migrated to the vendored engine** (`chunks_rs::formats::txt`); this module
//! is the PyO3 binding only. See CONSOLIDATION_PLAN.md.

pub mod chunkers;

pub(crate) fn register(m: &pyo3::Bound<'_, pyo3::types::PyModule>) -> pyo3::PyResult<()> {
    chunkers::register(m)
}
