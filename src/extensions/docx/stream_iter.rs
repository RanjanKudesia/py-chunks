use pyo3::prelude::*;
use pyo3::types::PyDict;
use quick_xml::events::Event;
use quick_xml::Reader;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::io::Cursor;
use std::io::Read;
use zip::ZipArchive;

#[pyclass]
pub struct DocxTrueStreamIterator {
    reader: Option<Reader<Cursor<Vec<u8>>>>,
    buf: Vec<u8>,
    in_paragraph: bool,
    in_text: bool,
    in_table: bool,
    in_cell: bool,
    in_num_pr: bool,
    current_para_style: String,
    current_para_text: String,
    current_table_rows: Vec<String>,
    current_table_cells: Vec<String>,
    current_cell_text: String,
    current_section_heading: Option<String>,
    current_section_parts: Vec<String>,
    pending_bullets: Vec<String>,
    outside_short_parts: Vec<String>,
    ready_chunks: VecDeque<(String, String)>,
    doc_metadata: Value,
    done: bool,
}

fn qname_eq(name: &[u8], target: &[u8]) -> bool {
    if let Some(pos) = name.iter().position(|&b| b == b':') {
        &name[pos + 1..] == target
    } else {
        name == target
    }
}

fn extract_metadata_from_archive(archive: &mut ZipArchive<Cursor<Vec<u8>>>) -> Value {
    let mut header_text = None;
    let mut footer_text = None;
    let mut image_count = 0;

    if let Ok(mut header_file) = archive.by_name("word/header1.xml") {
        let mut buf = Vec::new();
        let _ = header_file.read_to_end(&mut buf);
        if let Ok(text) = extract_text_from_xml(&buf) {
            header_text = Some(text);
        }
    }

    if let Ok(mut footer_file) = archive.by_name("word/footer1.xml") {
        let mut buf = Vec::new();
        let _ = footer_file.read_to_end(&mut buf);
        if let Ok(text) = extract_text_from_xml(&buf) {
            footer_text = Some(text);
        }
    }

    for i in 0..archive.len() {
        if let Ok(file) = archive.by_index(i) {
            if file.name().starts_with("word/media/") {
                image_count += 1;
            }
        }
    }

    json!({
        "header_text": header_text,
        "footer_text": footer_text,
        "image_count": image_count,
    })
}

fn extract_text_from_xml(xml: &[u8]) -> Result<String, String> {
    let mut reader = Reader::from_reader(xml);
    let mut result = String::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Text(e)) => {
                let txt = String::from_utf8_lossy(e.as_ref()).to_string();
                result.push_str(&txt);
                result.push(' ');
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    Ok(result.trim().to_string())
}

impl DocxTrueStreamIterator {
    fn push_chunk(&mut self, content: String, content_type: &str) {
        self.ready_chunks
            .push_back((content, content_type.to_string()));
    }

    fn process_paragraph(&mut self) {
        if self.current_para_text.is_empty() {
            return;
        }

        let text = self.current_para_text.trim().to_string();
        let is_heading = self.current_para_style.contains("Heading");
        let is_code = self.current_para_style.contains("Code");

        if is_heading {
            if !self.outside_short_parts.is_empty() {
                let combined = self.outside_short_parts.join(" ");
                self.outside_short_parts.clear();
                self.push_chunk(combined, "short_disconnected_paragraph");
            }

            if let Some(heading) = self.current_section_heading.take() {
                if !self.current_section_parts.is_empty() {
                    let combined =
                        format!("{}\n{}", heading, self.current_section_parts.join("\n"));
                    self.current_section_parts.clear();
                    self.push_chunk(combined, "mixed_content");
                }
            }

            self.current_section_heading = Some(text.clone());
            self.current_section_parts.push(text);
        } else if is_code {
            if !self.outside_short_parts.is_empty() {
                let combined = self.outside_short_parts.join(" ");
                self.outside_short_parts.clear();
                self.push_chunk(combined, "short_disconnected_paragraph");
            }

            self.push_chunk(text, "code_block");
        } else if self.in_num_pr {
            self.pending_bullets.push(text);
        } else if text.len() < 100 {
            if self.current_section_heading.is_some() {
                self.current_section_parts.push(text);
            } else {
                self.outside_short_parts.push(text);
            }
        } else {
            if self.current_section_heading.is_some() {
                self.current_section_parts.push(text);
            } else {
                self.push_chunk(text, "plain_paragraph");
            }
        }
    }

    fn flush_table(&mut self) {
        if self.current_table_rows.is_empty() {
            return;
        }

        if !self.outside_short_parts.is_empty() {
            let combined = self.outside_short_parts.join(" ");
            self.outside_short_parts.clear();
            self.push_chunk(combined, "short_disconnected_paragraph");
        }

        let table_content = self.current_table_rows.join("\n");
        self.push_chunk(table_content, "table");
        self.current_table_rows.clear();
    }

    fn flush_final(&mut self) {
        if !self.pending_bullets.is_empty() {
            let combined = self.pending_bullets.join("\n");
            self.pending_bullets.clear();
            self.push_chunk(combined, "bullet_numbered_list");
        }

        if let Some(heading) = self.current_section_heading.take() {
            if !self.current_section_parts.is_empty() {
                let combined = format!("{}\n{}", heading, self.current_section_parts.join("\n"));
                self.current_section_parts.clear();
                self.push_chunk(combined, "mixed_content");
            }
        }

        if !self.outside_short_parts.is_empty() {
            let combined = self.outside_short_parts.join(" ");
            self.outside_short_parts.clear();
            self.push_chunk(combined, "short_disconnected_paragraph");
        }
    }

    fn read_next_event(&mut self) -> Result<Event<'static>, String> {
        if let Some(ref mut reader) = self.reader {
            match reader.read_event_into(&mut self.buf) {
                Ok(event) => {
                    let owned_event = match event {
                        Event::Text(t) => Event::Text(t.into_owned()),
                        Event::Start(s) => Event::Start(s.into_owned()),
                        Event::End(e) => Event::End(e.into_owned()),
                        Event::Eof => Event::Eof,
                        _ => Event::Eof,
                    };
                    Ok(owned_event)
                }
                Err(_) => Err("Read error".to_string()),
            }
        } else {
            Err("No reader".to_string())
        }
    }
}

#[pymethods]
impl DocxTrueStreamIterator {
    fn __iter__(slf: PyRef<Self>) -> PyRef<Self> {
        slf
    }

    fn __next__(mut slf: PyRefMut<Self>, py: Python) -> Option<PyObject> {
        if let Some((content, content_type)) = slf.ready_chunks.pop_front() {
            let dict = PyDict::new_bound(py);
            let _ = dict.set_item("content", &content);
            let _ = dict.set_item("content_type", &content_type);
            let _ = dict.set_item("metadata", slf.doc_metadata.to_string());
            return Some(dict.into_any().unbind());
        }

        if slf.done {
            return None;
        }

        loop {
            if let Some((content, content_type)) = slf.ready_chunks.pop_front() {
                let dict = PyDict::new_bound(py);
                let _ = dict.set_item("content", &content);
                let _ = dict.set_item("content_type", &content_type);
                let _ = dict.set_item("metadata", slf.doc_metadata.to_string());
                return Some(dict.into_any().unbind());
            }

            if slf.reader.is_none() {
                slf.done = true;
                return None;
            }

            let event = match slf.read_next_event() {
                Ok(e) => e,
                Err(_) => {
                    slf.done = true;
                    return None;
                }
            };

            match event {
                Event::Start(e) => {
                    let name = e.name().as_ref().to_vec();
                    if qname_eq(&name, b"p") {
                        slf.in_paragraph = true;
                    } else if qname_eq(&name, b"t") && slf.in_paragraph {
                        slf.in_text = true;
                    } else if qname_eq(&name, b"tbl") {
                        slf.in_table = true;
                    } else if qname_eq(&name, b"tr") && slf.in_table {
                        slf.current_table_cells.clear();
                    } else if qname_eq(&name, b"tc") && slf.in_table {
                        slf.in_cell = true;
                        slf.current_cell_text.clear();
                    } else if qname_eq(&name, b"t") && slf.in_cell && slf.in_table {
                        slf.in_text = true;
                    } else if qname_eq(&name, b"numPr") && slf.in_paragraph {
                        slf.in_num_pr = true;
                    } else if qname_eq(&name, b"pStyle") && slf.in_paragraph {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"w:val" {
                                let val_str = String::from_utf8_lossy(&attr.value).into_owned();
                                slf.current_para_style = val_str;
                                break;
                            }
                        }
                    }
                }
                Event::Text(e) => {
                    let txt = String::from_utf8_lossy(e.as_ref()).to_string();
                    if slf.in_text && slf.in_paragraph && !slf.in_cell {
                        slf.current_para_text.push_str(&txt);
                        slf.current_para_text.push(' ');
                    } else if slf.in_text && slf.in_cell && slf.in_table {
                        slf.current_cell_text.push_str(&txt);
                        slf.current_cell_text.push(' ');
                    }
                }
                Event::End(e) => {
                    let name = e.name().as_ref().to_vec();
                    if qname_eq(&name, b"p") && slf.in_paragraph {
                        slf.process_paragraph();
                        if !slf.ready_chunks.is_empty() {
                            if let Some((content, content_type)) = slf.ready_chunks.pop_front() {
                                let dict = PyDict::new_bound(py);
                                let _ = dict.set_item("content", &content);
                                let _ = dict.set_item("content_type", &content_type);
                                let _ = dict.set_item("metadata", slf.doc_metadata.to_string());
                                slf.in_paragraph = false;
                                slf.in_text = false;
                                slf.current_para_text.clear();
                                slf.current_para_style.clear();
                                slf.buf.clear();
                                return Some(dict.into_any().unbind());
                            }
                        }
                        slf.in_paragraph = false;
                        slf.in_text = false;
                        slf.current_para_text.clear();
                        slf.current_para_style.clear();
                    } else if qname_eq(&name, b"tr") && slf.in_table {
                        if !slf.current_table_cells.is_empty() {
                            let row = slf.current_table_cells.join(" | ");
                            slf.current_table_rows.push(row);
                        }
                        slf.current_table_cells.clear();
                    } else if qname_eq(&name, b"tc") && slf.in_table {
                        let cell = slf.current_cell_text.trim().to_string();
                        if !cell.is_empty() {
                            slf.current_table_cells.push(cell);
                        }
                        slf.current_cell_text.clear();
                        slf.in_cell = false;
                    } else if qname_eq(&name, b"tbl") && slf.in_table {
                        slf.flush_table();
                        if !slf.ready_chunks.is_empty() {
                            if let Some((content, content_type)) = slf.ready_chunks.pop_front() {
                                let dict = PyDict::new_bound(py);
                                let _ = dict.set_item("content", &content);
                                let _ = dict.set_item("content_type", &content_type);
                                let _ = dict.set_item("metadata", slf.doc_metadata.to_string());
                                slf.in_table = false;
                                slf.buf.clear();
                                return Some(dict.into_any().unbind());
                            }
                        }
                        slf.in_table = false;
                    } else if qname_eq(&name, b"t") {
                        slf.in_text = false;
                    }
                }
                Event::Eof => {
                    slf.done = true;
                    slf.reader = None;
                    slf.flush_final();
                    if let Some((content, content_type)) = slf.ready_chunks.pop_front() {
                        let dict = PyDict::new_bound(py);
                        let _ = dict.set_item("content", &content);
                        let _ = dict.set_item("content_type", &content_type);
                        let _ = dict.set_item("metadata", slf.doc_metadata.to_string());
                        return Some(dict.into_any().unbind());
                    }
                    return None;
                }
                _ => {}
            }
        }
    }
}

#[pyfunction]
pub fn chunk_docx_true_stream(file_path: &str) -> PyResult<DocxTrueStreamIterator> {
    use pyo3::exceptions::{PyIOError, PyRuntimeError};

    let data = std::fs::read(file_path)
        .map_err(|e| PyIOError::new_err(format!("Failed to read DOCX: {}", e)))?;

    let cursor = Cursor::new(data);
    let mut archive = ZipArchive::new(cursor)
        .map_err(|e| PyRuntimeError::new_err(format!("Not a valid DOCX: {}", e)))?;

    let mut xml_bytes = Vec::new();
    {
        let mut doc = archive
            .by_name("word/document.xml")
            .map_err(|_| PyRuntimeError::new_err("word/document.xml not found".to_string()))?;
        doc.read_to_end(&mut xml_bytes)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to read document.xml: {}", e)))?;
    }

    let doc_metadata = extract_metadata_from_archive(&mut archive);

    let cursor = Cursor::new(xml_bytes);
    let mut reader = Reader::from_reader(cursor);
    reader.config_mut().trim_text(true);

    Ok(DocxTrueStreamIterator {
        reader: Some(reader),
        buf: Vec::new(),
        in_paragraph: false,
        in_text: false,
        in_table: false,
        in_cell: false,
        in_num_pr: false,
        current_para_style: String::new(),
        current_para_text: String::new(),
        current_table_rows: Vec::new(),
        current_table_cells: Vec::new(),
        current_cell_text: String::new(),
        current_section_heading: None,
        current_section_parts: Vec::new(),
        pending_bullets: Vec::new(),
        outside_short_parts: Vec::new(),
        ready_chunks: VecDeque::new(),
        doc_metadata,
        done: false,
    })
}

pub(crate) fn register(m: &pyo3::Bound<'_, pyo3::types::PyModule>) -> pyo3::PyResult<()> {
    use pyo3::wrap_pyfunction;
    m.add_function(wrap_pyfunction!(chunk_docx_true_stream, m)?)?;
    m.add_class::<DocxTrueStreamIterator>()?;
    Ok(())
}
