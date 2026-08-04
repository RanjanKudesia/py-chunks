/// Real streaming iterator for all XLSX chunking modes.
///
/// Memory profile by mode:
///   row / sliding_window — true state machines: O(parsed_rows) for the
///     pre-parsed SheetData; only one XlsxChunkRecord is built per __next__.
///   table / sheet / page_aware / semantic — batch-drain: all chunks are
///     built upfront (global analysis required), then converted to Python
///     dicts lazily one per __next__.
///
/// Parity guarantee: yields identical content and metadata to the
/// corresponding batch function for every mode.
use calamine::{Data, Reader};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};
use pyo3::wrap_pyfunction;
use pythonize::pythonize;
use serde_json::json;

use super::common::{
    cell_to_string, detect_header_row, row_is_empty_public, serialize_row_kv,
    serialize_row_values_public, XlsxChunkRecord, CT_ROW, CT_SLIDING_WINDOW,
};
use super::page_aware::build_page_aware_chunks;
use super::row_document::map_build_error;
use super::semantic::build_semantic_chunks;
use super::sheet::build_sheet_chunks;
use super::table_region::build_table_chunks;

// ── Chunk → Python dict ───────────────────────────────────────────────────────

fn chunk_to_pydict(py: Python<'_>, chunk: &XlsxChunkRecord) -> PyResult<PyObject> {
    let dict = PyDict::new_bound(py);
    dict.set_item("content", &chunk.content)?;
    dict.set_item("content_type", &chunk.content_type)?;
    dict.set_item("metadata", pythonize(py, &chunk.metadata)?)?;
    Ok(dict.into_any().unbind())
}

// ── Pre-parsed sheet data (row / sliding_window state machines) ───────────────

struct SheetData {
    sheet_name: String,
    sheet_index: usize,
    headers: Vec<String>,
    col_count: usize,
    /// (absolute_row_index, owned_cells_padded_to_col_count)
    data_rows: Vec<(usize, Vec<Data>)>,
}

fn row_slice_owned(row: &[Data], col_count: usize) -> Vec<Data> {
    (0..col_count)
        .map(|i| row.get(i).cloned().unwrap_or(Data::Empty))
        .collect()
}

fn build_headers_from_rows(
    rows: &[&[Data]],
    header_row_index: Option<usize>,
    col_count: usize,
) -> Vec<String> {
    (0..col_count)
        .map(|idx| {
            let h = header_row_index
                .and_then(|ri| rows.get(ri))
                .and_then(|row| row.get(idx))
                .map(cell_to_string)
                .unwrap_or_default();
            if h.trim().is_empty() {
                format!("Column {}", idx + 1)
            } else {
                h
            }
        })
        .collect()
}

/// Open the workbook once and collect all row data per sheet.
/// Used by row and sliding_window state machines.
fn parse_sheets_for_streaming(
    file_path: &str,
    sheet_names: Vec<String>,
    skip_empty_rows: bool,
) -> Result<Vec<SheetData>, String> {
    let mut workbook =
        super::common::open_spreadsheet(file_path)?;

    let workbook_sheet_names = workbook.sheet_names().to_vec();
    let selected_sheets = if sheet_names.is_empty() {
        workbook_sheet_names.clone()
    } else {
        for name in &sheet_names {
            if !workbook_sheet_names.iter().any(|n| n == name) {
                return Err(format!("Sheet '{name}' not found"));
            }
        }
        sheet_names
    };

    let mut result = Vec::new();
    let mut readable_sheets = 0usize;
    let mut first_sheet_error: Option<String> = None;
    for sheet_name in selected_sheets {
        let sheet_index = workbook_sheet_names
            .iter()
            .position(|n| n == &sheet_name)
            .unwrap_or(0);

        // A sheet calamine cannot read (chart sheets, XLM macro sheets) must not
        // take the whole workbook down with it — skip it and keep going.
        let range = match super::common::read_worksheet_range(&mut workbook, &sheet_name) {
            Ok(range) => {
                readable_sheets += 1;
                range
            }
            Err(e) => {
                first_sheet_error.get_or_insert(e);
                continue;
            }
        };
        let base_row_index = range.start().map(|(r, _)| r as usize).unwrap_or(0);

        let rows: Vec<&[Data]> = range.rows().collect();
        if rows.is_empty() {
            continue;
        }

        let col_count = rows.iter().map(|r| r.len()).max().unwrap_or(0);
        if col_count == 0 {
            continue;
        }

        let header_row_index = detect_header_row(&rows);
        let headers = build_headers_from_rows(&rows, header_row_index, col_count);
        let mut data_start = header_row_index.map_or(0, |i| i + 1);

        // F2 guard (parity with batch build_row_chunks): if every row was consumed
        // as the header (no data rows follow), fall back to emitting the header row
        // as content rather than silently dropping the sheet.
        let has_data_rows = rows
            .iter()
            .skip(data_start)
            .any(|row| !(skip_empty_rows && row_is_empty_public(&row_slice_owned(row, col_count))));
        if !has_data_rows {
            if let Some(hidx) = header_row_index {
                data_start = hidx;
            }
        }

        let mut data_rows: Vec<(usize, Vec<Data>)> = Vec::new();
        for (row_index, row) in rows.iter().enumerate().skip(data_start) {
            let cells = row_slice_owned(row, col_count);
            if skip_empty_rows && row_is_empty_public(&cells) {
                continue;
            }
            data_rows.push((base_row_index + row_index, cells));
        }

        if data_rows.is_empty() {
            continue;
        }

        result.push(SheetData {
            sheet_name,
            sheet_index,
            headers,
            col_count,
            data_rows,
        });
    }

    // Every selected sheet failed to read: this is not an empty workbook,
    // it is an unreadable one — surface the first failure rather than
    // returning success with no chunks.
    if readable_sheets == 0 {
        if let Some(e) = first_sheet_error {
            return Err(e);
        }
    }
    Ok(result)
}

// ── Row state machine ─────────────────────────────────────────────────────────

struct RowStreamState {
    sheets: Vec<SheetData>,
    sheet_idx: usize,
    row_cursor: usize,
    rows_per_chunk: usize,
    include_headers: bool,
    chunk_index: usize,
}

impl RowStreamState {
    fn advance(&mut self) -> Option<XlsxChunkRecord> {
        loop {
            if self.sheet_idx >= self.sheets.len() {
                return None;
            }

            let sheet_len = self.sheets[self.sheet_idx].data_rows.len();
            if self.row_cursor >= sheet_len {
                self.sheet_idx += 1;
                self.row_cursor = 0;
                self.chunk_index = 0; // build_row_chunks resets chunk_index per sheet
                continue;
            }

            let end = (self.row_cursor + self.rows_per_chunk).min(sheet_len);

            // Collect chunk data while sheet is borrowed, then mutate self.
            let (content, first_row_index, actual_row_count, sheet_name, sheet_index, headers, col_count) = {
                let sheet = &self.sheets[self.sheet_idx];
                let group = &sheet.data_rows[self.row_cursor..end];
                let include_headers = self.include_headers;
                let content = group
                    .iter()
                    .map(|(_, cells)| {
                        if include_headers {
                            serialize_row_kv(&sheet.headers, cells)
                        } else {
                            serialize_row_values_public(cells, sheet.col_count)
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                let first_row_index = group[0].0;
                let actual_row_count = group.len();
                (
                    content,
                    first_row_index,
                    actual_row_count,
                    sheet.sheet_name.clone(),
                    sheet.sheet_index,
                    sheet.headers.clone(),
                    sheet.col_count,
                )
            };

            let chunk_index = self.chunk_index;
            let rows_per_chunk = self.rows_per_chunk;
            self.row_cursor = end;
            self.chunk_index += 1;

            return Some(XlsxChunkRecord {
                content,
                content_type: CT_ROW.to_string(),
                metadata: json!({
                    "sheet_name": sheet_name,
                    "sheet_index": sheet_index,
                    "row_index": first_row_index,
                    "header_row": headers,
                    "col_count": col_count,
                    "rows_per_chunk": rows_per_chunk,
                    "actual_row_count": actual_row_count,
                    "chunk_index": chunk_index,
                }),
            });
        }
    }
}

// ── Sliding-window state machine ──────────────────────────────────────────────

struct SlidingWindowStreamState {
    sheets: Vec<SheetData>,
    sheet_idx: usize,
    window_start: usize,
    window_index: usize, // resets per sheet (mirrors build_sliding_window_chunks)
    window_size: usize,
    overlap: usize,
    include_headers: bool,
    chunk_index: usize,
}

impl SlidingWindowStreamState {
    fn advance(&mut self) -> Option<XlsxChunkRecord> {
        let step = self.window_size - self.overlap;
        loop {
            if self.sheet_idx >= self.sheets.len() {
                return None;
            }

            let sheet_len = self.sheets[self.sheet_idx].data_rows.len();
            if self.window_start >= sheet_len {
                self.sheet_idx += 1;
                self.window_start = 0;
                self.window_index = 0;
                continue;
            }

            let end = (self.window_start + self.window_size).min(sheet_len);

            let (content, start_row, end_row, actual_row_count, sheet_name, sheet_index, headers, col_count) = {
                let sheet = &self.sheets[self.sheet_idx];
                let window = &sheet.data_rows[self.window_start..end];
                let include_headers = self.include_headers;
                let content = window
                    .iter()
                    .map(|(_, cells)| {
                        if include_headers {
                            serialize_row_kv(&sheet.headers, cells)
                        } else {
                            serialize_row_values_public(cells, sheet.col_count)
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                let start_row = window.first().map(|(i, _)| *i).unwrap_or(0);
                let end_row = window.last().map(|(i, _)| *i).unwrap_or(start_row);
                let actual_row_count = window.len();
                (
                    content,
                    start_row,
                    end_row,
                    actual_row_count,
                    sheet.sheet_name.clone(),
                    sheet.sheet_index,
                    sheet.headers.clone(),
                    sheet.col_count,
                )
            };

            let chunk_index = self.chunk_index;
            let window_index = self.window_index;
            let window_size = self.window_size;
            let overlap = self.overlap;
            self.window_start += step;
            self.window_index += 1;
            self.chunk_index += 1;

            return Some(XlsxChunkRecord {
                content,
                content_type: CT_SLIDING_WINDOW.to_string(),
                metadata: json!({
                    "sheet_name": sheet_name,
                    "sheet_index": sheet_index,
                    "window_size": window_size,
                    "overlap": overlap,
                    "actual_row_count": actual_row_count,
                    "window_index": window_index,
                    "start_row": start_row,
                    "end_row": end_row,
                    "header_row": headers,
                    "col_count": col_count,
                    "chunk_index": chunk_index,
                }),
            });
        }
    }
}

// ── Batch-drain (table / sheet / page_aware / semantic) ───────────────────────

struct BatchDrainState {
    chunks: Vec<XlsxChunkRecord>,
    index: usize,
}

impl BatchDrainState {
    fn new(chunks: Vec<XlsxChunkRecord>) -> Self {
        BatchDrainState { chunks, index: 0 }
    }

    fn advance(&mut self) -> Option<&XlsxChunkRecord> {
        if self.index >= self.chunks.len() {
            return None;
        }
        let c = &self.chunks[self.index];
        self.index += 1;
        Some(c)
    }
}

// ── Backend enum ──────────────────────────────────────────────────────────────

enum XlsxStreamBackend {
    Row(RowStreamState),
    SlidingWindow(SlidingWindowStreamState),
    Batch(BatchDrainState),
}

// ── Iterator pyclass ──────────────────────────────────────────────────────────

#[pyclass]
pub struct XlsxStreamIterator {
    backend: XlsxStreamBackend,
}

#[pymethods]
impl XlsxStreamIterator {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(&mut self, py: Python<'_>) -> PyResult<Option<PyObject>> {
        match &mut self.backend {
            XlsxStreamBackend::Row(s) => match s.advance() {
                None => Ok(None),
                Some(chunk) => chunk_to_pydict(py, &chunk).map(Some),
            },
            XlsxStreamBackend::SlidingWindow(s) => match s.advance() {
                None => Ok(None),
                Some(chunk) => chunk_to_pydict(py, &chunk).map(Some),
            },
            XlsxStreamBackend::Batch(s) => match s.advance() {
                None => Ok(None),
                Some(chunk) => chunk_to_pydict(py, chunk).map(Some),
            },
        }
    }
}

// ── Public factory function ───────────────────────────────────────────────────

#[pyfunction]
pub fn stream_xlsx_chunks(
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
) -> PyResult<Py<XlsxStreamIterator>> {
    if !super::common::is_supported_spreadsheet(file_path) {
        return Err(PyValueError::new_err(format!(
            "Expected a spreadsheet file ({}), got: {file_path}",
            super::common::supported_spreadsheet_exts_display()
        )));
    }
    if (mode == "row" || mode == "semantic") && rows_per_chunk == 0 {
        return Err(PyValueError::new_err("rows_per_chunk must be > 0"));
    }
    if matches!(mode, "table" | "sheet" | "page_aware") && max_chunk_chars == 0 {
        return Err(PyValueError::new_err("max_chunk_chars must be > 0"));
    }

    let backend = match mode {
        "row" => {
            let sheets =
                parse_sheets_for_streaming(file_path, sheet_names, skip_empty_rows)
                    .map_err(map_build_error)?;
            XlsxStreamBackend::Row(RowStreamState {
                sheets,
                sheet_idx: 0,
                row_cursor: 0,
                rows_per_chunk,
                include_headers,
                chunk_index: 0,
            })
        }
        "sliding_window" => {
            if window_size == 0 {
                return Err(PyValueError::new_err("window_size must be >= 1"));
            }
            if overlap >= window_size {
                return Err(PyValueError::new_err("overlap must be < window_size"));
            }
            let sheets =
                parse_sheets_for_streaming(file_path, sheet_names, skip_empty_rows)
                    .map_err(map_build_error)?;
            XlsxStreamBackend::SlidingWindow(SlidingWindowStreamState {
                sheets,
                sheet_idx: 0,
                window_start: 0,
                window_index: 0,
                window_size,
                overlap,
                include_headers,
                chunk_index: 0,
            })
        }
        "table" => {
            let chunks = build_table_chunks(
                file_path,
                include_headers,
                sheet_names,
                skip_empty_rows,
                max_chunk_chars,
            )
            .map_err(map_build_error)?;
            XlsxStreamBackend::Batch(BatchDrainState::new(chunks))
        }
        "sheet" => {
            let chunks = build_sheet_chunks(
                file_path,
                include_headers,
                sheet_names,
                skip_empty_rows,
                max_chunk_chars,
            )
            .map_err(map_build_error)?;
            XlsxStreamBackend::Batch(BatchDrainState::new(chunks))
        }
        "page_aware" => {
            let chunks = build_page_aware_chunks(
                file_path,
                include_headers,
                sheet_names,
                skip_empty_rows,
                max_chunk_chars,
            )
            .map_err(map_build_error)?;
            XlsxStreamBackend::Batch(BatchDrainState::new(chunks))
        }
        "semantic" => {
            let chunks = build_semantic_chunks(
                file_path,
                rows_per_chunk,
                include_headers,
                sheet_names,
                skip_empty_rows,
            )
            .map_err(map_build_error)?;
            XlsxStreamBackend::Batch(BatchDrainState::new(chunks))
        }
        _ => {
            return Err(PyValueError::new_err(format!(
                "Unknown XLSX streaming mode: {mode}"
            )))
        }
    };

    Py::new(py, XlsxStreamIterator { backend })
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<XlsxStreamIterator>()?;
    m.add_function(wrap_pyfunction!(stream_xlsx_chunks, m)?)?;
    Ok(())
}
