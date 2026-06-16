// src/extensions/pdf/images.rs
//
// Image extraction for PDF documents. PDF images are page-scoped raster
// XObjects rather than text-inline elements, so each extracted image is
// associated with the 1-based page number on which it appears (mirroring the
// slide-scoped model used by the PPTX extractor).

use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Cursor;

use image::ImageFormat;
use pdfium_render::prelude::*;

#[derive(Debug, Clone)]
pub struct PdfImageInfo {
    pub hash_name: String,
    pub page_number: usize,
}

/// Encode a decoded bitmap to PNG bytes and derive a content-addressed file
/// name `"{16hexchars}.png"`. Returns None if encoding fails.
fn encode_png(img: &image::DynamicImage) -> Option<(String, Vec<u8>)> {
    let mut buf: Vec<u8> = Vec::new();
    img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
        .ok()?;
    if buf.is_empty() {
        return None;
    }
    let mut hasher = DefaultHasher::new();
    buf.hash(&mut hasher);
    let hash_name = format!("{:016x}.png", hasher.finish());
    Some((hash_name, buf))
}

/// Walk every page of `doc`, decode each image XObject to PNG, and return
/// `(per-occurrence infos, deduplicated (name, bytes) pairs)`.
///
/// The same image referenced on multiple pages yields one info entry per page
/// occurrence but a single entry in `image_out` (deduplicated by content hash),
/// matching the package-wide dedup behaviour of the PPTX/XLSX extractors.
pub fn extract_pdf_images(doc: &PdfDocument<'_>) -> (Vec<PdfImageInfo>, Vec<(String, Vec<u8>)>) {
    let mut infos: Vec<PdfImageInfo> = Vec::new();
    let mut image_out: Vec<(String, Vec<u8>)> = Vec::new();

    for (page_idx, page) in doc.pages().iter().enumerate() {
        let page_number = page_idx + 1;

        for object in page.objects().iter() {
            let image_obj = match object.as_image_object() {
                Some(obj) => obj,
                None => continue,
            };

            // get_raw_image() returns the decoded bitmap (ignoring page
            // transforms and soft masks), which is sufficient for retrieval
            // and always yields a valid, standalone raster we can re-encode.
            let dynimg = match image_obj.get_raw_image() {
                Ok(img) => img,
                Err(_) => continue,
            };

            let (hash_name, png_bytes) = match encode_png(&dynimg) {
                Some(pair) => pair,
                None => continue,
            };

            if !image_out.iter().any(|(n, _)| n == &hash_name) {
                image_out.push((hash_name.clone(), png_bytes));
            }

            infos.push(PdfImageInfo {
                hash_name,
                page_number,
            });
        }
    }

    (infos, image_out)
}
