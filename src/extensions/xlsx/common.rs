use calamine::{open_workbook_auto, Data, Reader};
use quick_xml::events::Event;
use quick_xml::name::QName;
use quick_xml::Reader as XmlReader;
use serde_json::{json, Value};
use std::fs::File;
use std::io::Read;
use zip::ZipArchive;

pub const CT_ROW: &str = "row_document";
pub const CT_TABLE: &str = "table_region";
pub const CT_SHEET: &str = "sheet";
pub const CT_SLIDING_WINDOW: &str = "row_window";
pub const CT_PAGE_AWARE: &str = "sheet_region";
pub const CT_SEMANTIC: &str = "semantic_group";

#[derive(Debug, Clone)]
pub struct XlsxChunkRecord {
    pub content: String,
    pub content_type: String,
    pub metadata: Value,
}

/// Split a flat list of serialised row strings into char-limited groups.
/// A single line that already exceeds the limit is kept as its own group rather than
/// being silently truncated.
pub fn split_content_lines(lines: Vec<String>, max_chunk_chars: usize) -> Vec<Vec<String>> {
    if lines.is_empty() {
        return Vec::new();
    }
    let mut parts: Vec<Vec<String>> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    let mut current_len = 0usize;

    for line in lines {
        let sep = if current.is_empty() { 0 } else { 1 };
        if !current.is_empty() && current_len + sep + line.len() > max_chunk_chars {
            parts.push(std::mem::take(&mut current));
            current_len = line.len();
            current.push(line);
        } else {
            current_len += sep + line.len();
            current.push(line);
        }
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

pub fn cell_to_string(cell: &Data) -> String {
    match cell {
        Data::String(s) => s.clone(),
        Data::Float(f) => {
            if f.fract() == 0.0 {
                format!("{}", *f as i64)
            } else {
                format!("{:.4}", f)
                    .trim_end_matches('0')
                    .trim_end_matches('.')
                    .to_string()
            }
        }
        Data::Int(i) => i.to_string(),
        Data::Bool(b) => {
            if *b {
                "true".to_string()
            } else {
                "false".to_string()
            }
        }
        Data::Empty => String::new(),
        _ => String::new(),
    }
}

pub fn detect_header_row(rows: &[&[Data]]) -> Option<usize> {
    for (i, row) in rows.iter().enumerate() {
        let non_empty: Vec<_> = row.iter().filter(|c| !matches!(c, Data::Empty)).collect();
        if non_empty.is_empty() {
            continue;
        }
        if non_empty.iter().all(|c| matches!(c, Data::String(_))) {
            return Some(i);
        }
        // Numeric index column (e.g. 0, 1, 2…) followed by all-string labels → treat as header
        if non_empty.len() >= 2
            && matches!(non_empty[0], Data::Float(_) | Data::Int(_))
            && non_empty[1..].iter().all(|c| matches!(c, Data::String(_)))
        {
            return Some(i);
        }
    }
    None
}

pub fn col_letter_to_index(col: &str) -> usize {
    col.chars().fold(0usize, |acc, c| {
        acc * 26 + (c.to_ascii_uppercase() as usize - 'A' as usize + 1)
    }) - 1
}

pub fn parse_cell_ref(cell_ref: &str) -> Option<(usize, usize)> {
    let letters: String = cell_ref
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect();
    let digits: String = cell_ref
        .chars()
        .skip_while(|c| c.is_ascii_alphabetic())
        .collect();
    if letters.is_empty() || digits.is_empty() {
        return None;
    }
    let col = col_letter_to_index(&letters);
    let row = digits.parse::<usize>().ok()?.saturating_sub(1);
    Some((row, col))
}

pub fn parse_range_ref(range_ref: &str) -> Option<(usize, usize, usize, usize)> {
    let parts: Vec<&str> = range_ref.split(':').collect();
    if parts.len() != 2 {
        return None;
    }
    let (r1, c1) = parse_cell_ref(parts[0])?;
    let (r2, c2) = parse_cell_ref(parts[1])?;
    Some((r1.min(r2), c1.min(c2), r1.max(r2), c1.max(c2)))
}

fn read_zip_entry(archive: &mut ZipArchive<File>, name: &str) -> Result<Option<Vec<u8>>, String> {
    match archive.by_name(name) {
        Ok(mut entry) => {
            let mut buf = Vec::new();
            entry
                .read_to_end(&mut buf)
                .map_err(|e| format!("Failed to read '{name}': {e}"))?;
            Ok(Some(buf))
        }
        Err(zip::result::ZipError::FileNotFound) => Ok(None),
        Err(e) => Err(format!("Failed to open '{name}' in xlsx archive: {e}")),
    }
}

fn local_name(name: QName<'_>) -> Vec<u8> {
    let bytes = name.as_ref();
    let idx = bytes
        .iter()
        .rposition(|b| *b == b':')
        .map(|i| i + 1)
        .unwrap_or(0);
    bytes[idx..].to_vec()
}

fn attr_value(attr: &quick_xml::events::attributes::Attribute<'_>) -> String {
    String::from_utf8_lossy(attr.value.as_ref()).into_owned()
}

fn resolve_target(base_dir: &str, target: &str) -> String {
    if target.starts_with('/') {
        return target.trim_start_matches('/').to_string();
    }
    let mut parts: Vec<&str> = base_dir.split('/').collect();
    for segment in target.split('/') {
        match segment {
            ".." => {
                parts.pop();
            }
            "." | "" => {}
            s => parts.push(s),
        }
    }
    parts.join("/")
}

fn parse_table_relationship_targets(rels_xml: &[u8]) -> Result<Vec<String>, String> {
    let mut reader = XmlReader::from_reader(rels_xml);
    let mut buf = Vec::new();
    let mut targets = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("Failed to parse worksheet relationships XML: {e}")),
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                if local_name(e.name()).as_slice() == b"Relationship" {
                    let mut rel_type = String::new();
                    let mut target = String::new();
                    for attr in e.attributes().flatten() {
                        let key = local_name(QName(attr.key.as_ref()));
                        if key.as_slice() == b"Type" {
                            rel_type = attr_value(&attr);
                        } else if key.as_slice() == b"Target" {
                            target = attr_value(&attr);
                        }
                    }
                    if rel_type.ends_with("/table") && !target.is_empty() {
                        targets.push(target);
                    }
                }
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(targets)
}

fn parse_table_name(table_xml: &[u8]) -> Result<Option<String>, String> {
    let mut reader = XmlReader::from_reader(table_xml);
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("Failed to parse table XML: {e}")),
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                if local_name(e.name()).as_slice() == b"table" {
                    let mut table_name: Option<String> = None;
                    let mut display_name: Option<String> = None;
                    for attr in e.attributes().flatten() {
                        let key = local_name(QName(attr.key.as_ref()));
                        let value = attr_value(&attr);
                        if key.as_slice() == b"name" {
                            table_name = Some(value);
                        } else if key.as_slice() == b"displayName" {
                            display_name = Some(value);
                        }
                    }
                    return Ok(table_name.or(display_name));
                }
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(None)
}

pub fn get_named_table_names_for_sheet(
    file_path: &str,
    sheet_index_1based: usize,
) -> Result<Vec<String>, String> {
    let zip_file = File::open(file_path).map_err(|e| format!("Failed to open file: {e}"))?;
    let mut archive = match ZipArchive::new(zip_file) {
        Ok(a) => a,
        Err(_) => return Ok(Vec::new()), // Not a ZIP archive (e.g. XLS) — no named tables
    };

    let rels_path = format!("xl/worksheets/_rels/sheet{}.xml.rels", sheet_index_1based);
    let Some(rels_xml) = read_zip_entry(&mut archive, &rels_path)? else {
        return Ok(Vec::new());
    };

    let targets = parse_table_relationship_targets(&rels_xml)?;
    let mut names = Vec::new();
    for target in targets {
        let full_path = resolve_target("xl/worksheets", &target);
        let Some(table_xml) = read_zip_entry(&mut archive, &full_path)? else {
            continue;
        };
        if let Some(name) = parse_table_name(&table_xml)? {
            names.push(name);
        }
    }

    Ok(names)
}

#[derive(Debug, Clone)]
pub struct DataRegion {
    pub start_row: usize,
    pub end_row: usize,
    pub start_col: usize,
    pub end_col: usize,
}

pub fn detect_contiguous_regions(rows: &[&[Data]], base_row: usize) -> Vec<DataRegion> {
    let mut regions = Vec::new();

    let mut current_start: Option<usize> = None;
    let mut current_min_col = usize::MAX;
    let mut current_max_col = 0usize;

    for (row_idx, row) in rows.iter().enumerate() {
        let mut first_non_empty: Option<usize> = None;
        let mut last_non_empty: Option<usize> = None;
        for (col_idx, cell) in row.iter().enumerate() {
            if !matches!(cell, Data::Empty) {
                if first_non_empty.is_none() {
                    first_non_empty = Some(col_idx);
                }
                last_non_empty = Some(col_idx);
            }
        }

        match (first_non_empty, last_non_empty) {
            (Some(first), Some(last)) => {
                if current_start.is_none() {
                    current_start = Some(row_idx);
                    current_min_col = first;
                    current_max_col = last;
                } else {
                    current_min_col = current_min_col.min(first);
                    current_max_col = current_max_col.max(last);
                }
            }
            _ => {
                if let Some(start) = current_start {
                    let end = row_idx.saturating_sub(1);
                    regions.push(DataRegion {
                        start_row: base_row + start,
                        end_row: base_row + end,
                        start_col: current_min_col,
                        end_col: current_max_col,
                    });
                    current_start = None;
                    current_min_col = usize::MAX;
                    current_max_col = 0;
                }
            }
        }
    }

    if let Some(start) = current_start {
        let end = rows.len().saturating_sub(1);
        regions.push(DataRegion {
            start_row: base_row + start,
            end_row: base_row + end,
            start_col: current_min_col,
            end_col: current_max_col,
        });
    }

    regions
}

pub fn serialize_row_kv(headers: &[String], cells: &[Data]) -> String {
    (0..headers.len())
        .map(|idx| {
            let value = cells.get(idx).map(cell_to_string).unwrap_or_default();
            format!("{}: {}", headers[idx], value)
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

fn row_is_empty(row: &[Data]) -> bool {
    row.iter().all(|cell| matches!(cell, Data::Empty))
}

pub fn row_is_empty_public(row: &[Data]) -> bool {
    row_is_empty(row)
}

fn build_headers(
    rows: &[&[Data]],
    header_row_index: Option<usize>,
    col_count: usize,
) -> Vec<String> {
    let mut headers = Vec::with_capacity(col_count);
    for idx in 0..col_count {
        let header = header_row_index
            .and_then(|row_index| rows.get(row_index))
            .and_then(|row| row.get(idx))
            .map(cell_to_string)
            .unwrap_or_default();
        if header.trim().is_empty() {
            headers.push(format!("Column {}", idx + 1));
        } else {
            headers.push(header);
        }
    }
    headers
}

fn serialize_row_values(cells: &[Data], col_count: usize) -> String {
    (0..col_count)
        .map(|idx| cells.get(idx).map(cell_to_string).unwrap_or_default())
        .collect::<Vec<_>>()
        .join(" | ")
}

pub fn serialize_row_values_public(cells: &[Data], col_count: usize) -> String {
    serialize_row_values(cells, col_count)
}

fn build_chunk_content(
    grouped_rows: &[(usize, &[Data])],
    headers: &[String],
    include_headers: bool,
    col_count: usize,
) -> String {
    grouped_rows
        .iter()
        .map(|(_, row)| {
            if include_headers {
                serialize_row_kv(headers, row)
            } else {
                serialize_row_values(row, col_count)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn build_row_chunks(
    file_path: &str,
    rows_per_chunk: usize,
    include_headers: bool,
    sheet_names: Vec<String>,
    skip_empty_rows: bool,
) -> Result<Vec<XlsxChunkRecord>, String> {
    let mut workbook =
        open_workbook_auto(file_path).map_err(|e| format!("Failed to open workbook: {e}"))?;

    let workbook_sheet_names = workbook.sheet_names().to_vec();
    let selected_sheets = if sheet_names.is_empty() {
        workbook_sheet_names.clone()
    } else {
        for sheet_name in &sheet_names {
            if !workbook_sheet_names.iter().any(|name| name == sheet_name) {
                return Err(format!("Sheet '{sheet_name}' not found"));
            }
        }
        sheet_names
    };

    let mut chunks = Vec::new();
    for sheet_name in selected_sheets {
        let sheet_index = workbook_sheet_names
            .iter()
            .position(|name| name == &sheet_name)
            .unwrap_or(0);

        let range = workbook
            .worksheet_range(&sheet_name)
            .map_err(|e| format!("Failed to read sheet '{sheet_name}': {e}"))?;
        let base_row_index = range.start().map(|(row, _)| row as usize).unwrap_or(0);

        let rows: Vec<&[Data]> = range.rows().collect();
        if rows.is_empty() {
            continue;
        }

        let header_row_index = detect_header_row(&rows);
        let start_row_index = header_row_index.map_or(0, |idx| idx + 1);
        let col_count = rows.iter().map(|row| row.len()).max().unwrap_or(0);
        if col_count == 0 {
            continue;
        }
        let headers = build_headers(&rows, header_row_index, col_count);

        let mut pending_rows: Vec<(usize, &[Data])> = Vec::new();
        let mut chunk_index = 0usize;

        for (row_index, row) in rows.iter().enumerate().skip(start_row_index) {
            if skip_empty_rows && row_is_empty(row) {
                continue;
            }

            pending_rows.push((base_row_index + row_index, row));
            if pending_rows.len() == rows_per_chunk {
                let content =
                    build_chunk_content(&pending_rows, &headers, include_headers, col_count);
                let first_row_index = pending_rows[0].0;
                let actual_row_count = pending_rows.len();
                chunks.push(XlsxChunkRecord {
                    content,
                    content_type: CT_ROW.to_string(),
                    metadata: json!({
                        "sheet_name": sheet_name.clone(),
                        "sheet_index": sheet_index,
                        "row_index": first_row_index,
                        "header_row": headers.clone(),
                        "col_count": col_count,
                        "rows_per_chunk": rows_per_chunk,
                        "actual_row_count": actual_row_count,
                        "chunk_index": chunk_index,
                    }),
                });
                pending_rows.clear();
                chunk_index += 1;
            }
        }

        if !pending_rows.is_empty() {
            let content = build_chunk_content(&pending_rows, &headers, include_headers, col_count);
            let first_row_index = pending_rows[0].0;
            let actual_row_count = pending_rows.len();
            chunks.push(XlsxChunkRecord {
                content,
                content_type: CT_ROW.to_string(),
                metadata: json!({
                    "sheet_name": sheet_name.clone(),
                    "sheet_index": sheet_index,
                    "row_index": first_row_index,
                    "header_row": headers.clone(),
                    "col_count": col_count,
                    "rows_per_chunk": rows_per_chunk,
                    "actual_row_count": actual_row_count,
                    "chunk_index": chunk_index,
                }),
            });
        }
    }

    Ok(chunks)
}
