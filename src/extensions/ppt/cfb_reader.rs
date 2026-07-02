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

/// Reads the optional "Pictures" stream, which stores every embedded image as
/// a sequence of OfficeArtBlip records. Returns `None` when the stream is
/// absent (a presentation without images) — that is not an error.
pub fn read_pictures_stream(file_path: &str) -> Result<Option<Vec<u8>>, String> {
    let mut compound = cfb::open(file_path)
        .map_err(|e| format!("Cannot open .ppt file (invalid CFB format): {e}"))?;
    let mut stream = match compound.open_stream("/Pictures") {
        Ok(s) => s,
        Err(_) => return Ok(None),
    };
    let mut buf = Vec::new();
    stream
        .read_to_end(&mut buf)
        .map_err(|e| format!("Failed to read Pictures stream: {e}"))?;
    Ok(Some(buf))
}
