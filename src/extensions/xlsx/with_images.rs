//! Spreadsheet image extraction. Images are read from the OOXML `xl/media/`
//! parts, so this covers `.xlsx`/`.xlsm`/`.xlsb`/`.xltx`/`.xltm`/`.ods`.
//!
//! Known limitation: legacy binary `.xls` (BIFF8) is NOT covered — its images
//! live in the OLE `Workbook` stream as MSODRAWING/Escher blip records, which
//! would need a dedicated BIFF8 Escher decoder. Deferred as genuinely additive:
//! images in `.xls` are rare and no real `.xls`-with-image fixture exists in our
//! corpus to ground/test an implementation. Text extraction for `.xls` is full.

use calamine::Reader;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyModule};
use pyo3::wrap_pyfunction;
use pythonize::pythonize;
use serde_json::json;

use super::common::{build_row_chunks, XlsxChunkRecord};
use super::images::collect_spreadsheet_images;
use super::page_aware::build_page_aware_chunks;
use super::row_document::map_build_error;
use super::semantic::build_semantic_chunks;
use super::sheet::build_sheet_chunks;
use super::sliding_window::build_sliding_window_chunks;
use super::table_region::build_table_chunks;

#[pyfunction]
#[pyo3(signature = (file_path, mode, rows_per_chunk, window_size, overlap, include_headers, sheet_names, skip_empty_rows, max_chunk_chars))]
pub fn chunk_xlsx_with_images(
    py: Python<'_>,
    file_path: &str,
    mode: &str,
    rows_per_chunk: usize,
    window_size: usize,
    overlap: usize,
    include_headers: bool,
    sheet_names: Vec<String>,
    skip_empty_rows: bool,
    max_chunk_chars: usize,
) -> PyResult<(Vec<PyObject>, Vec<(String, Py<PyBytes>)>)> {
    if !super::common::is_supported_spreadsheet(file_path) {
        return Err(PyValueError::new_err(format!(
            "Expected a spreadsheet file ({}), got: {file_path}",
            super::common::supported_spreadsheet_exts_display()
        )));
    }

    if rows_per_chunk == 0 {
        return Err(PyValueError::new_err("rows_per_chunk must be > 0"));
    }
    if max_chunk_chars == 0 {
        return Err(PyValueError::new_err("max_chunk_chars must be > 0"));
    }
    if mode == "sliding_window" {
        if window_size == 0 {
            return Err(PyValueError::new_err("window_size must be >= 1"));
        }
        if overlap >= window_size {
            return Err(PyValueError::new_err("overlap must be < window_size"));
        }
    }

    let workbook = super::common::open_spreadsheet(file_path)
        .map_err(pyo3::exceptions::PyIOError::new_err)?;
    let all_sheet_names = workbook.sheet_names().to_vec();

    let normalized_mode = if mode == "default" { "row" } else { mode };

    let text_records: Vec<XlsxChunkRecord> = match normalized_mode {
        "row" => build_row_chunks(
            file_path,
            rows_per_chunk,
            include_headers,
            sheet_names.clone(),
            skip_empty_rows,
        ),
        "table" => build_table_chunks(
            file_path,
            include_headers,
            sheet_names.clone(),
            skip_empty_rows,
            max_chunk_chars,
        ),
        "sheet" => build_sheet_chunks(
            file_path,
            include_headers,
            sheet_names.clone(),
            skip_empty_rows,
            max_chunk_chars,
        ),
        "sliding_window" => build_sliding_window_chunks(
            file_path,
            window_size,
            overlap,
            include_headers,
            sheet_names.clone(),
            skip_empty_rows,
        ),
        "page_aware" => build_page_aware_chunks(
            file_path,
            include_headers,
            sheet_names.clone(),
            skip_empty_rows,
            max_chunk_chars,
        ),
        "semantic" => build_semantic_chunks(
            file_path,
            rows_per_chunk,
            include_headers,
            sheet_names.clone(),
            skip_empty_rows,
        ),
        _ => return Err(PyValueError::new_err(format!("Unknown mode: {mode}"))),
    }
    .map_err(map_build_error)?;

    let mut image_out: Vec<(String, Vec<u8>)> = Vec::new();
    let image_infos = collect_spreadsheet_images(file_path, &all_sheet_names, &mut image_out);

    let mut chunk_list: Vec<PyObject> = image_infos
        .into_iter()
        .map(|info| {
            let dict = PyDict::new_bound(py);
            dict.set_item("content", &info.hash_name)?;
            dict.set_item("content_type", "image")?;
            dict.set_item(
                "metadata",
                pythonize(
                    py,
                    &json!({
                        "sheet_name": info.sheet_name,
                        "sheet_index": info.sheet_index,
                        "image_name": info.hash_name,
                        "alt_text": info.alt_text,
                    }),
                )?,
            )?;
            Ok(dict.into_any().unbind())
        })
        .collect::<PyResult<_>>()?;

    let text_chunks: Vec<PyObject> = text_records
        .into_iter()
        .map(|rec| {
            let dict = PyDict::new_bound(py);
            dict.set_item("content", &rec.content)?;
            dict.set_item("content_type", &rec.content_type)?;
            dict.set_item("metadata", pythonize(py, &rec.metadata)?)?;
            Ok(dict.into_any().unbind())
        })
        .collect::<PyResult<_>>()?;

    chunk_list.extend(text_chunks);

    let image_out_py: Vec<(String, Py<PyBytes>)> = image_out
        .into_iter()
        .map(|(name, bytes)| (name, PyBytes::new_bound(py, &bytes).unbind()))
        .collect();

    Ok((chunk_list, image_out_py))
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(chunk_xlsx_with_images, m)?)?;
    Ok(())
}
