pub mod common;
pub mod images;
pub mod repair;
pub mod page_aware;
pub mod row_document;
pub mod semantic;
pub mod sheet;
pub mod sliding_window;
pub mod stream_iter;
pub mod table_region;
pub mod to_markdown;
pub mod with_images;

pub(crate) fn register(m: &pyo3::Bound<'_, pyo3::types::PyModule>) -> pyo3::PyResult<()> {
    page_aware::register(m)?;
    row_document::register(m)?;
    semantic::register(m)?;
    sheet::register(m)?;
    sliding_window::register(m)?;
    table_region::register(m)?;
    stream_iter::register(m)?;
    to_markdown::register(m)?;
    with_images::register(m)?;
    Ok(())
}
