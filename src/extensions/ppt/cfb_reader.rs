use std::io::Read;

/// Reads the "PowerPoint Document" stream from the CFB (OLE) container of a
/// legacy `.ppt` file. Returns raw bytes; the caller walks the record tree.
pub fn read_powerpoint_document_stream(file_path: &str) -> Result<Vec<u8>, String> {
    let mut compound = cfb::open(file_path)
        .map_err(|e| format!("Cannot open .ppt file (invalid CFB format): {e}"))?;
    let mut buf = Vec::new();
    compound
        .open_stream("/PowerPoint Document")
        .map_err(|_| {
            "Missing 'PowerPoint Document' stream — not a valid .ppt file".to_string()
        })?
        .read_to_end(&mut buf)
        .map_err(|e| format!("Failed to read PowerPoint Document stream: {e}"))?;
    Ok(buf)
}
