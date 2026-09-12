//! PDF output. One page per processed image, sized to that image's pixel
//! dimensions — mirrors `buildPDF()`'s intent in upstream `comic2ebook.py`,
//! but via `printpdf` instead of PyMuPDF, since there is no PDF *input* to
//! support here (see [`super`] module docs).

use super::Chapter;
use crate::error::Result;

pub fn build_pdf(_chapters: &[Chapter]) -> Result<Vec<u8>> {
    todo!("one printpdf page per image, sized to the image's own pixel dimensions")
}
