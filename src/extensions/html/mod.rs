pub mod common;
pub mod page_aware;
pub mod section;
pub mod semantic;
pub mod sentence;
pub mod sliding_window;
pub mod stream_iter;
pub mod structural;
pub mod to_markdown;

pub(crate) fn register(m: &pyo3::Bound<'_, pyo3::types::PyModule>) -> pyo3::PyResult<()> {
    structural::register(m)?;
    semantic::register(m)?;
    section::register(m)?;
    sliding_window::register(m)?;
    sentence::register(m)?;
    page_aware::register(m)?;
    stream_iter::register(m)?;
    to_markdown::register(m)?;
    Ok(())
}
