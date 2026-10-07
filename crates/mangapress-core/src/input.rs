//! Resource checks at the boundary where file bytes and image headers are trusted.
//! See ADR 0015 for the policy, its reference, and what these limits do not bound.

use crate::{Error, Result};
use image::{DynamicImage, ImageDecoder, ImageReader, Limits};
use std::io::{Cursor, Read};
use std::path::Path;

/// Maximum uncompressed bytes in one input file or archive entry (256 MiB).
pub const MAX_ENTRY_BYTES: u64 = 256 * 1024 * 1024;

/// Twice KCC 12.0.0's page parser's `Image.MAX_IMAGE_PIXELS` override.
/// This is deliberately larger than Pillow's default; see ADR 0015.
pub const MAX_IMAGE_PIXELS: u64 = 2 * 715_827_882;

/// KCC 12.0.0's webtoon splitter turns the warning above this area into an error.
pub const MAX_WEBTOON_PIXELS: u64 = 1_000_000_000;

/// Read a source file without allocating from its reported size. Used for
/// pages, metadata, separately supplied covers, and spread-label files.
pub fn read_file(path: &Path) -> Result<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    let declared_size = file.metadata()?.len();
    read_entry(file, path, declared_size)
}

pub(crate) fn read_entry(reader: impl Read, path: &Path, declared_size: u64) -> Result<Vec<u8>> {
    read_bounded(reader, path, declared_size, MAX_ENTRY_BYTES)
}

fn read_bounded(reader: impl Read, path: &Path, declared_size: u64, limit: u64) -> Result<Vec<u8>> {
    let too_large = || Error::InputTooLarge {
        path: path.to_path_buf(),
        limit,
    };
    if declared_size > limit {
        return Err(too_large());
    }
    // The header is only an early rejection, never an allocation hint or
    // the final authority: stop after one byte beyond the actual limit.
    let mut bytes = Vec::new();
    reader.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(too_large());
    }
    Ok(bytes)
}

fn check_dimensions(width: u32, height: u32, limit: u64) -> Result<()> {
    if u64::from(width) * u64::from(height) > limit {
        return Err(Error::ImageTooLarge {
            width,
            height,
            limit,
        });
    }
    Ok(())
}

fn decoder(bytes: &[u8], pixel_limit: u64) -> Result<impl ImageDecoder + '_> {
    let decoder = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()?
        .into_decoder()?;
    let (width, height) = decoder.dimensions();
    check_dimensions(width, height, pixel_limit)?;
    Ok(decoder)
}

/// Check image headers without decoding pixels (also used by passthrough mode).
pub(crate) fn image_dimensions(bytes: &[u8]) -> Result<(u32, u32)> {
    Ok(decoder(bytes, MAX_IMAGE_PIXELS)?.dimensions())
}

/// Decode through the image crate, preserving its default allocation checks
/// as well as checking the pixel count before allocating the output buffer.
pub(crate) fn decode_image(bytes: &[u8]) -> Result<DynamicImage> {
    let mut decoder = decoder(bytes, MAX_IMAGE_PIXELS)?;
    let mut limits = Limits::default();
    limits.reserve(decoder.total_bytes())?;
    decoder.set_limits(limits)?;
    Ok(DynamicImage::from_decoder(decoder)?)
}

pub(crate) fn check_webtoon_dimensions(width: u32, height: u32) -> Result<()> {
    check_dimensions(width, height, MAX_WEBTOON_PIXELS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declared_size_is_rejected_before_reading() {
        struct MustNotRead;
        impl Read for MustNotRead {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                panic!("an oversized declaration must not be read")
            }
        }
        let error = read_bounded(MustNotRead, Path::new("page.png"), 9, 8).unwrap_err();
        assert!(matches!(error, Error::InputTooLarge { limit: 8, .. }));
    }

    #[test]
    fn actual_bytes_are_bounded_even_when_the_declared_size_is_small() {
        let mut bytes = Cursor::new([0; 32]);
        let error = read_bounded(&mut bytes, Path::new("page.png"), 1, 8).unwrap_err();
        assert!(matches!(error, Error::InputTooLarge { limit: 8, .. }));
        assert_eq!(bytes.position(), 9, "do not consume the remaining input");
    }

    #[test]
    fn the_exact_byte_limit_and_short_reads_are_allowed() {
        assert_eq!(
            read_bounded(Cursor::new([3; 8]), Path::new("page.png"), 8, 8).unwrap(),
            [3; 8]
        );
        assert_eq!(
            read_bounded(Cursor::new([3; 4]), Path::new("page.png"), 8, 8).unwrap(),
            [3; 4]
        );
    }

    #[test]
    fn image_area_matches_the_reference_limit_without_integer_overflow() {
        assert!(check_dimensions(MAX_IMAGE_PIXELS as u32, 1, MAX_IMAGE_PIXELS).is_ok());
        for (width, height) in [(MAX_IMAGE_PIXELS as u32 + 1, 1), (u32::MAX, u32::MAX)] {
            assert!(matches!(
                check_dimensions(width, height, MAX_IMAGE_PIXELS),
                Err(Error::ImageTooLarge { .. })
            ));
        }
        assert!(
            check_dimensions(1, 100_000, MAX_IMAGE_PIXELS).is_ok(),
            "long strips are not a square dimension limit"
        );
    }

    #[test]
    fn an_oversized_image_header_is_refused_before_decoding_pixels() {
        let bytes = crate::test_support::oversized_bmp();
        assert!(bytes.len() < 1024);
        for error in [
            image_dimensions(&bytes).unwrap_err(),
            decode_image(&bytes).unwrap_err(),
        ] {
            assert!(matches!(
                error,
                Error::ImageTooLarge {
                    width: 50_000,
                    height: 50_000,
                    ..
                }
            ));
        }
    }

    #[test]
    fn kccs_larger_image_limit_is_not_replaced_by_pillows_default() {
        let bytes = crate::test_support::bmp_with_dimensions(20_000, 20_000);
        assert_eq!(image_dimensions(&bytes).unwrap(), (20_000, 20_000));
        assert!(check_webtoon_dimensions(MAX_WEBTOON_PIXELS as u32, 1).is_ok());
        assert!(check_webtoon_dimensions(MAX_WEBTOON_PIXELS as u32 + 1, 1).is_err());
    }

    #[test]
    fn the_existing_decoder_allocation_limit_is_still_enforced() {
        let bytes = crate::test_support::bmp_with_dimensions(20_000, 20_000);
        assert!(matches!(
            decode_image(&bytes),
            Err(Error::Image(image::ImageError::Limits(_)))
        ));
    }

    #[test]
    fn ordinary_images_keep_the_image_crates_pixels_and_color_type() {
        for image in [
            DynamicImage::ImageLuma8(image::GrayImage::from_pixel(3, 7, image::Luma([89]))),
            DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
                3,
                7,
                image::Rgba([2, 5, 8, 11]),
            )),
        ] {
            for format in [image::ImageFormat::Png, image::ImageFormat::Bmp] {
                let mut bytes = Cursor::new(Vec::new());
                image.write_to(&mut bytes, format).unwrap();
                let expected = image::load_from_memory(bytes.get_ref()).unwrap();
                let actual = decode_image(bytes.get_ref()).unwrap();
                assert_eq!(actual.color(), expected.color());
                assert_eq!(actual.as_bytes(), expected.as_bytes());
            }
        }
    }

    #[test]
    fn decoder_and_io_errors_are_not_hidden() {
        assert!(matches!(
            decode_image(b"not an image"),
            Err(Error::Image(_))
        ));
        struct BrokenReader;
        impl Read for BrokenReader {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "test failure",
                ))
            }
        }
        assert!(matches!(
            read_bounded(BrokenReader, Path::new("page.png"), 1, 8),
            Err(Error::Io(_))
        ));
    }
}
