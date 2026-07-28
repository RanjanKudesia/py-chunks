//! Outlook `.msg` (MS-OXMSG) support. Extracts subject/from/to/body from the CFB
//! property streams, then chunks via the Markdown chunker.

pub mod chunkers;
pub mod extract;
pub mod nameid;
pub mod rtf;

use pyo3::prelude::*;
use pyo3::types::PyModule;

pub(crate) fn register(m: &pyo3::Bound<'_, PyModule>) -> PyResult<()> {
    chunkers::register(m)?;
    Ok(())
}
