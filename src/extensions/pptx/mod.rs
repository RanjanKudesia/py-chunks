//! PowerPoint OOXML (`.pptx` / `.potx` / `.potm` / `.ppsx` / `.ppsm`) support.
//!
//! **Migrated to the vendored engine** (`chunks_rs::formats::pptx`); this module
//! is the PyO3 binding only. See CONSOLIDATION_PLAN.md.

pub mod chunkers;

pub(crate) fn register(m: &pyo3::Bound<'_, pyo3::types::PyModule>) -> pyo3::PyResult<()> {
    chunkers::register_chunkers(m)?;
    chunkers::register_streams(m)?;
    chunkers::register_images(m)?;
    chunkers::register_markdown(m)?;
    chunkers::register_aliases(m)?;
    Ok(())
}
