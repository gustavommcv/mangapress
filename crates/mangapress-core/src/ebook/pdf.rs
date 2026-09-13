//! PDF output. One page per processed image, sized to exactly that image's
//! pixel dimensions (1 pixel = 1 point) — mirrors `buildPDF()`'s intent in
//! upstream `comic2ebook.py` (GPLv3 — reimplemented from documented
//! behavior, not copied; see `docs/adr/0007-gplv3-boundary-kcc-image-rs.md`),
//! but via `printpdf` instead of PyMuPDF, since there is no PDF *input* to
//! support here (see [`super`] module docs) — only building output pages
//! from already-processed raster images.
//!
//! `printpdf`'s `RawImage::from_dynamic_image()` accepts an already-decoded
//! image directly, so each page only needs decoding once (there is no
//! bytes -> `RawImage` -> re-encode round trip). A `DynamicImage` is always
//! normalized to `Luma8` before that call regardless of what the decoder
//! produced — this pipeline only ever writes grayscale pages, but forcing
//! it explicitly avoids depending on JPEG/PNG round-tripping through the
//! `image` crate always preserving the original color type.
//!
//! Placing an image at exactly 1 pixel = 1 point (rather than printpdf's
//! own default of 1 pixel = 1/300 inch) needs `XObjectTransform`'s `dpi`
//! field explicitly set to `72.0`: `into_pt(dpi)` computes `px * 72.0/dpi`,
//! which only reduces to `px` (matching the page's own point-sized
//! dimensions exactly) when `dpi == 72.0` — confirmed by reading
//! `XObjectTransform::get_ctms()` in printpdf's source, not assumed from
//! its (points-per-inch-flavored) documentation alone.

use super::Chapter;
use crate::error::{Error, Result};
use image::DynamicImage;
use printpdf::{Mm, Op, PdfDocument, PdfPage, PdfSaveOptions, Pt, RawImage, XObjectTransform};

/// Resolved title/author to embed in the PDF's own Info dictionary — mirrors
/// [`super::epub::EpubOptions`]'s title/author fields (see
/// [`crate::metadata::resolve`], which produces both from the CLI and any
/// `ComicInfo.xml`).
pub struct PdfOptions {
    pub title: String,
    pub author: String,
}

pub fn build_pdf(chapters: &[Chapter], options: &PdfOptions) -> Result<Vec<u8>> {
    let chapters: Vec<&Chapter> = chapters.iter().filter(|c| !c.pages.is_empty()).collect();
    if chapters.is_empty() {
        return Err(Error::EmptyBook);
    }

    let mut doc = PdfDocument::new(&options.title);
    doc.metadata.info.document_title = options.title.clone();
    doc.metadata.info.author = options.author.clone();
    let mut pdf_pages = Vec::new();

    for chapter in chapters {
        for page in &chapter.pages {
            let decoded = image::load_from_memory(&page.bytes)?;
            let (width_px, height_px) = (decoded.width(), decoded.height());
            let normalized = DynamicImage::ImageLuma8(decoded.to_luma8());

            let raw_image = RawImage::from_dynamic_image(normalized).map_err(Error::Pdf)?;
            let xobject_id = doc.add_image(&raw_image);

            let ops = vec![Op::UseXobject {
                id: xobject_id,
                transform: XObjectTransform {
                    dpi: Some(72.0),
                    ..Default::default()
                },
            }];
            pdf_pages.push(PdfPage::new(
                Mm::from(Pt(width_px as f32)),
                Mm::from(Pt(height_px as f32)),
                ops,
            ));
        }
    }

    let mut warnings = Vec::new();
    Ok(doc
        .with_pages(pdf_pages)
        .save(&PdfSaveOptions::default(), &mut warnings))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ebook::Page;
    use std::path::PathBuf;

    fn tiny_png(w: u32, h: u32) -> Vec<u8> {
        let img = image::GrayImage::from_pixel(w, h, image::Luma([128]));
        let mut bytes = Vec::new();
        image::DynamicImage::ImageLuma8(img)
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        bytes
    }

    fn chapter_with_pages(sizes: &[(u32, u32)]) -> Chapter {
        Chapter {
            relative_path: PathBuf::from("c001"),
            title: "c001".to_string(),
            pages: sizes
                .iter()
                .map(|&(w, h)| Page {
                    extension: "png".to_string(),
                    bytes: tiny_png(w, h),
                })
                .collect(),
        }
    }

    fn parse_back(bytes: &[u8]) -> PdfDocument {
        let mut warnings = Vec::new();
        PdfDocument::parse(bytes, &printpdf::PdfParseOptions::default(), &mut warnings)
            .expect("mangapress's own PDF output should parse back cleanly")
    }

    fn default_options() -> PdfOptions {
        PdfOptions {
            title: "Test Book".to_string(),
            author: "Test Author".to_string(),
        }
    }

    #[test]
    fn empty_book_is_rejected() {
        assert!(matches!(
            build_pdf(&[], &default_options()),
            Err(Error::EmptyBook)
        ));
    }

    #[test]
    fn starts_with_the_pdf_header() {
        let chapters = vec![chapter_with_pages(&[(100, 150)])];
        let bytes = build_pdf(&chapters, &default_options()).unwrap();
        assert!(bytes.starts_with(b"%PDF"), "output should be a real PDF");
    }

    #[test]
    fn page_count_matches_total_pages_across_chapters() {
        let chapters = vec![
            chapter_with_pages(&[(100, 100), (100, 100)]),
            chapter_with_pages(&[(100, 100)]),
        ];
        let bytes = build_pdf(&chapters, &default_options()).unwrap();
        let parsed = parse_back(&bytes);
        assert_eq!(parsed.pages.len(), 3);
    }

    #[test]
    fn each_page_is_sized_to_its_own_image_in_points() {
        // 72 DPI is chosen specifically so 1 pixel == 1 point -- confirmed
        // by reading XObjectTransform::get_ctms() rather than assumed from
        // documentation (see module docs).
        let chapters = vec![chapter_with_pages(&[(300, 150), (80, 200)])];
        let bytes = build_pdf(&chapters, &default_options()).unwrap();
        let parsed = parse_back(&bytes);

        assert_eq!(parsed.pages.len(), 2);
        assert!((parsed.pages[0].media_box.width.0 - 300.0).abs() < 0.5);
        assert!((parsed.pages[0].media_box.height.0 - 150.0).abs() < 0.5);
        assert!((parsed.pages[1].media_box.width.0 - 80.0).abs() < 0.5);
        assert!((parsed.pages[1].media_box.height.0 - 200.0).abs() < 0.5);
    }

    #[test]
    fn resolved_title_and_author_are_embedded_in_the_pdf_info_dictionary() {
        let chapters = vec![chapter_with_pages(&[(100, 100)])];
        let options = PdfOptions {
            title: "Chainsaw Man Vol. 01".to_string(),
            author: "Tatsuki Fujimoto".to_string(),
        };
        let bytes = build_pdf(&chapters, &options).unwrap();
        let parsed = parse_back(&bytes);
        assert_eq!(parsed.metadata.info.document_title, "Chainsaw Man Vol. 01");
        assert_eq!(parsed.metadata.info.author, "Tatsuki Fujimoto");
    }
}
