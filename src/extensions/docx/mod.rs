pub mod common;
pub mod page_aware;
pub mod section;
pub mod semantic;
pub mod sentence;
pub mod sliding_window;
pub mod structural;

pub(crate) fn register(m: &pyo3::Bound<'_, pyo3::types::PyModule>) -> pyo3::PyResult<()> {
    structural::register(m)?;
    page_aware::register(m)?;
    section::register(m)?;
    sentence::register(m)?;
    semantic::register(m)?;
    sliding_window::register(m)?;
    Ok(())
}
