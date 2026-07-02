//! Image extraction for legacy `.ppt` files.
//!
//! Image bytes live in the optional "Pictures" CFB stream as back-to-back
//! OfficeArtBlip records. The blip store (OfficeArtBStoreContainer, inside
//! the DggContainer of the "PowerPoint Document" stream) lists one FBSE per
//! image whose `foDelay` is the record's byte offset in the Pictures stream.
//! Shapes on a slide reference images through the `Pib` shape property — a
//! 1-based index into the blip store.
//!
//! Slide attribution follows the same rule the text extractor uses for
//! freeform text: SlideContainer drawings are matched positionally to the
//! extracted slide list only when the counts agree; otherwise images are
//! emitted without a slide number rather than risking misattribution.

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyModule};
use pyo3::wrap_pyfunction;
use pythonize::pythonize;
use serde_json::json;

use crate::extensions::doc::structural::{
    build_page_aware_chunks, build_section_chunks, build_semantic_chunks, build_sentence_chunks,
    build_sliding_window_chunks, build_structural_chunks, ChunkRecord,
};
use crate::extensions::doc::text_extractor::DocParagraph;
use crate::extensions::odraw::{
    blip_hash_name, decode_blip_record_at, decode_fbse_blip, find_record, is_blip_record,
    parse_odraw_header, DecodedBlip, RT_BSTORE_CONTAINER, RT_FBSE,
};

use super::cfb_reader;
use super::records::{parse_header, RT_MAIN_MASTER, RT_NOTES_CONTAINER, RT_SLIDE_CONTAINER,
    REC_VER_CONTAINER};
use super::structural::{load_ppt_paragraphs, validate_ppt_path};
use super::text_extractor;

/// One image occurrence in the presentation. `slide_idx` is a 0-based index
/// into the slide list returned by `text_extractor::extract_slides`, or
/// `None` when the image could not be attributed to a slide.
#[derive(Debug, Clone)]
pub struct PptImage {
    pub hash_name: String,
    pub bytes: Vec<u8>,
    pub slide_idx: Option<usize>,
}

/// Blip-store entries decoded to images (index = 0-based store position).
fn decode_bstore(doc_stream: &[u8], pictures: Option<&[u8]>) -> Vec<Option<DecodedBlip>> {
    let Some(bstore) = find_record(doc_stream, 0, doc_stream.len(), RT_BSTORE_CONTAINER) else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    let mut pos = bstore.body_start;
    while let Some((hdr, next)) = parse_odraw_header(doc_stream, pos, bstore.body_end) {
        if next <= pos {
            break;
        }
        let body = &doc_stream[hdr.body_start..hdr.body_end];
        let decoded = if hdr.rec_type == RT_FBSE {
            decode_fbse_blip(body, pictures)
        } else if is_blip_record(hdr.rec_type) {
            decode_blip_record_at(doc_stream, pos)
        } else {
            None
        };
        entries.push(decoded);
        pos = next;
    }
    entries
}

/// Collect `Pib` blip-store references per SlideContainer, in document order.
fn collect_slide_pibs(data: &[u8], start: usize, end: usize, out: &mut Vec<Vec<u32>>) {
    let mut pos = start;
    while let Some((hdr, next)) = parse_header(data, pos, end) {
        if next <= pos {
            break;
        }
        match hdr.rec_type {
            RT_SLIDE_CONTAINER => {
                let mut pibs = Vec::new();
                crate::extensions::odraw::collect_pib_values(
                    data,
                    hdr.body_start,
                    hdr.body_end,
                    &mut pibs,
                );
                out.push(pibs);
            }
            RT_NOTES_CONTAINER | RT_MAIN_MASTER => {}
            _ => {
                if hdr.rec_ver == REC_VER_CONTAINER {
                    collect_slide_pibs(data, hdr.body_start, hdr.body_end, out);
                }
            }
        }
        pos = next;
    }
}

/// Decode every blip record found by walking the Pictures stream
/// sequentially. Fallback when no slide/blip-store association is possible.
fn decode_pictures_stream(pictures: &[u8]) -> Vec<DecodedBlip> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while let Some((hdr, next)) = parse_odraw_header(pictures, pos, pictures.len()) {
        if next <= pos {
            break;
        }
        if let Some(decoded) = decode_blip_record_at(pictures, pos) {
            out.push(decoded);
        } else if hdr.rec_type == RT_FBSE {
            // Some writers store the FBSE header inline in the Pictures
            // stream with the blip record embedded right after it.
            if let Some(decoded) =
                decode_fbse_blip(&pictures[hdr.body_start..hdr.body_end], None)
            {
                out.push(decoded);
            }
        }
        pos = next;
    }
    out
}

/// Extract all slide images from a `.ppt` file. One entry per image
/// occurrence on a slide (master/notes images are excluded); duplicates
/// across slides share the same `hash_name`.
pub fn extract_ppt_images(file_path: &str) -> Result<Vec<PptImage>, String> {
    let doc_stream = cfb_reader::read_powerpoint_document_stream(file_path)?;
    let pictures = cfb_reader::read_pictures_stream(file_path)?;

    let bstore = decode_bstore(&doc_stream, pictures.as_deref());

    let mut slide_pibs: Vec<Vec<u32>> = Vec::new();
    collect_slide_pibs(&doc_stream, 0, doc_stream.len(), &mut slide_pibs);

    // Drawing order maps 1:1 onto the extracted slide list only when the
    // counts agree (the rule the text extractor uses for freeform merge).
    let slide_count = text_extractor::extract_slides(&doc_stream).len();
    let aligned = slide_pibs.len() == slide_count;

    let mut out: Vec<PptImage> = Vec::new();
    for (drawing_idx, pibs) in slide_pibs.iter().enumerate() {
        for pib in pibs {
            let idx = (*pib as usize).wrapping_sub(1);
            if let Some(Some(decoded)) = bstore.get(idx) {
                out.push(PptImage {
                    hash_name: blip_hash_name(&decoded.bytes, decoded.ext),
                    bytes: decoded.bytes.clone(),
                    slide_idx: if aligned { Some(drawing_idx) } else { None },
                });
            }
        }
    }

    // Fallback: no slide association was possible at all, but the file does
    // carry pictures — emit them unanchored instead of dropping them.
    if out.is_empty() {
        if let Some(ref pics) = pictures {
            for decoded in decode_pictures_stream(pics) {
                out.push(PptImage {
                    hash_name: blip_hash_name(&decoded.bytes, decoded.ext),
                    bytes: decoded.bytes,
                    slide_idx: None,
                });
            }
        }
    }

    Ok(out)
}

/// Build the `(chunks, images)` tuple shared by every `_with_images` mode:
/// image chunks first (the pptx convention), then the text chunks produced
/// by `build`, with chunk indices renumbered across the combined list.
fn chunk_ppt_with_images_impl(
    py: Python<'_>,
    file_path: &str,
    build: impl FnOnce(Vec<DocParagraph>) -> Vec<ChunkRecord>,
) -> PyResult<(Vec<PyObject>, Vec<(String, Py<PyBytes>)>)> {
    validate_ppt_path(file_path)?;
    let paragraphs = load_ppt_paragraphs(file_path).map_err(PyRuntimeError::new_err)?;
    // Image extraction must never break text chunking: degrade to no images.
    let images = extract_ppt_images(file_path).unwrap_or_default();
    let text_chunks = build(paragraphs);

    let total = images.len() + text_chunks.len();
    let mut chunk_list: Vec<PyObject> = Vec::with_capacity(total);
    let mut image_out: Vec<(String, Vec<u8>)> = Vec::new();

    for (i, img) in images.iter().enumerate() {
        if !image_out.iter().any(|(n, _)| n == &img.hash_name) {
            image_out.push((img.hash_name.clone(), img.bytes.clone()));
        }
        let dict = PyDict::new_bound(py);
        dict.set_item("content", &img.hash_name)?;
        dict.set_item("content_type", "image")?;
        dict.set_item(
            "metadata",
            pythonize(
                py,
                &json!({
                    "source": file_path,
                    "chunk_index": i,
                    "total_chunks": total,
                    "paragraph_type": "image",
                    "heading_level": serde_json::Value::Null,
                    "page_number": img.slide_idx.map(|s| s + 1),
                    "image_name": img.hash_name,
                }),
            )?,
        )?;
        chunk_list.push(dict.into_any().unbind());
    }

    let offset = images.len();
    for chunk in &text_chunks {
        let dict = PyDict::new_bound(py);
        dict.set_item("content", &chunk.content)?;
        dict.set_item("content_type", chunk.content_type)?;
        dict.set_item(
            "metadata",
            pythonize(
                py,
                &json!({
                    "source": file_path,
                    "chunk_index": chunk.chunk_index + offset,
                    "total_chunks": total,
                    "paragraph_type": chunk.paragraph_type,
                    "heading_level": chunk.heading_level,
                    "page_number": serde_json::Value::Null,
                }),
            )?,
        )?;
        chunk_list.push(dict.into_any().unbind());
    }

    let image_out_py = image_out
        .into_iter()
        .map(|(name, data)| (name, PyBytes::new_bound(py, &data).unbind()))
        .collect();
    Ok((chunk_list, image_out_py))
}

#[pyfunction]
fn chunk_ppt_structural_with_images(
    py: Python<'_>,
    file_path: &str,
) -> PyResult<(Vec<PyObject>, Vec<(String, Py<PyBytes>)>)> {
    chunk_ppt_with_images_impl(py, file_path, build_structural_chunks)
}

#[pyfunction]
fn chunk_ppt_section_with_images(
    py: Python<'_>,
    file_path: &str,
) -> PyResult<(Vec<PyObject>, Vec<(String, Py<PyBytes>)>)> {
    chunk_ppt_with_images_impl(py, file_path, build_section_chunks)
}

#[pyfunction]
fn chunk_ppt_semantic_with_images(
    py: Python<'_>,
    file_path: &str,
) -> PyResult<(Vec<PyObject>, Vec<(String, Py<PyBytes>)>)> {
    chunk_ppt_with_images_impl(py, file_path, build_semantic_chunks)
}

#[pyfunction]
fn chunk_ppt_sliding_window_with_images(
    py: Python<'_>,
    file_path: &str,
    window_size: usize,
    overlap: usize,
) -> PyResult<(Vec<PyObject>, Vec<(String, Py<PyBytes>)>)> {
    use pyo3::exceptions::PyValueError;
    if window_size == 0 {
        return Err(PyValueError::new_err("window_size must be greater than 0"));
    }
    if overlap >= window_size {
        return Err(PyValueError::new_err("overlap must be less than window_size"));
    }
    chunk_ppt_with_images_impl(py, file_path, move |p| {
        build_sliding_window_chunks(p, window_size, overlap)
    })
}

#[pyfunction]
fn chunk_ppt_sentence_with_images(
    py: Python<'_>,
    file_path: &str,
    sentences_per_chunk: usize,
) -> PyResult<(Vec<PyObject>, Vec<(String, Py<PyBytes>)>)> {
    use pyo3::exceptions::PyValueError;
    if sentences_per_chunk == 0 {
        return Err(PyValueError::new_err(
            "sentences_per_chunk must be greater than 0",
        ));
    }
    chunk_ppt_with_images_impl(py, file_path, move |p| {
        build_sentence_chunks(p, sentences_per_chunk)
    })
}

#[pyfunction]
fn chunk_ppt_page_aware_with_images(
    py: Python<'_>,
    file_path: &str,
    paragraphs_per_page: usize,
) -> PyResult<(Vec<PyObject>, Vec<(String, Py<PyBytes>)>)> {
    use pyo3::exceptions::PyValueError;
    if paragraphs_per_page == 0 {
        return Err(PyValueError::new_err(
            "paragraphs_per_page must be greater than 0",
        ));
    }
    chunk_ppt_with_images_impl(py, file_path, move |p| {
        build_page_aware_chunks(p, paragraphs_per_page)
    })
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(chunk_ppt_structural_with_images, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_ppt_section_with_images, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_ppt_semantic_with_images, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_ppt_sliding_window_with_images, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_ppt_sentence_with_images, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_ppt_page_aware_with_images, m)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extensions::odraw::{FBSE_FO_DELAY_OFFSET, FBSE_HEADER_LEN};

    const PNG_MAGIC: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

    fn record(ver_inst: u16, rec_type: u16, body: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(8 + body.len());
        out.extend_from_slice(&ver_inst.to_le_bytes());
        out.extend_from_slice(&rec_type.to_le_bytes());
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(body);
        out
    }

    fn png_blip_record() -> Vec<u8> {
        let mut body = vec![0u8; 17];
        body.extend_from_slice(&PNG_MAGIC);
        body.extend_from_slice(b"data");
        record(0x6E0 << 4, crate::extensions::odraw::RT_BLIP_PNG, &body)
    }

    #[test]
    fn decode_bstore_resolves_delay_stream_blip() {
        let mut pictures = vec![0u8; 4];
        let fo_delay = pictures.len() as u32;
        pictures.extend_from_slice(&png_blip_record());

        let mut fbse = vec![0u8; FBSE_HEADER_LEN];
        fbse[FBSE_FO_DELAY_OFFSET..FBSE_FO_DELAY_OFFSET + 4]
            .copy_from_slice(&fo_delay.to_le_bytes());
        let fbse_rec = record(0, RT_FBSE, &fbse);
        let bstore = record((1 << 4) | 0xF, RT_BSTORE_CONTAINER, &fbse_rec);
        // Wrap in a container so find_record has to descend.
        let dgg = record(0xF, crate::extensions::odraw::RT_DGG_CONTAINER, &bstore);

        let entries = decode_bstore(&dgg, Some(&pictures));
        assert_eq!(entries.len(), 1);
        assert!(entries[0].is_some());
        assert_eq!(entries[0].as_ref().unwrap().ext, ".png");
    }

    #[test]
    fn collect_slide_pibs_groups_by_slide_container() {
        // Slide container holding an OPT record with a Pib property.
        let mut opt_body = Vec::new();
        opt_body.extend_from_slice(&0x4104u16.to_le_bytes());
        opt_body.extend_from_slice(&1u32.to_le_bytes());
        let opt = record((1 << 4) | 0x3, crate::extensions::odraw::RT_OPT, &opt_body);
        let slide = record(0xF, RT_SLIDE_CONTAINER, &opt);

        let mut pibs = Vec::new();
        collect_slide_pibs(&slide, 0, slide.len(), &mut pibs);
        assert_eq!(pibs, vec![vec![1]]);
    }

    #[test]
    fn decode_pictures_stream_walks_records() {
        let mut stream = png_blip_record();
        stream.extend_from_slice(&png_blip_record());
        let decoded = decode_pictures_stream(&stream);
        assert_eq!(decoded.len(), 2);
    }
}
