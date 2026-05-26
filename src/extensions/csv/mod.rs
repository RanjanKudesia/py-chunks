pub mod chunker;
pub mod common;
pub mod sliding_window;
pub mod stream_iter;

pub(crate) fn register(m: &pyo3::Bound<'_, pyo3::types::PyModule>) -> pyo3::PyResult<()> {
    chunker::register(m)?;
    sliding_window::register(m)?;
    stream_iter::register(m)?;
    Ok(())
}