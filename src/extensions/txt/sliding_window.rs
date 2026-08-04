use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};
use pyo3::wrap_pyfunction;
use pythonize::pythonize;
use serde_json::json;
use std::fs;
use std::time::Instant;

use super::common::{
    current_section_heading, extract_heading_text, heading_level_txt, heading_path_strings,
    parse_txt_blocks, update_heading_stack, ChunkRecordInput, ContentType,
};

struct BlockUnit {
    content: String,
    section_heading: Option<String>,
    heading_path: Vec<String>,
}

pub fn build_sliding_window_chunks(
    bytes: &[u8],
    window_size: usize,
    overlap: usize,
) -> Result<Vec<ChunkRecordInput>, String> {
    if window_size == 0 { return Err("window_size must be greater than 0".to_string()); }
    if overlap >= window_size { return Err("overlap must be less than window_size".to_string()); }

    let text = crate::extensions::text_encoding::decode_text(bytes).0;
    if text.trim().is_empty() { return Err("TXT file is empty".to_string()); }

    let blocks = parse_txt_blocks(&text);
    let total = blocks.len();
    let mut heading_stack: Vec<(u8, String)> = Vec::new();
    let mut units: Vec<BlockUnit> = Vec::new();

    for block in &blocks {
        let content = if block.content_type == ContentType::HeadingSection {
            let level = heading_level_txt(&block.content);
            let text = extract_heading_text(&block.content);
            update_heading_stack(&mut heading_stack, level, text.clone());
            text
        } else {
            block.content.trim().to_string()
        };
        if content.is_empty() { continue; }
        units.push(BlockUnit {
            content,
            section_heading: current_section_heading(&heading_stack),
            heading_path: heading_path_strings(&heading_stack),
        });
    }

    if units.is_empty() { return Err("No content blocks found".to_string()); }

    let step = window_size - overlap;
    let mut result: Vec<ChunkRecordInput> = Vec::new();
    let mut start = 0usize;
    let mut window_index = 0usize;

    loop {
        let end = (start + window_size).min(units.len());
        let window = &units[start..end];
        let content = window.iter().map(|u| u.content.as_str()).collect::<Vec<_>>().join("\n\n");
        if !content.is_empty() {
            result.push(ChunkRecordInput {
                content_type: ContentType::SlidingWindow,
                content,
                metadata: json!({
                    "window_size":      window_size,
                    "overlap":          overlap,
                    "window_index":     window_index,
                    "paragraph_range":  [start, end.saturating_sub(1)],
                    "paragraph_count":  window.len(),
                    "section_heading":  window[0].section_heading,
                    "heading_path":     window[0].heading_path,
                    "chunk_index":      window_index,
                    "document_metadata": { "source_type": "txt", "total_input_blocks": total }
                }),
            });
            window_index += 1;
        }
        if end >= units.len() { break; }
        start += step;
    }

    if result.is_empty() { return Err("No sliding-window chunks generated".to_string()); }
    Ok(result)
}

#[pyfunction]
pub fn chunk_txt_sliding_window(
    py: Python<'_>,
    file_path: &str,
    window_size: usize,
    overlap: usize,
) -> PyResult<PyObject> {
    if !file_path.to_ascii_lowercase().ends_with(".txt") {
        return Err(PyValueError::new_err(format!("Expected .txt file path, got: {file_path}")));
    }
    if window_size == 0 { return Err(PyValueError::new_err("window_size must be greater than 0")); }
    if overlap >= window_size { return Err(PyValueError::new_err("overlap must be less than window_size")); }
    let bytes = fs::read(file_path)
        .map_err(|e| PyIOError::new_err(format!("Failed to read TXT file: {e}")))?;
    let rust_start = Instant::now();
    let chunks_raw = build_sliding_window_chunks(&bytes, window_size, overlap)
        .map_err(|e| PyRuntimeError::new_err(format!("TXT sliding-window failed: {e}")))?;
    let rust_ms = rust_start.elapsed().as_secs_f64() * 1000.0;
    let chunk_list: Vec<PyObject> = chunks_raw.into_iter().map(|c| {
        let dict = PyDict::new_bound(py);
        dict.set_item("content", &c.content)?;
        dict.set_item("content_type", c.content_type.as_str())?;
        dict.set_item("metadata", pythonize(py, &c.metadata)?)?;
        Ok(dict.into_any().unbind())
    }).collect::<PyResult<_>>()?;
    let result = PyDict::new_bound(py);
    result.set_item("chunks", chunk_list)?;
    result.set_item("rust_ms", (rust_ms * 1000.0).round() / 1000.0)?;
    Ok(result.into_any().unbind())
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(chunk_txt_sliding_window, m)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn four_para_text() -> &'static str {
        "First paragraph content.\n\nSecond paragraph content.\n\nThird paragraph content.\n\nFourth paragraph."
    }

    #[test]
    fn window_size_zero_returns_error() {
        assert!(build_sliding_window_chunks(b"text", 0, 0).is_err());
    }

    #[test]
    fn overlap_gte_window_returns_error() {
        assert!(build_sliding_window_chunks(b"text", 3, 3).is_err());
        assert!(build_sliding_window_chunks(b"text", 3, 4).is_err());
    }

    #[test]
    fn empty_input_returns_error() {
        assert!(build_sliding_window_chunks(b"", 2, 1).is_err());
    }

    #[test]
    fn window_1_overlap_0_produces_one_chunk_per_block() {
        let text = four_para_text();
        let chunks = build_sliding_window_chunks(text.as_bytes(), 1, 0).unwrap();
        assert_eq!(chunks.len(), 4);
    }

    #[test]
    fn window_2_overlap_1_produces_correct_count() {
        // 4 blocks, window=2, step=1 → windows: [0,1], [1,2], [2,3] = 3
        let text = four_para_text();
        let chunks = build_sliding_window_chunks(text.as_bytes(), 2, 1).unwrap();
        assert_eq!(chunks.len(), 3);
    }

    #[test]
    fn window_larger_than_blocks_produces_one_chunk() {
        let text = four_para_text();
        let chunks = build_sliding_window_chunks(text.as_bytes(), 10, 1).unwrap();
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn chunk_metadata_contains_window_params() {
        let text = four_para_text();
        let chunks = build_sliding_window_chunks(text.as_bytes(), 2, 1).unwrap();
        assert_eq!(chunks[0].metadata["window_size"], 2);
        assert_eq!(chunks[0].metadata["overlap"], 1);
    }

    #[test]
    fn window_index_increments() {
        let text = four_para_text();
        let chunks = build_sliding_window_chunks(text.as_bytes(), 1, 0).unwrap();
        for (i, c) in chunks.iter().enumerate() {
            assert_eq!(c.metadata["window_index"], i as u64);
        }
    }

    #[test]
    fn content_type_is_sliding_window() {
        let text = four_para_text();
        let chunks = build_sliding_window_chunks(text.as_bytes(), 2, 1).unwrap();
        assert!(chunks.iter().all(|c| c.content_type.as_str() == "sliding_window"));
    }

    #[test]
    fn adjacent_windows_share_overlap_content() {
        // With overlap=1, the last block of window N is the first block of window N+1
        let text = four_para_text();
        let chunks = build_sliding_window_chunks(text.as_bytes(), 2, 1).unwrap();
        // Window 0 ends with "Second paragraph content." and window 1 starts with it
        assert!(chunks[0].content.contains("Second paragraph"));
        assert!(chunks[1].content.contains("Second paragraph"));
    }
}
