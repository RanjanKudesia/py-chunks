pub mod cfb_reader;
pub mod fib;
pub mod paragraph_props;
pub mod piece_table;
pub mod stream_iter;
pub mod structural;
pub mod stylesheet;
pub mod text_extractor;
pub mod to_markdown;

pub(crate) fn register(m: &pyo3::Bound<'_, pyo3::types::PyModule>) -> pyo3::PyResult<()> {
    structural::register(m)?;
    stream_iter::register(m)?;
    to_markdown::register(m)?;
    Ok(())
}
