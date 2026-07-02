use std::io::Read;

/// Reads the "WordDocument" stream from the CFB container.
/// Returns raw bytes; caller parses the FIB from these bytes.
pub fn read_word_document_stream(file_path: &str) -> Result<Vec<u8>, String> {
    let mut compound = cfb::open(file_path)
        .map_err(|e| format!("Cannot open .doc file (invalid CFB format): {e}"))?;
    let mut buf = Vec::new();
    compound
        .open_stream("/WordDocument")
        .map_err(|_| "Missing WordDocument stream — not a valid .doc file".to_string())?
        .read_to_end(&mut buf)
        .map_err(|e| format!("Failed to read WordDocument stream: {e}"))?;
    Ok(buf)
}

/// Reads the table stream ("0Table" or "1Table") chosen by `which` (0 or 1).
pub fn read_table_stream(file_path: &str, which: u8) -> Result<Vec<u8>, String> {
    let stream_name = if which == 1 { "/1Table" } else { "/0Table" };
    let mut compound = cfb::open(file_path).map_err(|e| format!("Cannot open .doc file: {e}"))?;
    let mut buf = Vec::new();
    compound
        .open_stream(stream_name)
        .map_err(|_| format!("Missing {stream_name} stream — not a valid .doc file"))?
        .read_to_end(&mut buf)
        .map_err(|e| format!("Failed to read {stream_name}: {e}"))?;
    Ok(buf)
}

/// Reads the optional "Data" stream, which stores inline picture data (PICF
/// structures referenced by sprmCPicLocation). Returns `None` when the
/// stream is absent — a document without inline pictures is not an error.
pub fn read_data_stream(file_path: &str) -> Result<Option<Vec<u8>>, String> {
    let mut compound = cfb::open(file_path).map_err(|e| format!("Cannot open .doc file: {e}"))?;
    let mut stream = match compound.open_stream("/Data") {
        Ok(s) => s,
        Err(_) => return Ok(None),
    };
    let mut buf = Vec::new();
    stream
        .read_to_end(&mut buf)
        .map_err(|e| format!("Failed to read Data stream: {e}"))?;
    Ok(Some(buf))
}
